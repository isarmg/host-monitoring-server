use std::{fs, path::Path};

use chrono::Utc;
use host_monitoring_server::{database_schema, store};
use sha2::Digest;
use sqlx::{Sqlite, pool::PoolConnection};
use uuid::Uuid;

fn database_url(path: &Path) -> String {
    format!("sqlite://{}", path.display())
}

async fn assert_connection_baseline(connection: &mut PoolConnection<Sqlite>) {
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut **connection)
        .await
        .expect("read journal_mode");
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut **connection)
        .await
        .expect("read foreign_keys");
    assert_eq!(foreign_keys, 1);

    let busy_timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut **connection)
        .await
        .expect("read busy_timeout");
    assert_eq!(busy_timeout, 5_000);

    let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
        .fetch_one(&mut **connection)
        .await
        .expect("read synchronous");
    assert_eq!(synchronous, 2);
}

async fn assert_database_checks(pool: &sqlx::SqlitePool) {
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(pool)
        .await
        .expect("run integrity_check");
    assert_eq!(integrity, "ok");

    let foreign_key_violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await
        .expect("run foreign_key_check");
    assert!(foreign_key_violations.is_empty());
}

fn directory_snapshot(directory: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let bytes = if entry.file_type().unwrap().is_file() {
                fs::read(entry.path()).unwrap()
            } else {
                Vec::new()
            };
            (name, bytes)
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn assert_snapshot_unchanged(before: &[(String, Vec<u8>)], after: &[(String, Vec<u8>)]) {
    let summary = |snapshot: &[(String, Vec<u8>)]| {
        snapshot
            .iter()
            .map(|(name, bytes)| {
                let digest = sha2::Sha256::digest(bytes);
                format!("{name}:{}:{digest:x}", bytes.len())
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(summary(after), summary(before));
}

fn mutate_checkpoint_and_seed_sidecar_sentinels(path: &Path, sql: &str) {
    let connection = rusqlite::Connection::open(path).unwrap();
    connection.execute(sql, []).unwrap();
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(connection);

    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        fs::write(sidecar, b"noncurrent-sidecar-must-remain-unchanged").unwrap();
    }
}

async fn copy_current_database_image(target: &Path) {
    let source_directory = tempfile::tempdir().unwrap();
    let source = source_directory.path().join("current.sqlite3");
    let pool = store::open_or_initialize(&database_url(&source))
        .await
        .unwrap();
    pool.close().await;
    fs::copy(source, target).unwrap();
}

#[tokio::test]
async fn exact_current_schema_survives_close_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("current.sqlite3");
    let url = database_url(&path);

    let pool = store::open_or_initialize(&url)
        .await
        .expect("initialize the one current schema");
    assert!(store::ready(&pool).await);
    assert_eq!(
        database_schema::actual_schema_sha256(&pool).await.unwrap(),
        database_schema::SCHEMA_SHA256
    );
    let metadata: (i64, String, String, i64, String) = sqlx::query_as(
        "SELECT singleton,application,application_version,schema_revision,schema_sha256 \
         FROM product_metadata",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        metadata,
        (
            1,
            database_schema::APPLICATION.to_string(),
            "0.9.15".to_string(),
            database_schema::SCHEMA_REVISION,
            database_schema::SCHEMA_SHA256.to_string(),
        )
    );

    let mut first = pool.acquire().await.expect("acquire first connection");
    let mut second = pool.acquire().await.expect("acquire second connection");
    assert_connection_baseline(&mut first).await;
    assert_connection_baseline(&mut second).await;
    drop(first);
    drop(second);

    let host_id = Uuid::new_v4();
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO monitored_hosts(\
           host_id,name,os,arch,client_version,registered_at,last_seen_at\
         ) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(host_id)
    .bind("Persistent Host")
    .bind("linux")
    .bind("x86_64")
    .bind(env!("CARGO_PKG_VERSION"))
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .expect("seed persistent host");

    let invalid_credential = sqlx::query(
        "INSERT INTO client_credentials(credential_id,host_id,token_hash,issued_at) \
         VALUES(?,?,?,?)",
    )
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind("orphaned-token-hash")
    .bind(now)
    .execute(&pool)
    .await;
    assert!(
        invalid_credential.is_err(),
        "foreign keys were not enforced"
    );
    assert_database_checks(&pool).await;
    pool.close().await;

    let reopened = store::open_existing(&url)
        .await
        .expect("reopen exact current database");
    let stored_name: String =
        sqlx::query_scalar("SELECT name FROM monitored_hosts WHERE host_id=?")
            .bind(host_id)
            .fetch_one(&reopened)
            .await
            .expect("read host after reopening");
    assert_eq!(stored_name, "Persistent Host");
    assert_database_checks(&reopened).await;
    reopened.close().await;
}

#[tokio::test]
async fn noncurrent_version_missing_metadata_and_schema_drift_are_read_only_rejections() {
    let directory = tempfile::tempdir().unwrap();

    let unknown_schema = directory.path().join("unknown-schema.sqlite3");
    rusqlite::Connection::open(&unknown_schema)
        .unwrap()
        .execute("CREATE TABLE unknown_data(id INTEGER PRIMARY KEY)", [])
        .unwrap();
    let before = directory_snapshot(directory.path());
    assert!(
        store::open_or_initialize(&database_url(&unknown_schema))
            .await
            .is_err()
    );
    assert_snapshot_unchanged(&before, &directory_snapshot(directory.path()));

    let noncurrent_version = directory.path().join("noncurrent-version.sqlite3");
    copy_current_database_image(&noncurrent_version).await;
    mutate_checkpoint_and_seed_sidecar_sentinels(
        &noncurrent_version,
        "UPDATE product_metadata SET application_version='not-current'",
    );
    let before = directory_snapshot(directory.path());
    assert!(
        store::open_or_initialize(&database_url(&noncurrent_version))
            .await
            .is_err()
    );
    assert_snapshot_unchanged(&before, &directory_snapshot(directory.path()));

    let drifted = directory.path().join("drifted.sqlite3");
    copy_current_database_image(&drifted).await;
    mutate_checkpoint_and_seed_sidecar_sentinels(&drifted, "CREATE TABLE unexpected(id INTEGER)");
    let before = directory_snapshot(directory.path());
    assert!(
        store::open_or_initialize(&database_url(&drifted))
            .await
            .is_err()
    );
    assert_snapshot_unchanged(&before, &directory_snapshot(directory.path()));
}

#[tokio::test]
async fn readiness_rejects_live_schema_drift() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ready.sqlite3");
    let pool = store::open_or_initialize(&database_url(&path))
        .await
        .unwrap();
    assert!(store::ready(&pool).await);
    assert!(store::retention_ready(&pool).await);

    sqlx::query("DROP TABLE client_metric_hourly_aggregates")
        .execute(&pool)
        .await
        .unwrap();
    assert!(!store::retention_ready(&pool).await);
    assert!(!store::ready(&pool).await);
    pool.close().await;
}
