use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, ensure};
use sqlx::{Connection, SqliteConnection, SqlitePool, sqlite::SqliteConnectOptions};
use xcss::schema_identity::SchemaIdentity;
#[cfg(test)]
use xcss::schema_identity::SchemaRow;
use xcss::sqlite::{PRODUCT_METADATA_DDL, PoolOptions};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

pub const APPLICATION: &str = "xsos";
pub const APPLICATION_VERSION: &str = env!("CARGO_PKG_VERSION");
// Persisted schema identity changes only with a data-format migration.
pub const SCHEMA_APPLICATION_VERSION: &str = "1.0.0";
pub const SCHEMA_REVISION: i64 = 1;
pub const SCHEMA_SHA256: &str = "3dcffe26f698fbacbc386a1e35dbc9d4f38f516e56115549709703d09987a40d";

const CURRENT_SCHEMA_SQL: &str = include_str!("../../../schema/generated/current_schema.sql");

pub async fn open_or_initialize(database_url: &str) -> anyhow::Result<SqlitePool> {
    let path = database_path(database_url)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    match options.open(&path) {
        Ok(file) => initialize_created(&path, file).await,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            open_existing(database_url).await
        }
        Err(error) => Err(error).context("create current xsos database"),
    }
}

pub async fn open_existing(database_url: &str) -> anyhow::Result<SqlitePool> {
    let path = database_path(database_url)?;
    let validation_path = path.clone();
    tokio::task::spawn_blocking(move || validate_read_only(&validation_path))
        .await
        .context("join read-only database schema validation")??;
    let pool = open_pool(&path).await?;
    if let Err(error) = validate_pool(&pool).await {
        pool.close().await;
        return Err(error);
    }
    Ok(pool)
}

/// Open a lock-anchored location after the caller inspected the source generation.
pub async fn open_validated_location(database_url: &str) -> anyhow::Result<SqlitePool> {
    let path = database_path(database_url)?;
    let pool = open_pool(&path).await?;
    validate_pool(&pool).await?;
    Ok(pool)
}

async fn initialize_created(path: &Path, file: File) -> anyhow::Result<SqlitePool> {
    if let Err(error) = file
        .sync_all()
        .context("synchronize new xsos database file")
    {
        drop(file);
        return fail_initialization(path, error);
    }
    if let Err(error) = sync_parent(path) {
        drop(file);
        return fail_initialization(path, error);
    }
    drop(file);
    let pool = match open_pool(path).await {
        Ok(pool) => pool,
        Err(error) => return fail_initialization(path, error),
    };
    if let Err(error) = initialize_empty(&pool).await {
        pool.close().await;
        return fail_initialization(path, error);
    }
    if let Err(error) = checkpoint_and_sync(&pool, path).await {
        pool.close().await;
        return fail_initialization(path, error);
    }
    Ok(pool)
}

fn fail_initialization<T>(path: &Path, error: anyhow::Error) -> anyhow::Result<T> {
    if let Err(cleanup_error) = cleanup_failed_initialization(path) {
        return Err(cleanup_error.context(format!(
            "current schema initialization failed and cleanup was incomplete; original error: {error:#}"
        )));
    }
    Err(error.context("initialize current xsos schema"))
}

async fn checkpoint_and_sync(pool: &SqlitePool, path: &Path) -> anyhow::Result<()> {
    xcss::sqlite::checkpoint(pool)
        .await
        .context("checkpoint initialized xsos schema")?;
    sync_file_and_parent(path)
}

async fn open_pool(database_path: &Path) -> anyhow::Result<SqlitePool> {
    let options = PoolOptions::new(16)
        .with_min_connections(1)
        .with_acquire_timeout(Duration::from_secs(5))
        .with_connection_limits(connection_limits());
    xcss::sqlite::open_existing(database_path, options)
        .await
        .context("open xsos database with the xcss SQLite baseline")
}

/// Initializes one completely empty SQLite database with the single current
/// schema. Any existing product object causes the transaction to fail before
/// DDL is executed.
pub async fn initialize_empty(pool: &SqlitePool) -> anyhow::Result<()> {
    let mut transaction = pool.begin().await?;
    let existing: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*'")
            .fetch_one(&mut *transaction)
            .await?;
    ensure!(
        existing == 0,
        "database is not empty; initialization requires an empty database"
    );
    sqlx::raw_sql(CURRENT_SCHEMA_SQL)
        .execute(&mut *transaction)
        .await?;
    let created_at_micros = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_micros(),
    )
    .context("platform creation timestamp exceeds u64")?;
    xcss::platform_db::initialize_current_platform_metadata(
        &mut transaction,
        "server-control-plane",
        created_at_micros,
    )
    .await
    .context("initialize current xcss platform metadata")?;
    let actual = xcss::sqlite::schema_fingerprint(&mut *transaction).await?;
    ensure!(
        actual == SCHEMA_SHA256,
        "compiled current schema fingerprint mismatch: expected {SCHEMA_SHA256}, computed {actual}"
    );
    sqlx::query(
        "INSERT INTO product_metadata(\
           singleton,application,application_version,schema_revision,schema_sha256\
         ) VALUES(1,?,?,?,?)",
    )
    .bind(APPLICATION)
    .bind(SCHEMA_APPLICATION_VERSION)
    .bind(SCHEMA_REVISION)
    .bind(SCHEMA_SHA256)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    validate_pool(pool).await
}

pub async fn validate_pool(pool: &SqlitePool) -> anyhow::Result<()> {
    let metadata_sql: Option<String> = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE type='table' AND name='product_metadata'",
    )
    .fetch_optional(pool)
    .await?;
    ensure!(
        metadata_sql.as_deref() == Some(PRODUCT_METADATA_DDL),
        "database product_metadata schema is not the exact current contract: actual={metadata_sql:?} expected={PRODUCT_METADATA_DDL:?}"
    );
    xcss::sqlite::require_pool_current_schema(pool, &expected_identity()?)
        .await
        .context("database is not the exact current xsos schema")?;
    xcss::platform_db::require_current_platform_metadata(pool, "server-control-plane")
        .await
        .context("database platform metadata is not the exact current contract")?;
    Ok(())
}

pub async fn is_current(pool: &SqlitePool) -> bool {
    validate_pool(pool).await.is_ok()
}

pub async fn actual_schema_sha256(pool: &SqlitePool) -> anyhow::Result<String> {
    xcss::sqlite::schema_fingerprint(pool)
        .await
        .context("fingerprint xsos schema")
}

pub fn validate_configuration_database(database_url: &str) -> anyhow::Result<()> {
    validate_read_only(&database_path(database_url)?)
}

/// Persistent admission and source-preserving validation share one physical byte boundary.
pub const DATABASE_BYTE_BUDGET: u64 = 8 * 1024 * 1024 * 1024;

pub fn snapshot_limits() -> xcss::sqlite::SnapshotLimits {
    xcss::sqlite::SnapshotLimits {
        max_total_bytes: DATABASE_BYTE_BUDGET,
        ..Default::default()
    }
}

pub fn connection_limits() -> xcss::sqlite::ConnectionLimits {
    xcss::sqlite::ConnectionLimits::new(2 * 1024 * 1024)
}

/// Capture before this process acquires any original SQLite connection.
pub async fn validation_snapshot(
    path: PathBuf,
) -> anyhow::Result<xcss::sqlite::ValidationSnapshotPool> {
    let snapshot = tokio::task::spawn_blocking(move || {
        xcss::sqlite::ValidationSnapshot::capture_with_limits(path, snapshot_limits())
    })
    .await
    .context("join private database validation capture")??;
    Ok(snapshot
        .into_pool_with_connection_limits(connection_limits())
        .await?)
}

fn validate_read_only(path: &Path) -> anyhow::Result<()> {
    let snapshot = xcss::sqlite::ValidationSnapshot::capture_with_limits(path, snapshot_limits())?;
    xcss::sqlite::block_on_sqlite_connection(async {
        let mut connection = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(snapshot.database_path())
                .create_if_missing(false)
                .busy_timeout(Duration::from_secs(2)),
        )
        .await
        .context("open private xsos validation snapshot")?;
        let result = async {
            xcss::sqlite::apply_connection_limits(&mut connection, connection_limits()).await?;
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            connection
                .lock_handle()
                .await?
                .set_progress_handler(1000, move || std::time::Instant::now() < deadline);
            sqlx::raw_sql("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
                .execute(&mut connection)
                .await?;
            let metadata_sql: Option<String> = sqlx::query_scalar(
                "SELECT sql FROM sqlite_schema WHERE type='table' AND name='product_metadata'",
            )
            .fetch_optional(&mut connection)
            .await?;
            ensure!(
                metadata_sql.as_deref() == Some(PRODUCT_METADATA_DDL),
                "database product_metadata schema is not the exact current contract"
            );
            xcss::sqlite::require_current_schema(&mut connection, &expected_identity()?)
                .await
                .context("database is not the exact current xsos schema")?;
            let platform_metadata: (i64, i64, String, i64) = sqlx::query_as(
                "SELECT platform_generation,platform_schema_revision,profile,created_at_micros \
                 FROM _xcss_platform_metadata WHERE singleton=1",
            )
            .fetch_one(&mut connection)
            .await
            .context("read current xcss platform metadata")?;
            ensure!(
                platform_metadata.0 == i64::from(xcss::platform_db::PLATFORM_GENERATION)
                    && platform_metadata.1
                        == i64::from(xcss::platform_db::PLATFORM_SCHEMA_REVISION)
                    && platform_metadata.2 == "server-control-plane"
                    && platform_metadata.3 >= 0,
                "database platform metadata is not the exact current contract"
            );
            Ok::<_, anyhow::Error>(())
        }
        .await;
        let closed = connection.close().await;
        result?;
        closed.context("close private xsos validation connection")
    })
}

pub fn expected_identity() -> anyhow::Result<SchemaIdentity> {
    SchemaIdentity::new(
        APPLICATION,
        SCHEMA_APPLICATION_VERSION,
        u64::try_from(SCHEMA_REVISION).context("schema revision must not be negative")?,
        SCHEMA_SHA256,
    )
    .context("compiled xsos schema identity is invalid")
}

pub fn database_path(database_url: &str) -> anyhow::Result<PathBuf> {
    let value = database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))
        .context("database URL must use the sqlite scheme")?;
    ensure!(!value.is_empty(), "SQLite database path must not be empty");
    ensure!(
        value != ":memory:",
        "in-memory database files are unsupported"
    );
    ensure!(
        !value.contains('?')
            && !value.contains('#')
            && !value.contains('%')
            && !value.contains('\0'),
        "database requires a plain, unescaped SQLite file URL without query or fragment"
    );
    Ok(PathBuf::from(value))
}

fn cleanup_failed_initialization(path: &Path) -> anyhow::Result<()> {
    for suffix in ["-wal", "-shm", "-journal", ""] {
        let mut value = path.as_os_str().to_os_string();
        value.push(suffix);
        match fs::remove_file(PathBuf::from(value)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("remove failed schema initialization file"),
        }
    }
    sync_parent(path)
}

fn sync_file_and_parent(path: &Path) -> anyhow::Result<()> {
    File::open(path)?.sync_all()?;
    sync_parent(path)
}

fn sync_parent(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    File::open(path.parent().unwrap_or_else(|| Path::new(".")))?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_uses_the_foundation_binary_framing() {
        let rows = vec![
            SchemaRow::new(
                "table".to_string(),
                "a".to_string(),
                "a".to_string(),
                "CREATE TABLE a(x)".to_string(),
            ),
            SchemaRow::new(
                "trigger".to_string(),
                "触发".to_string(),
                "a".to_string(),
                String::new(),
            ),
        ];
        assert_eq!(
            xcss::schema_identity::schema_fingerprint(&rows).unwrap(),
            "c51a04c9248c03f8637dadfa8aafad30bd3f233b474f464f807892071c010049"
        );
    }

    #[test]
    fn release_manifest_matches_the_compiled_product_schema() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../release.json")).unwrap();
        assert_eq!(manifest["application"], APPLICATION);
        assert_eq!(manifest["version"], APPLICATION_VERSION);
        assert_eq!(manifest["schema_revision"], SCHEMA_REVISION);
        assert_eq!(manifest["schema_sha256"], SCHEMA_SHA256);
    }
}
