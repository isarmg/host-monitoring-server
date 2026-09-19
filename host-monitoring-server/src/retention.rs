use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use chrono::{DateTime, Timelike, Utc};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool};
use tokio::{
    sync::Notify,
    task::JoinHandle,
    time::{sleep, timeout},
};
use uuid::Uuid;

pub const MIN_RAW_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
pub const MAX_RAW_RETENTION: Duration = Duration::from_secs(365 * 24 * 60 * 60);
pub const MAX_AGGREGATE_RETENTION: Duration = Duration::from_secs(3_650 * 24 * 60 * 60);
pub const MIN_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(1);
pub const MAX_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub const MAX_RETENTION_BATCH_SIZE: usize = 512;
pub const MAX_RETENTION_TRANSACTIONS: usize = 30;
pub const MAX_RETENTION_RUN_TIME: Duration = Duration::from_secs(10);
pub const MAX_RETENTION_YIELD: Duration = Duration::from_millis(100);
pub const DEFAULT_RAW_RETENTION_DAYS: u64 = 7;
pub const DEFAULT_AGGREGATE_RETENTION_DAYS: u64 = 365;
pub const DEFAULT_MAINTENANCE_INTERVAL_SECONDS: u64 = 300;
pub const DEFAULT_RETENTION_BATCH_SIZE: usize = 256;
pub const DEFAULT_RETENTION_TRANSACTIONS: usize = 12;
pub const DEFAULT_RETENTION_RUN_MILLISECONDS: u64 = 2_000;
pub const DEFAULT_RETENTION_YIELD_MILLISECONDS: u64 = 10;

const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);
const METRIC_NAMES: [&str; 9] = [
    "cpu_usage_percent",
    "memory_usage_percent",
    "network_received_bytes_per_second",
    "network_transmitted_bytes_per_second",
    "disk_read_bytes_per_second",
    "disk_written_bytes_per_second",
    "max_temperature_celsius",
    "gpu_utilization_percent",
    "gpu_memory_usage_percent",
];

#[derive(Debug, Clone, Copy)]
pub struct RetentionConfig {
    raw_retention: Duration,
    aggregate_retention: Duration,
    maintenance_interval: Duration,
    batch_size: usize,
    max_transactions_per_run: usize,
    max_run_time: Duration,
    yield_between_transactions: Duration,
}

impl RetentionConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        raw_retention: Duration,
        aggregate_retention: Duration,
        maintenance_interval: Duration,
        batch_size: usize,
        max_transactions_per_run: usize,
        max_run_time: Duration,
        yield_between_transactions: Duration,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (MIN_RAW_RETENTION..=MAX_RAW_RETENTION).contains(&raw_retention),
            "raw telemetry retention must be between 1 and 365 days"
        );
        anyhow::ensure!(
            aggregate_retention > raw_retention && aggregate_retention <= MAX_AGGREGATE_RETENTION,
            "aggregate telemetry retention must exceed raw retention and be at most 3650 days"
        );
        anyhow::ensure!(
            (MIN_MAINTENANCE_INTERVAL..=MAX_MAINTENANCE_INTERVAL).contains(&maintenance_interval),
            "retention maintenance interval must be between 1 second and 24 hours"
        );
        anyhow::ensure!(
            (1..=MAX_RETENTION_BATCH_SIZE).contains(&batch_size),
            "retention batch size must be between 1 and {MAX_RETENTION_BATCH_SIZE}"
        );
        anyhow::ensure!(
            (3..=MAX_RETENTION_TRANSACTIONS).contains(&max_transactions_per_run),
            "retention transactions per run must be between 3 and {MAX_RETENTION_TRANSACTIONS}"
        );
        anyhow::ensure!(
            max_run_time >= Duration::from_millis(100)
                && max_run_time <= MAX_RETENTION_RUN_TIME
                && max_run_time < maintenance_interval,
            "retention run time must be 100ms-10s and shorter than its interval"
        );
        anyhow::ensure!(
            !yield_between_transactions.is_zero()
                && yield_between_transactions <= MAX_RETENTION_YIELD,
            "retention writer yield must be between 1ms and 100ms"
        );
        Ok(Self {
            raw_retention,
            aggregate_retention,
            maintenance_interval,
            batch_size,
            max_transactions_per_run,
            max_run_time,
            yield_between_transactions,
        })
    }

    pub fn production() -> Self {
        Self::new(
            Duration::from_secs(DEFAULT_RAW_RETENTION_DAYS * 24 * 60 * 60),
            Duration::from_secs(DEFAULT_AGGREGATE_RETENTION_DAYS * 24 * 60 * 60),
            Duration::from_secs(DEFAULT_MAINTENANCE_INTERVAL_SECONDS),
            DEFAULT_RETENTION_BATCH_SIZE,
            DEFAULT_RETENTION_TRANSACTIONS,
            Duration::from_millis(DEFAULT_RETENTION_RUN_MILLISECONDS),
            Duration::from_millis(DEFAULT_RETENTION_YIELD_MILLISECONDS),
        )
        .expect("the built-in retention limits are valid")
    }

    pub fn raw_retention(self) -> Duration {
        self.raw_retention
    }

    pub fn aggregate_retention(self) -> Duration {
        self.aggregate_retention
    }

    pub fn maintenance_interval(self) -> Duration {
        self.maintenance_interval
    }

    pub fn batch_size(self) -> usize {
        self.batch_size
    }

    pub fn max_transactions_per_run(self) -> usize {
        self.max_transactions_per_run
    }

    pub fn max_run_time(self) -> Duration {
        self.max_run_time
    }

    pub fn yield_between_transactions(self) -> Duration {
        self.yield_between_transactions
    }
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self::production()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionRunOutcome {
    pub aggregated_reports: u64,
    pub deleted_raw_reports: u64,
    pub deleted_hourly_aggregates: u64,
    pub transactions: usize,
    pub backlog_remaining: bool,
}

impl RetentionRunOutcome {
    fn add(&mut self, other: Self) {
        self.aggregated_reports = self
            .aggregated_reports
            .saturating_add(other.aggregated_reports);
        self.deleted_raw_reports = self
            .deleted_raw_reports
            .saturating_add(other.deleted_raw_reports);
        self.deleted_hourly_aggregates = self
            .deleted_hourly_aggregates
            .saturating_add(other.deleted_hourly_aggregates);
        self.transactions = self.transactions.saturating_add(other.transactions);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionMaintenanceStats {
    pub running: bool,
    pub runs: u64,
    pub failures: u64,
    pub consecutive_failures: u64,
    pub last_success_unix_seconds: i64,
    pub backlog_remaining: bool,
    pub aggregated_reports: u64,
    pub deleted_raw_reports: u64,
    pub deleted_hourly_aggregates: u64,
}

#[derive(Default)]
struct MaintenanceCounters {
    running: AtomicBool,
    runs: AtomicU64,
    failures: AtomicU64,
    consecutive_failures: AtomicU64,
    last_success_unix_seconds: AtomicI64,
    backlog_remaining: AtomicBool,
    aggregated_reports: AtomicU64,
    deleted_raw_reports: AtomicU64,
    deleted_hourly_aggregates: AtomicU64,
}

impl MaintenanceCounters {
    fn snapshot(&self) -> RetentionMaintenanceStats {
        RetentionMaintenanceStats {
            running: self.running.load(Ordering::Acquire),
            runs: self.runs.load(Ordering::Relaxed),
            failures: self.failures.load(Ordering::Relaxed),
            consecutive_failures: self.consecutive_failures.load(Ordering::Relaxed),
            last_success_unix_seconds: self.last_success_unix_seconds.load(Ordering::Relaxed),
            backlog_remaining: self.backlog_remaining.load(Ordering::Relaxed),
            aggregated_reports: self.aggregated_reports.load(Ordering::Relaxed),
            deleted_raw_reports: self.deleted_raw_reports.load(Ordering::Relaxed),
            deleted_hourly_aggregates: self.deleted_hourly_aggregates.load(Ordering::Relaxed),
        }
    }

    fn record(&self, outcome: RetentionRunOutcome) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.last_success_unix_seconds
            .store(Utc::now().timestamp(), Ordering::Relaxed);
        self.backlog_remaining
            .store(outcome.backlog_remaining, Ordering::Relaxed);
        self.aggregated_reports
            .fetch_add(outcome.aggregated_reports, Ordering::Relaxed);
        self.deleted_raw_reports
            .fetch_add(outcome.deleted_raw_reports, Ordering::Relaxed);
        self.deleted_hourly_aggregates
            .fetch_add(outcome.deleted_hourly_aggregates, Ordering::Relaxed);
    }
}

#[derive(Clone)]
pub struct RetentionMaintenance {
    counters: Arc<MaintenanceCounters>,
}

pub struct RetentionMaintenanceTask {
    shutdown: Arc<ShutdownState>,
    join: JoinHandle<()>,
}

#[derive(Default)]
struct ShutdownState {
    requested: AtomicBool,
    notify: Notify,
}

struct RunningGuard(Arc<MaintenanceCounters>);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::Release);
    }
}

impl RetentionMaintenance {
    pub fn start(pool: SqlitePool, config: RetentionConfig) -> (Self, RetentionMaintenanceTask) {
        let counters = Arc::new(MaintenanceCounters::default());
        counters.running.store(true, Ordering::Release);
        let shutdown = Arc::new(ShutdownState::default());
        let join = tokio::spawn(run_maintenance(
            pool,
            config,
            counters.clone(),
            shutdown.clone(),
        ));
        (
            Self { counters },
            RetentionMaintenanceTask { shutdown, join },
        )
    }

    pub fn stats(&self) -> RetentionMaintenanceStats {
        self.counters.snapshot()
    }

    pub fn is_running(&self) -> bool {
        self.counters.running.load(Ordering::Acquire)
    }

    pub fn is_healthy(&self) -> bool {
        self.is_running() && self.counters.consecutive_failures.load(Ordering::Relaxed) < 3
    }
}

impl RetentionMaintenanceTask {
    pub async fn run_until(
        mut self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), String> {
        tokio::select! {
            _ = sarmg_server_runtime::wait_for_shutdown(&mut shutdown) => {
                self.shutdown().await.map_err(|error| error.to_string())
            }
            result = &mut self.join => result.map_err(|error| error.to_string()),
        }
    }

    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        self.shutdown.requested.store(true, Ordering::Release);
        self.shutdown.notify.notify_one();
        match timeout(SHUTDOWN_WAIT, &mut self.join).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(anyhow::anyhow!(
                "telemetry retention maintenance task failed: {error}"
            )),
            Err(_) => {
                self.join.abort();
                let _ = (&mut self.join).await;
                anyhow::bail!("telemetry retention maintenance did not stop before its deadline")
            }
        }
    }
}

impl Drop for RetentionMaintenanceTask {
    fn drop(&mut self) {
        self.join.abort();
    }
}

async fn run_maintenance(
    pool: SqlitePool,
    config: RetentionConfig,
    counters: Arc<MaintenanceCounters>,
    shutdown: Arc<ShutdownState>,
) {
    let _guard = RunningGuard(counters.clone());
    let mut delay = Duration::ZERO;
    loop {
        tokio::select! {
            _ = shutdown.notify.notified() => {
                if shutdown.requested.load(Ordering::Acquire) {
                    break;
                }
            }
            _ = sleep(delay) => {
                counters.runs.fetch_add(1, Ordering::Relaxed);
                let maintenance = run_once_at(&pool, config, Utc::now());
                tokio::select! {
                    _ = shutdown.notify.notified() => {
                        if shutdown.requested.load(Ordering::Acquire) {
                            break;
                        }
                    }
                    result = maintenance => match result {
                        Ok(outcome) => {
                            delay = if outcome.backlog_remaining {
                                MIN_MAINTENANCE_INTERVAL
                            } else {
                                config.maintenance_interval
                            };
                            counters.record(outcome);
                        }
                        Err(error) => {
                            counters.failures.fetch_add(1, Ordering::Relaxed);
                            let consecutive = counters.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
                            delay = MIN_MAINTENANCE_INTERVAL
                                .saturating_mul(1_u32 << consecutive.min(8))
                                .min(config.maintenance_interval);
                            tracing::warn!(error = %error, "bounded telemetry retention maintenance failed; a later run will retry");
                        }
                    }
                }
            }
        }
    }
}

pub async fn run_once_at(
    pool: &SqlitePool,
    config: RetentionConfig,
    now: DateTime<Utc>,
) -> anyhow::Result<RetentionRunOutcome> {
    timeout(config.max_run_time, run_cycle_at(pool, config, now))
        .await
        .context("telemetry retention run exceeded its time budget")?
}

async fn run_cycle_at(
    pool: &SqlitePool,
    config: RetentionConfig,
    now: DateTime<Utc>,
) -> anyhow::Result<RetentionRunOutcome> {
    let raw_delta = chrono::Duration::from_std(config.raw_retention)
        .context("convert raw telemetry retention")?;
    let aggregate_delta = chrono::Duration::from_std(config.aggregate_retention)
        .context("convert aggregate telemetry retention")?;
    let raw_cutoff = now
        .checked_sub_signed(raw_delta)
        .context("raw telemetry cutoff is outside the supported range")?;
    let aggregate_cutoff = now
        .checked_sub_signed(aggregate_delta)
        .context("aggregate telemetry cutoff is outside the supported range")?;
    let mut outcome = RetentionRunOutcome::default();

    while outcome.transactions < config.max_transactions_per_run {
        let rolled = aggregate_raw_batch(pool, raw_cutoff, now, config.batch_size).await?;
        outcome.add(RetentionRunOutcome {
            aggregated_reports: rolled,
            transactions: 1,
            ..RetentionRunOutcome::default()
        });
        yield_to_telemetry(config.yield_between_transactions, rolled).await;
        if outcome.transactions >= config.max_transactions_per_run {
            break;
        }

        let deleted = delete_aggregated_raw_batch(pool, raw_cutoff, config.batch_size).await?;
        outcome.add(RetentionRunOutcome {
            deleted_raw_reports: deleted,
            transactions: 1,
            ..RetentionRunOutcome::default()
        });
        yield_to_telemetry(config.yield_between_transactions, deleted).await;
        if outcome.transactions >= config.max_transactions_per_run {
            break;
        }

        let deleted_aggregates =
            delete_expired_aggregate_batch(pool, aggregate_cutoff, config.batch_size).await?;
        outcome.add(RetentionRunOutcome {
            deleted_hourly_aggregates: deleted_aggregates,
            transactions: 1,
            ..RetentionRunOutcome::default()
        });
        yield_to_telemetry(config.yield_between_transactions, deleted_aggregates).await;

        if rolled == 0 && deleted == 0 && deleted_aggregates == 0 {
            break;
        }
    }
    outcome.backlog_remaining = outcome.transactions >= config.max_transactions_per_run
        && (outcome.aggregated_reports > 0
            || outcome.deleted_raw_reports > 0
            || outcome.deleted_hourly_aggregates > 0);
    Ok(outcome)
}

async fn yield_to_telemetry(duration: Duration, affected: u64) {
    tokio::task::yield_now().await;
    if affected > 0 {
        sleep(duration).await;
    }
}

#[derive(Debug, FromRow)]
struct RawScalarReport {
    report_id: Uuid,
    host_id: Uuid,
    collected_at: DateTime<Utc>,
    cpu_usage_percent: Option<f64>,
    memory_usage_percent: Option<f64>,
    network_received_bytes_per_second: Option<f64>,
    network_transmitted_bytes_per_second: Option<f64>,
    disk_read_bytes_per_second: Option<f64>,
    disk_written_bytes_per_second: Option<f64>,
    max_temperature_celsius: Option<f64>,
    gpu_utilization_percent: Option<f64>,
    gpu_memory_usage_percent: Option<f64>,
}

impl RawScalarReport {
    fn metrics(&self) -> [Option<f64>; 9] {
        [
            self.cpu_usage_percent,
            self.memory_usage_percent,
            self.network_received_bytes_per_second,
            self.network_transmitted_bytes_per_second,
            self.disk_read_bytes_per_second,
            self.disk_written_bytes_per_second,
            self.max_temperature_celsius,
            self.gpu_utilization_percent,
            self.gpu_memory_usage_percent,
        ]
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ScalarAggregate {
    count: i64,
    min: Option<f64>,
    max: Option<f64>,
    avg: Option<f64>,
}

impl ScalarAggregate {
    fn record(&mut self, value: Option<f64>) {
        let Some(value) = value else {
            return;
        };
        self.count += 1;
        self.min = Some(self.min.map_or(value, |current| current.min(value)));
        self.max = Some(self.max.map_or(value, |current| current.max(value)));
        self.avg = Some(match self.avg {
            Some(current) => current + (value - current) / self.count as f64,
            None => value,
        });
    }
}

#[derive(Debug)]
struct HourlyAggregate {
    host_id: Uuid,
    bucket_start: DateTime<Utc>,
    interval_start: DateTime<Utc>,
    interval_end: DateTime<Utc>,
    sample_count: i64,
    metrics: [ScalarAggregate; 9],
}

impl HourlyAggregate {
    fn new(row: &RawScalarReport, bucket_start: DateTime<Utc>) -> Self {
        let mut aggregate = Self {
            host_id: row.host_id,
            bucket_start,
            interval_start: row.collected_at,
            interval_end: row.collected_at,
            sample_count: 0,
            metrics: [ScalarAggregate::default(); 9],
        };
        aggregate.record(row);
        aggregate
    }

    fn record(&mut self, row: &RawScalarReport) {
        self.interval_start = self.interval_start.min(row.collected_at);
        self.interval_end = self.interval_end.max(row.collected_at);
        self.sample_count += 1;
        for (aggregate, value) in self.metrics.iter_mut().zip(row.metrics()) {
            aggregate.record(value);
        }
    }
}

async fn aggregate_raw_batch(
    pool: &SqlitePool,
    cutoff: DateTime<Utc>,
    now: DateTime<Utc>,
    batch_size: usize,
) -> anyhow::Result<u64> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let reports: Vec<RawScalarReport> = sqlx::query_as(
        r#"SELECT r.report_id,r.host_id,r.collected_at,
                  r.cpu_usage_percent,r.memory_usage_percent,
                  r.network_received_bytes_per_second,r.network_transmitted_bytes_per_second,
                  r.disk_read_bytes_per_second,r.disk_written_bytes_per_second,
                  r.max_temperature_celsius,r.gpu_utilization_percent,r.gpu_memory_usage_percent
             FROM client_metric_reports r
            WHERE r.aggregated_at IS NULL
              AND r.collected_at < ?
              AND NOT EXISTS (
                    SELECT 1 FROM monitored_hosts h
                     WHERE h.latest_report_id=r.report_id
              )
            ORDER BY r.collected_at,r.report_id
            LIMIT ?"#,
    )
    .bind(cutoff)
    .bind(i64::try_from(batch_size)?)
    .fetch_all(&mut *tx)
    .await?;
    if reports.is_empty() {
        tx.rollback().await?;
        return Ok(0);
    }

    let mut buckets = BTreeMap::new();
    for report in &reports {
        let bucket_start = report
            .collected_at
            .with_minute(0)
            .and_then(|value| value.with_second(0))
            .and_then(|value| value.with_nanosecond(0))
            .context("truncate telemetry timestamp to a UTC hour")?;
        buckets
            .entry((report.host_id, bucket_start))
            .and_modify(|aggregate: &mut HourlyAggregate| aggregate.record(report))
            .or_insert_with(|| HourlyAggregate::new(report, bucket_start));
    }

    let upsert_sql = hourly_upsert_sql();
    for aggregate in buckets.values() {
        let mut query = sqlx::query(&upsert_sql)
            .bind(aggregate.host_id)
            .bind(aggregate.bucket_start)
            .bind(aggregate.interval_start)
            .bind(aggregate.interval_end)
            .bind(aggregate.sample_count);
        for metric in aggregate.metrics {
            query = query
                .bind(metric.count)
                .bind(metric.min)
                .bind(metric.max)
                .bind(metric.avg);
        }
        query.bind(now).execute(&mut *tx).await?;
    }

    let mut mark = QueryBuilder::<Sqlite>::new("UPDATE client_metric_reports SET aggregated_at=");
    mark.push_bind(now)
        .push(" WHERE aggregated_at IS NULL AND report_id IN (");
    let mut ids = mark.separated(",");
    for report in &reports {
        ids.push_bind(report.report_id);
    }
    ids.push_unseparated(")");
    let marked = mark.build().execute(&mut *tx).await?.rows_affected();
    anyhow::ensure!(
        marked == reports.len() as u64,
        "telemetry aggregate marker count changed inside its write transaction"
    );
    tx.commit().await?;
    Ok(marked)
}

async fn delete_aggregated_raw_batch(
    pool: &SqlitePool,
    received_cutoff: DateTime<Utc>,
    batch_size: usize,
) -> anyhow::Result<u64> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let result = sqlx::query(
        r#"DELETE FROM client_metric_reports
            WHERE report_id IN (
                SELECT r.report_id FROM client_metric_reports r
                 WHERE r.aggregated_at IS NOT NULL
                   AND r.received_at < ?
                   AND NOT EXISTS (
                       SELECT 1 FROM monitored_hosts h
                        WHERE h.latest_report_id=r.report_id
                   )
                 ORDER BY r.aggregated_at,r.report_id
                 LIMIT ?
            )
              AND NOT EXISTS (
                  SELECT 1 FROM monitored_hosts h
                   WHERE h.latest_report_id=client_metric_reports.report_id
              )"#,
    )
    .bind(received_cutoff)
    .bind(i64::try_from(batch_size)?)
    .execute(&mut *tx)
    .await?;
    let deleted = result.rows_affected();
    if deleted == 0 {
        tx.rollback().await?;
    } else {
        tx.commit().await?;
    }
    Ok(deleted)
}

async fn delete_expired_aggregate_batch(
    pool: &SqlitePool,
    cutoff: DateTime<Utc>,
    batch_size: usize,
) -> anyhow::Result<u64> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let result = sqlx::query(
        r#"DELETE FROM client_metric_hourly_aggregates
            WHERE rowid IN (
                SELECT rowid FROM client_metric_hourly_aggregates
                 WHERE interval_end < ?
                 ORDER BY interval_end,host_id,bucket_start
                 LIMIT ?
            )"#,
    )
    .bind(cutoff)
    .bind(i64::try_from(batch_size)?)
    .execute(&mut *tx)
    .await?;
    let deleted = result.rows_affected();
    if deleted == 0 {
        tx.rollback().await?;
    } else {
        tx.commit().await?;
    }
    Ok(deleted)
}

fn hourly_upsert_sql() -> String {
    let mut columns = vec![
        "host_id".to_string(),
        "bucket_start".to_string(),
        "interval_start".to_string(),
        "interval_end".to_string(),
        "sample_count".to_string(),
    ];
    for metric in METRIC_NAMES {
        columns.extend([
            format!("{metric}_count"),
            format!("{metric}_min"),
            format!("{metric}_max"),
            format!("{metric}_avg"),
        ]);
    }
    columns.push("updated_at".to_string());

    let mut updates = vec![
        "interval_start=MIN(client_metric_hourly_aggregates.interval_start,excluded.interval_start)"
            .to_string(),
        "interval_end=MAX(client_metric_hourly_aggregates.interval_end,excluded.interval_end)"
            .to_string(),
        "sample_count=client_metric_hourly_aggregates.sample_count+excluded.sample_count"
            .to_string(),
    ];
    for metric in METRIC_NAMES {
        let current_count = format!("client_metric_hourly_aggregates.{metric}_count");
        let incoming_count = format!("excluded.{metric}_count");
        updates.extend([
            format!(
                "{metric}_min=CASE WHEN {current_count}=0 THEN excluded.{metric}_min WHEN {incoming_count}=0 THEN client_metric_hourly_aggregates.{metric}_min ELSE MIN(client_metric_hourly_aggregates.{metric}_min,excluded.{metric}_min) END"
            ),
            format!(
                "{metric}_max=CASE WHEN {current_count}=0 THEN excluded.{metric}_max WHEN {incoming_count}=0 THEN client_metric_hourly_aggregates.{metric}_max ELSE MAX(client_metric_hourly_aggregates.{metric}_max,excluded.{metric}_max) END"
            ),
            format!(
                "{metric}_avg=CASE WHEN {current_count}=0 THEN excluded.{metric}_avg WHEN {incoming_count}=0 THEN client_metric_hourly_aggregates.{metric}_avg ELSE client_metric_hourly_aggregates.{metric}_avg+(excluded.{metric}_avg-client_metric_hourly_aggregates.{metric}_avg)*(CAST({incoming_count} AS REAL)/({current_count}+{incoming_count})) END"
            ),
            format!("{metric}_count={current_count}+{incoming_count}"),
        ]);
    }
    updates.push("updated_at=excluded.updated_at".to_string());
    format!(
        "INSERT INTO client_metric_hourly_aggregates({}) VALUES ({}) ON CONFLICT(host_id,bucket_start) DO UPDATE SET {}",
        columns.join(","),
        vec!["?"; columns.len()].join(","),
        updates.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_enforces_retention_order_and_hard_bounds() {
        assert!(
            RetentionConfig::new(
                Duration::from_secs(2 * 24 * 60 * 60),
                Duration::from_secs(24 * 60 * 60),
                Duration::from_secs(60),
                1,
                3,
                Duration::from_secs(1),
                Duration::from_millis(1),
            )
            .is_err()
        );
        assert!(
            RetentionConfig::new(
                MIN_RAW_RETENTION,
                Duration::from_secs(2 * 24 * 60 * 60),
                Duration::from_secs(60),
                MAX_RETENTION_BATCH_SIZE + 1,
                3,
                Duration::from_secs(1),
                Duration::from_millis(1),
            )
            .is_err()
        );
        assert!(
            RetentionConfig::new(
                MIN_RAW_RETENTION,
                Duration::from_secs(2 * 24 * 60 * 60),
                Duration::from_secs(60),
                1,
                MAX_RETENTION_TRANSACTIONS + 1,
                Duration::from_secs(1),
                Duration::from_millis(1),
            )
            .is_err()
        );
    }
}
