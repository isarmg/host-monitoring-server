//! Persistent admission precedes new writes under the SQLite writer transaction.
//! Existing receipts, active instances, and administration remain usable at capacity.
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub instance_rows: i64,
    pub host_reports: i64,
    pub total_reports: i64,
    pub host_bytes: u64,
    pub database_bytes: u64,
    pub free_floor: u64,
}
impl Limits {
    pub const PRODUCTION: Self = Self {
        instance_rows: 4096,
        host_reports: 100_000,
        total_reports: 1_000_000,
        host_bytes: 1024 * 1024 * 1024,
        database_bytes: crate::database_schema::DATABASE_BYTE_BUDGET,
        free_floor: 1024 * 1024 * 1024,
    };
}
#[derive(Debug, thiserror::Error)]
#[error("persistent monitoring capacity is exhausted")]
pub(crate) struct Exhausted;
const ADMIN_RESERVE: u64 = 64 * 1024 * 1024;

pub(crate) async fn instance(
    tx: &mut Transaction<'_, Sqlite>,
    limits: Limits,
) -> anyhow::Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM client_instance_invites")
        .fetch_one(&mut **tx)
        .await?;
    if count >= limits.instance_rows {
        return Err(Exhausted.into());
    };
    storage(tx, 32 * 1024, limits).await
}

pub(crate) async fn pairing(
    tx: &mut Transaction<'_, Sqlite>,
    limits: Limits,
) -> anyhow::Result<()> {
    storage(tx, 32 * 1024, limits).await
}

pub(crate) async fn report(
    tx: &mut Transaction<'_, Sqlite>,
    host: Uuid,
    report: Uuid,
    payload_bytes: u64,
    limits: Limits,
) -> anyhow::Result<()> {
    let existing: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM client_metric_reports WHERE report_id=?)")
            .bind(report)
            .fetch_one(&mut **tx)
            .await?;
    // The authoritative existing owner check still runs; a retry is not a new reservation.
    if existing {
        return Ok(());
    };
    let host_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM client_metric_reports WHERE host_id=?")
            .bind(host)
            .fetch_one(&mut **tx)
            .await?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM client_metric_reports")
        .fetch_one(&mut **tx)
        .await?;
    let retained_payload:i64=sqlx::query_scalar("SELECT COALESCE(SUM(COALESCE(length(CAST(payload AS BLOB)),0)),0) FROM client_metric_reports WHERE host_id=?").bind(host).fetch_one(&mut **tx).await?;
    // Raw scalar rows and the retained payloads are charged for main plus WAL.
    let charge = (host_rows.max(0) as u64)
        .saturating_add(1)
        .saturating_mul(4096)
        .saturating_add(
            (retained_payload.max(0) as u64)
                .saturating_add(payload_bytes)
                .saturating_mul(2),
        );
    if host_rows >= limits.host_reports
        || total >= limits.total_reports
        || charge > limits.host_bytes
    {
        return Err(Exhausted.into());
    };
    storage(
        tx,
        payload_bytes.saturating_add(4096).saturating_mul(2),
        limits,
    )
    .await
}

async fn storage(
    tx: &mut Transaction<'_, Sqlite>,
    new_bytes: u64,
    limits: Limits,
) -> anyhow::Result<()> {
    let pages: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(&mut **tx)
        .await?;
    let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
        .fetch_one(&mut **tx)
        .await?;
    let allocated = (pages.max(0) as u64).saturating_mul(page_size.max(0) as u64);
    let path: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name='main'")
            .fetch_one(&mut **tx)
            .await?;
    let (wal, available) = if path.is_empty() {
        (0, u64::MAX)
    } else {
        tokio::task::spawn_blocking(move || -> anyhow::Result<(u64, u64)> {
            let journal = std::path::PathBuf::from(format!("{path}-wal"));
            let wal = match std::fs::symlink_metadata(journal) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    metadata.len()
                }
                Ok(_) => anyhow::bail!("invalid database journal identity"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                Err(error) => return Err(error.into()),
            };
            let parent = std::path::Path::new(&path)
                .parent()
                .ok_or_else(|| anyhow::anyhow!("database has no parent"))?;
            let stats = rustix::fs::statvfs(parent)?;
            Ok((wal, stats.f_bavail.saturating_mul(stats.f_frsize)))
        })
        .await??
    };
    if allocated
        .saturating_add(wal)
        .saturating_add(new_bytes)
        .saturating_add(ADMIN_RESERVE)
        > limits.database_bytes
        || available
            < limits
                .free_floor
                .saturating_add(new_bytes)
                .saturating_add(ADMIN_RESERVE)
    {
        return Err(Exhausted.into());
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn capacity_preserves_existing_receipts_and_other_host_budget() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::store::initialize_empty(&pool).await.unwrap();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let existing = Uuid::new_v4();
        for id in [a, b] {
            sqlx::query("INSERT INTO monitored_hosts(host_id,name,os,arch,client_version,registered_at,last_seen_at) VALUES(?,'test','linux','x','1',?,?)").bind(id).bind(chrono::Utc::now()).bind(chrono::Utc::now()).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO client_metric_reports(report_id,host_id,schema_version,collected_at,received_at,interval_seconds) VALUES(?,?,3,?,?,10)").bind(existing).bind(a).bind(chrono::Utc::now()).bind(chrono::Utc::now()).execute(&pool).await.unwrap();
        let limits = Limits {
            host_reports: 1,
            total_reports: 2,
            free_floor: 0,
            ..Limits::PRODUCTION
        };
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        assert!(
            report(&mut tx, a, Uuid::new_v4(), 0, limits)
                .await
                .unwrap_err()
                .is::<Exhausted>()
        );
        report(&mut tx, a, existing, 0, limits).await.unwrap();
        report(&mut tx, b, Uuid::new_v4(), 0, limits).await.unwrap();
        assert!(
            report(
                &mut tx,
                b,
                Uuid::new_v4(),
                0,
                Limits {
                    database_bytes: 1,
                    ..limits
                }
            )
            .await
            .unwrap_err()
            .is::<Exhausted>()
        );
        assert!(
            report(
                &mut tx,
                b,
                Uuid::new_v4(),
                0,
                Limits {
                    host_bytes: 1,
                    ..limits
                }
            )
            .await
            .unwrap_err()
            .is::<Exhausted>()
        );
        tx.rollback().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM client_metric_reports")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
    #[tokio::test]
    async fn instance_capacity_does_not_create_an_unusable_invitation() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::store::initialize_empty(&pool).await.unwrap();
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        assert!(
            instance(
                &mut tx,
                Limits {
                    instance_rows: 0,
                    ..Limits::PRODUCTION
                }
            )
            .await
            .unwrap_err()
            .is::<Exhausted>()
        );
        tx.rollback().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM client_instance_invites")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
