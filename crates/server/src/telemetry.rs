use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use tokio::{
    sync::{Notify, mpsc, oneshot},
    task::JoinHandle,
    time::{Instant, sleep_until, timeout, timeout_at},
};

use crate::store::{self, ReportStoreError, ReportWrite};

pub const MAX_QUEUE_CAPACITY: usize = 1_024;
pub const MAX_PENDING_PER_HOST: usize = 16;
pub const MAX_BATCH_SIZE: usize = 512;
pub const MAX_FLUSH_LATENCY: Duration = Duration::from_secs(1);
pub const MAX_ENQUEUE_WAIT: Duration = Duration::from_millis(250);
pub const MAX_REQUEST_TIME: Duration = Duration::from_secs(30);
pub const MAX_SHUTDOWN_DRAIN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy)]
pub struct TelemetryWriterConfig {
    queue_capacity: usize,
    batch_size: usize,
    flush_latency: Duration,
    enqueue_wait: Duration,
    request_time: Duration,
    shutdown_drain: Duration,
}

impl TelemetryWriterConfig {
    pub fn new(
        queue_capacity: usize,
        batch_size: usize,
        flush_latency: Duration,
        enqueue_wait: Duration,
        request_time: Duration,
        shutdown_drain: Duration,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=MAX_QUEUE_CAPACITY).contains(&queue_capacity),
            "telemetry queue capacity must be between 1 and {MAX_QUEUE_CAPACITY}"
        );
        anyhow::ensure!(
            (1..=MAX_BATCH_SIZE.min(queue_capacity)).contains(&batch_size),
            "telemetry batch size must be between 1 and the queue capacity"
        );
        anyhow::ensure!(
            !flush_latency.is_zero() && flush_latency <= MAX_FLUSH_LATENCY,
            "telemetry flush latency must be between 1ms and 1s"
        );
        anyhow::ensure!(
            !enqueue_wait.is_zero() && enqueue_wait <= MAX_ENQUEUE_WAIT,
            "telemetry enqueue wait must be between 1ms and 250ms"
        );
        anyhow::ensure!(
            request_time >= Duration::from_millis(100)
                && request_time <= MAX_REQUEST_TIME
                && request_time > enqueue_wait + flush_latency,
            "telemetry request time must be 100ms-30s and exceed enqueue wait plus flush latency"
        );
        anyhow::ensure!(
            shutdown_drain >= Duration::from_millis(100) && shutdown_drain <= MAX_SHUTDOWN_DRAIN,
            "telemetry shutdown drain must be between 100ms and 60s"
        );
        Ok(Self {
            queue_capacity,
            batch_size,
            flush_latency,
            enqueue_wait,
            request_time,
            shutdown_drain,
        })
    }

    pub fn production() -> Self {
        Self::new(
            256,
            64,
            Duration::from_millis(25),
            Duration::from_millis(10),
            Duration::from_secs(10),
            Duration::from_secs(15),
        )
        .expect("the built-in telemetry writer limits are valid")
    }

    pub fn queue_capacity(self) -> usize {
        self.queue_capacity
    }

    pub fn batch_size(self) -> usize {
        self.batch_size
    }

    pub fn flush_latency(self) -> Duration {
        self.flush_latency
    }

    pub fn enqueue_wait(self) -> Duration {
        self.enqueue_wait
    }

    pub fn request_time(self) -> Duration {
        self.request_time
    }

    pub fn shutdown_drain(self) -> Duration {
        self.shutdown_drain
    }
}

impl Default for TelemetryWriterConfig {
    fn default() -> Self {
        Self::production()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TelemetryWriterStats {
    pub enqueued: u64,
    pub batches: u64,
    pub completed: u64,
    pub failed: u64,
    pub abandoned_acknowledgements: u64,
    pub largest_batch: usize,
}

#[derive(Default)]
struct WriterCounters {
    enqueued: AtomicU64,
    batches: AtomicU64,
    completed: AtomicU64,
    failed: AtomicU64,
    abandoned_acknowledgements: AtomicU64,
    largest_batch: AtomicUsize,
}

impl WriterCounters {
    fn snapshot(&self) -> TelemetryWriterStats {
        TelemetryWriterStats {
            enqueued: self.enqueued.load(Ordering::Relaxed),
            batches: self.batches.load(Ordering::Relaxed),
            completed: self.completed.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            abandoned_acknowledgements: self.abandoned_acknowledgements.load(Ordering::Relaxed),
            largest_batch: self.largest_batch.load(Ordering::Relaxed),
        }
    }
}

struct ShutdownState {
    requested: AtomicBool,
    notify: Notify,
}

impl Default for ShutdownState {
    fn default() -> Self {
        Self {
            requested: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }
}

struct QueuedReport {
    write: ReportWrite,
    completion: oneshot::Sender<anyhow::Result<(bool, DateTime<Utc>)>>,
    owner_permit: tokio::sync::OwnedSemaphorePermit,
}

#[derive(Clone)]
pub struct TelemetryWriter {
    sender: mpsc::Sender<QueuedReport>,
    config: TelemetryWriterConfig,
    counters: Arc<WriterCounters>,
    owners: Arc<
        std::sync::Mutex<
            std::collections::HashMap<String, std::sync::Weak<tokio::sync::Semaphore>>,
        >,
    >,
}

pub struct TelemetryWriterTask {
    shutdown: Arc<ShutdownState>,
    join: JoinHandle<()>,
    drain_timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetrySubmitError {
    #[error("telemetry queue is full")]
    QueueFull,
    #[error("telemetry writer is unavailable")]
    WriterUnavailable,
    #[error("telemetry persistence exceeded its response deadline")]
    ResponseDeadline,
    #[error("telemetry report persistence failed")]
    Store(#[source] anyhow::Error),
}

impl TelemetrySubmitError {
    pub fn store_error(&self) -> Option<&anyhow::Error> {
        match self {
            Self::Store(error) => Some(error),
            _ => None,
        }
    }

    pub fn is_unauthorized(&self) -> bool {
        self.store_error()
            .and_then(|error| error.downcast_ref::<ReportStoreError>())
            .is_some_and(|error| matches!(error, ReportStoreError::Unauthorized))
    }

    pub fn is_report_id_conflict(&self) -> bool {
        self.store_error()
            .and_then(|error| error.downcast_ref::<ReportStoreError>())
            .is_some_and(|error| matches!(error, ReportStoreError::ReportIdConflict))
    }
}

impl TelemetryWriter {
    pub fn start(pool: SqlitePool, config: TelemetryWriterConfig) -> (Self, TelemetryWriterTask) {
        let (sender, receiver) = mpsc::channel(config.queue_capacity);
        let counters = Arc::new(WriterCounters::default());
        let shutdown = Arc::new(ShutdownState::default());
        let join = tokio::spawn(run_writer(
            pool,
            receiver,
            config,
            counters.clone(),
            shutdown.clone(),
        ));
        (
            Self {
                sender,
                config,
                counters,
                owners: Default::default(),
            },
            TelemetryWriterTask {
                shutdown,
                join,
                drain_timeout: config.shutdown_drain,
            },
        )
    }

    pub async fn submit(
        &self,
        write: ReportWrite,
    ) -> Result<(bool, DateTime<Utc>), TelemetrySubmitError> {
        let owner = {
            let mut owners = self
                .owners
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            owners.retain(|_, weak| weak.strong_count() > 0);
            if let Some(value) = owners
                .get(write.host_id())
                .and_then(std::sync::Weak::upgrade)
            {
                value
            } else {
                let value = Arc::new(tokio::sync::Semaphore::new(MAX_PENDING_PER_HOST));
                owners.insert(write.host_id().to_owned(), Arc::downgrade(&value));
                value
            }
        };
        let owner_permit = owner
            .try_acquire_owned()
            .map_err(|_| TelemetrySubmitError::QueueFull)?;
        let deadline = Instant::now() + self.config.request_time;
        let permit = match timeout(self.config.enqueue_wait, self.sender.reserve()).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => return Err(TelemetrySubmitError::WriterUnavailable),
            Err(_) => return Err(TelemetrySubmitError::QueueFull),
        };
        let (completion, response) = oneshot::channel();
        permit.send(QueuedReport {
            write,
            completion,
            owner_permit,
        });
        self.counters.enqueued.fetch_add(1, Ordering::Relaxed);
        match timeout_at(deadline, response).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(error))) => Err(TelemetrySubmitError::Store(error)),
            Ok(Err(_)) => Err(TelemetrySubmitError::WriterUnavailable),
            Err(_) => Err(TelemetrySubmitError::ResponseDeadline),
        }
    }

    pub fn stats(&self) -> TelemetryWriterStats {
        self.counters.snapshot()
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
}

impl TelemetryWriterTask {
    pub async fn run_until(
        mut self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), String> {
        tokio::select! {
            _ = xcss_server_runtime::wait_for_shutdown(&mut shutdown) => {
                self.shutdown().await.map_err(|error| error.to_string())
            }
            result = &mut self.join => result.map_err(|error| error.to_string()),
        }
    }

    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        self.shutdown.requested.store(true, Ordering::Release);
        self.shutdown.notify.notify_one();
        match timeout(self.drain_timeout, &mut self.join).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(anyhow::anyhow!("telemetry writer task failed: {error}")),
            Err(_) => {
                self.join.abort();
                let _ = (&mut self.join).await;
                anyhow::bail!("telemetry writer drain exceeded its shutdown deadline")
            }
        }
    }
}

impl Drop for TelemetryWriterTask {
    fn drop(&mut self) {
        self.join.abort();
    }
}

async fn run_writer(
    pool: SqlitePool,
    mut receiver: mpsc::Receiver<QueuedReport>,
    config: TelemetryWriterConfig,
    counters: Arc<WriterCounters>,
    shutdown: Arc<ShutdownState>,
) {
    let mut draining = false;
    loop {
        if shutdown.requested.load(Ordering::Acquire) && !draining {
            receiver.close();
            draining = true;
        }
        let first = if draining {
            receiver.recv().await
        } else {
            tokio::select! {
                report = receiver.recv() => report,
                _ = shutdown.notify.notified() => {
                    if shutdown.requested.load(Ordering::Acquire) {
                        receiver.close();
                        draining = true;
                    }
                    receiver.recv().await
                }
            }
        };
        let Some(first) = first else {
            break;
        };

        let mut batch = Vec::with_capacity(config.batch_size);
        batch.push(first);
        let flush_at = Instant::now() + config.flush_latency;
        while batch.len() < config.batch_size {
            if shutdown.requested.load(Ordering::Acquire) && !draining {
                receiver.close();
                draining = true;
            }
            if draining {
                match receiver.try_recv() {
                    Ok(report) => batch.push(report),
                    Err(
                        mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected,
                    ) => {
                        break;
                    }
                }
                continue;
            }
            tokio::select! {
                report = receiver.recv() => match report {
                    Some(report) => batch.push(report),
                    None => break,
                },
                _ = sleep_until(flush_at) => break,
                _ = shutdown.notify.notified() => {
                    if shutdown.requested.load(Ordering::Acquire) {
                        receiver.close();
                        draining = true;
                    }
                }
            }
        }
        persist_batch(&pool, batch, &counters).await;
    }
}

async fn persist_batch(pool: &SqlitePool, batch: Vec<QueuedReport>, counters: &WriterCounters) {
    counters.batches.fetch_add(1, Ordering::Relaxed);
    counters
        .largest_batch
        .fetch_max(batch.len(), Ordering::Relaxed);
    let mut writes = Vec::with_capacity(batch.len());
    let mut completions = Vec::with_capacity(batch.len());
    for report in batch {
        writes.push(report.write);
        completions.push((report.completion, report.owner_permit));
    }

    let results = match store::store_report_batch(pool, &writes).await {
        Ok(results) => results,
        Err(error) => {
            tracing::warn!(
                error = %error,
                report_count = writes.len(),
                "telemetry writer batch transaction failed"
            );
            counters
                .failed
                .fetch_add(writes.len() as u64, Ordering::Relaxed);
            for (completion, _owner_permit) in completions {
                if completion
                    .send(Err(anyhow::anyhow!(
                        "telemetry batch transaction did not commit"
                    )))
                    .is_err()
                {
                    counters
                        .abandoned_acknowledgements
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
            return;
        }
    };

    for (result, (completion, _owner_permit)) in results.into_iter().zip(completions) {
        if result.is_ok() {
            counters.completed.fetch_add(1, Ordering::Relaxed);
        } else {
            counters.failed.fetch_add(1, Ordering::Relaxed);
        }
        if completion.send(result).is_err() {
            counters
                .abandoned_acknowledgements
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_has_hard_resource_and_time_limits() {
        assert!(
            TelemetryWriterConfig::new(
                0,
                1,
                Duration::from_millis(1),
                Duration::from_millis(1),
                Duration::from_millis(100),
                Duration::from_millis(100),
            )
            .is_err()
        );
        assert!(
            TelemetryWriterConfig::new(
                MAX_QUEUE_CAPACITY + 1,
                1,
                Duration::from_millis(1),
                Duration::from_millis(1),
                Duration::from_millis(100),
                Duration::from_millis(100),
            )
            .is_err()
        );
        assert!(
            TelemetryWriterConfig::new(
                8,
                9,
                Duration::from_millis(1),
                Duration::from_millis(1),
                Duration::from_millis(100),
                Duration::from_millis(100),
            )
            .is_err()
        );
        assert!(
            TelemetryWriterConfig::new(
                8,
                4,
                MAX_FLUSH_LATENCY + Duration::from_millis(1),
                Duration::from_millis(1),
                Duration::from_millis(100),
                Duration::from_millis(100),
            )
            .is_err()
        );
        assert!(
            TelemetryWriterConfig::new(
                8,
                4,
                Duration::from_millis(1),
                MAX_ENQUEUE_WAIT + Duration::from_millis(1),
                Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .is_err()
        );
        assert!(
            TelemetryWriterConfig::new(
                8,
                4,
                Duration::from_millis(1),
                Duration::from_millis(1),
                MAX_REQUEST_TIME + Duration::from_millis(1),
                Duration::from_secs(1),
            )
            .is_err()
        );
    }
}
