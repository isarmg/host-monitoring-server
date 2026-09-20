use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use sarmg_schema_identity::{
    ProductMetadataRow, SQLITE_SCHEMA_ROWS_QUERY, SchemaIdentity, SchemaRow, verify_current_schema,
};
use sarmg_sqlite::{PRODUCT_METADATA_DDL, PoolOptions};
use sqlx::SqlitePool;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

pub const APPLICATION: &str = "host-monitoring";
pub const APPLICATION_VERSION: &str = env!("CARGO_PKG_VERSION");
// Persisted schema identity changes only with a data-format migration.
pub const SCHEMA_APPLICATION_VERSION: &str = "0.9.18";
pub const SCHEMA_REVISION: i64 = 6;
pub const SCHEMA_SHA256: &str = "dc97f6526439673f7a633a15a2557e758e922bc49749f8b9562ee8ee3ed7048d";

const CURRENT_SCHEMA_SQL: &str = include_str!("../../schema/generated/current_schema.sql");

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
        Err(error) => Err(error).context("create current Host Monitoring database"),
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

async fn initialize_created(path: &Path, file: File) -> anyhow::Result<SqlitePool> {
    if let Err(error) = file
        .sync_all()
        .context("synchronize new Host Monitoring database file")
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
    Err(error.context("initialize current Host Monitoring schema"))
}

async fn checkpoint_and_sync(pool: &SqlitePool, path: &Path) -> anyhow::Result<()> {
    sarmg_sqlite::checkpoint(pool)
        .await
        .context("checkpoint initialized Host Monitoring schema")?;
    sync_file_and_parent(path)
}

async fn open_pool(database_path: &Path) -> anyhow::Result<SqlitePool> {
    let options = PoolOptions::new(16)
        .with_min_connections(1)
        .with_acquire_timeout(Duration::from_secs(5));
    sarmg_sqlite::open_existing(database_path, options)
        .await
        .context("open Host Monitoring database with the Foundation SQLite baseline")
}

/// Initializes one completely empty SQLite database with the single current
/// schema. This is deliberately not an upgrade path: any existing product
/// object causes the transaction to fail before DDL is executed.
pub async fn initialize_empty(pool: &SqlitePool) -> anyhow::Result<()> {
    let mut transaction = pool.begin().await?;
    let existing: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*'")
            .fetch_one(&mut *transaction)
            .await?;
    ensure!(
        existing == 0,
        "database is not empty; product schema upgrades require the external upgrade tool"
    );
    sqlx::raw_sql(CURRENT_SCHEMA_SQL)
        .execute(&mut *transaction)
        .await?;
    let actual = sarmg_sqlite::schema_fingerprint(&mut *transaction).await?;
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
    sarmg_sqlite::require_pool_current_schema(pool, &expected_identity()?)
        .await
        .context("database is not the exact current Host Monitoring schema")?;
    Ok(())
}

pub async fn is_current(pool: &SqlitePool) -> bool {
    validate_pool(pool).await.is_ok()
}

pub async fn actual_schema_sha256(pool: &SqlitePool) -> anyhow::Result<String> {
    sarmg_sqlite::schema_fingerprint(pool)
        .await
        .context("fingerprint Host Monitoring schema")
}

fn validate_read_only(path: &Path) -> anyhow::Result<()> {
    ensure!(
        path.is_file(),
        "Host Monitoring database file does not exist"
    );
    let path = path
        .to_str()
        .context("SQLite database path must be valid UTF-8")?;
    // Immutable mode is essential here: even a normal SQLite read-only
    // connection can create, rewrite, or remove WAL shared-memory sidecars.
    // The product never changes schema after initialization, so the durable
    // main image is authoritative for this preflight. A second validation on
    // the live pool still rejects unexpected committed WAL schema frames.
    let read_only_uri = format!("file:{path}?mode=ro&immutable=1");
    let connection = Connection::open_with_flags(
        read_only_uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .context("open Host Monitoring database read-only for schema validation")?;
    connection.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")?;
    validate_connection_contract(&connection)
}

fn validate_connection_contract(connection: &Connection) -> anyhow::Result<()> {
    let metadata_sql: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE type='table' AND name='product_metadata'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    ensure!(
        metadata_sql.as_deref() == Some(PRODUCT_METADATA_DDL),
        "database product_metadata schema is not the exact current contract: actual={metadata_sql:?} expected={PRODUCT_METADATA_DDL:?}"
    );
    let metadata = {
        let mut statement = connection.prepare(
            "SELECT singleton,application,application_version,schema_revision,schema_sha256 \
             FROM product_metadata ORDER BY singleton",
        )?;
        statement
            .query_map([], |row| {
                Ok(ProductMetadataRow {
                    singleton: row.get(0)?,
                    application: row.get(1)?,
                    application_version: row.get(2)?,
                    schema_revision: row.get(3)?,
                    schema_sha256: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut statement = connection.prepare(SQLITE_SCHEMA_ROWS_QUERY)?;
    let rows = statement
        .query_map([], |row| {
            Ok(SchemaRow::new(
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    verify_current_schema(&metadata, &rows, &expected_identity()?)
        .context("database is not the exact current Host Monitoring schema")?;
    Ok(())
}

pub fn expected_identity() -> anyhow::Result<SchemaIdentity> {
    SchemaIdentity::new(
        APPLICATION,
        SCHEMA_APPLICATION_VERSION,
        u64::try_from(SCHEMA_REVISION).context("schema revision must not be negative")?,
        SCHEMA_SHA256,
    )
    .context("compiled Host Monitoring schema identity is invalid")
}

fn database_path(database_url: &str) -> anyhow::Result<PathBuf> {
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
    fn fingerprint_uses_the_upgrade_contract_binary_framing() {
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
            sarmg_schema_identity::schema_fingerprint(&rows).unwrap(),
            "c51a04c9248c03f8637dadfa8aafad30bd3f233b474f464f807892071c010049"
        );
    }

    #[test]
    fn release_manifest_matches_the_compiled_product_schema() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../release.json")).unwrap();
        assert_eq!(manifest["application"], APPLICATION);
        assert_eq!(manifest["version"], APPLICATION_VERSION);
        assert_eq!(manifest["schema_revision"], SCHEMA_REVISION);
        assert_eq!(manifest["schema_sha256"], SCHEMA_SHA256);
    }
}
