//! Product lists use bounded keyset pages; navigation never changes authorization.
use anyhow::{Context, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, SqlitePool};
use std::time::Duration;
use uuid::Uuid;

pub const PAGE_SIZE: usize = 50;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    version: u8,
    scope: String,
    pub newer: bool,
    pub sort: String,
    pub id: Uuid,
}

impl Cursor {
    pub fn scope(value: impl Serialize) -> String {
        hex::encode(Sha256::digest(
            serde_json::to_vec(&value).expect("primitive scope"),
        ))
    }
    pub fn decode(value: Option<&str>, scope: &str) -> anyhow::Result<Option<Self>> {
        let Some(value) = value else { return Ok(None) };
        ensure!(
            !value.is_empty() && value.len() <= 512 && value.is_ascii(),
            "invalid list cursor"
        );
        let cursor: Self = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(value)?)
            .context("invalid list cursor")?;
        ensure!(
            cursor.version == 1
                && cursor.scope == scope
                && cursor.sort.len() <= 128
                && !cursor.sort.chars().any(char::is_control),
            "invalid list cursor"
        );
        Ok(Some(cursor))
    }
    fn encode(sort: String, id: Uuid, newer: bool, scope: &str) -> anyhow::Result<String> {
        ensure!(
            sort.len() <= 128 && !sort.chars().any(char::is_control),
            "invalid stored pagination key"
        );
        let value = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&Self {
            version: 1,
            scope: scope.to_owned(),
            newer,
            sort,
            id,
        })?);
        ensure!(value.len() <= 512, "invalid stored pagination key");
        Ok(value)
    }
}

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub rows: Vec<T>,
    pub next_cursor: Option<String>,
    pub previous_cursor: Option<String>,
}

pub fn page<T: Serialize>(
    mut rows: Vec<T>,
    cursor: Option<&Cursor>,
    scope: &str,
    key: impl Fn(&T) -> (String, Uuid),
) -> anyhow::Result<Page<T>> {
    let newer = cursor.is_some_and(|value| value.newer);
    // Capability-rich hosts still remain traversable: a large valid row reduces
    // the page count instead of causing a permanent whole-page HTTP rejection.
    let mut size = 0usize;
    let mut count = 0;
    for row in rows.iter().take(PAGE_SIZE) {
        let bytes = serde_json::to_vec(row)?.len();
        if size.saturating_add(bytes) > 7 * 1024 * 1024 {
            break;
        }
        size += bytes;
        count += 1;
    }
    ensure!(
        rows.is_empty() || count > 0,
        "stored list row exceeds response budget"
    );
    let more = rows.len() > count;
    rows.truncate(count);
    if newer {
        rows.reverse();
    }
    let previous_cursor = if (newer && more) || (!newer && cursor.is_some()) {
        rows.first()
            .map(|row| {
                let (sort, id) = key(row);
                Cursor::encode(sort, id, true, scope)
            })
            .transpose()?
    } else {
        None
    };
    let next_cursor = if (!newer && more) || newer {
        rows.last()
            .map(|row| {
                let (sort, id) = key(row);
                Cursor::encode(sort, id, false, scope)
            })
            .transpose()?
    } else {
        None
    };
    Ok(Page {
        rows,
        next_cursor,
        previous_cursor,
    })
}

/// The leased worker is closed before admission is released, even after SQL errors.
/// All statements in a page share the same native SQLite deadline.
pub async fn query<T, F>(pool: &SqlitePool, work: F) -> anyhow::Result<T>
where
    T: Send,
    F: for<'a> FnOnce(
            &'a mut SqliteConnection,
        ) -> futures_util::future::BoxFuture<'a, anyhow::Result<T>>
        + Send,
{
    let mut connection = tokio::time::timeout(Duration::from_secs(2), pool.acquire())
        .await
        .context("list connection budget exhausted")??;
    let memory_fixture = pool
        .connect_options()
        .get_filename()
        .to_string_lossy()
        .starts_with("file:sqlx-in-memory-");
    if !memory_fixture {
        connection.close_on_drop();
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    connection
        .lock_handle()
        .await?
        .set_progress_handler(1000, move || std::time::Instant::now() < deadline);
    let result = work(&mut connection).await;
    if memory_fixture {
        // Private in-memory tests would lose their whole database on closing
        // the last connection. Every awaited statement has completed here.
        connection.lock_handle().await?.remove_progress_handler();
    } else {
        connection.close().await?;
    }
    result
}

#[derive(Clone)]
pub struct Admission {
    global: std::sync::Arc<tokio::sync::Semaphore>,
    owners: std::sync::Arc<
        std::sync::Mutex<
            std::collections::HashMap<String, std::sync::Weak<tokio::sync::Semaphore>>,
        >,
    >,
}
struct Permit {
    _global: tokio::sync::OwnedSemaphorePermit,
    _owner: tokio::sync::OwnedSemaphorePermit,
}
impl Default for Admission {
    fn default() -> Self {
        Self {
            global: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
            owners: Default::default(),
        }
    }
}
impl Admission {
    fn acquire(&self, owner: &str) -> crate::error::Result<Permit> {
        let busy = || crate::error::Error::RateLimited {
            message: "list resource is busy",
            retry_after: 1,
        };
        let global = self
            .global
            .clone()
            .try_acquire_owned()
            .map_err(|_| busy())?;
        let semaphore = {
            let mut owners = self
                .owners
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            owners.retain(|_, weak| weak.strong_count() > 0);
            if let Some(value) = owners.get(owner).and_then(std::sync::Weak::upgrade) {
                value
            } else {
                let value = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
                owners.insert(owner.to_owned(), std::sync::Arc::downgrade(&value));
                value
            }
        };
        let owner = semaphore.try_acquire_owned().map_err(|_| busy())?;
        Ok(Permit {
            _global: global,
            _owner: owner,
        })
    }
}

pub async fn response<T, F>(
    admission: &Admission,
    owner: &str,
    work: F,
) -> crate::error::Result<axum::response::Response>
where
    T: Serialize + Send + 'static,
    F: std::future::Future<Output = crate::error::Result<T>> + Send + 'static,
{
    use axum::response::IntoResponse;
    let permit = admission.acquire(owner)?;
    // Detaching the admitted task on caller cancellation keeps its actual SQL
    // worker and resource reservation paired until close has completed.
    let task = tokio::spawn(async move {
        let value = work.await?;
        let mut output = LimitedOutput(Vec::new());
        serde_json::to_writer(&mut output, &value)
            .map_err(|error| crate::error::Error::Internal(error.into()))?;
        Ok::<_, crate::error::Error>((output.0, permit))
    });
    let (bytes, permit) = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .map_err(|_| crate::error::Error::RetryableUnavailable {
            message: "list deadline exceeded",
            retry_after: 1,
        })?
        .map_err(|error| crate::error::Error::Internal(error.into()))??;
    let stream =
        futures_util::stream::unfold((bytes, 0, permit), |(bytes, offset, permit)| async move {
            if offset == bytes.len() {
                return None;
            };
            let end = (offset + 16 * 1024).min(bytes.len());
            let chunk = axum::body::Bytes::copy_from_slice(&bytes[offset..end]);
            Some((
                Ok::<_, std::convert::Infallible>(chunk),
                (bytes, end, permit),
            ))
        });
    let mut response = axum::body::Body::from_stream(stream).into_response();
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}
struct LimitedOutput(Vec<u8>);
impl std::io::Write for LimitedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > 8 * 1024 * 1024 {
            return Err(std::io::Error::other("list response budget exceeded"));
        };
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn admission_survives_caller_cancellation_and_separates_resources() {
        let admission = Admission::default();
        let started = std::sync::Arc::new(tokio::sync::Notify::new());
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let child_admission = admission.clone();
        let child_start = started.clone();
        let child_release = release.clone();
        let caller = tokio::spawn(async move {
            response(&child_admission, "reports:a", async move {
                child_start.notify_one();
                child_release.notified().await;
                Ok(1)
            })
            .await
        });
        started.notified().await;
        caller.abort();
        let _ = caller.await;
        assert!(admission.acquire("reports:a").is_err());
        let other = admission.acquire("reports:b").unwrap();
        let same_admin_other = admission.acquire("hosts:a").unwrap();
        drop(other);
        drop(same_admin_other);
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if admission.acquire("reports:a").is_ok() {
                    break;
                };
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn native_query_deadline_returns_worker_before_releasing_admission() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let started = std::time::Instant::now();
        let result=query(&pool,|conn|Box::pin(async move {
            let _:i64=sqlx::query_scalar("WITH RECURSIVE counter(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM counter WHERE x<1000000000) SELECT sum(x) FROM counter").fetch_one(conn).await?;Ok(())
        })).await;
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        let actual: i64 = sqlx::query_scalar("SELECT 42")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(actual, 42);
    }
    #[test]
    fn oversized_row_reduces_page_and_cross_scope_cursor_is_rejected() {
        let rows: Vec<String> = (0..51).map(|_| "x".repeat(256 * 1024)).collect();
        let page = page(rows, None, "a", |_| ("key".into(), Uuid::nil())).unwrap();
        assert!(page.rows.len() < 50);
        assert!(page.next_cursor.is_some());
        assert!(Cursor::decode(page.next_cursor.as_deref(), "b").is_err());
        assert!(Cursor::decode(Some(&"x".repeat(513)), "a").is_err());
    }
}
