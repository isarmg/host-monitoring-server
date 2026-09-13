#![cfg(target_os = "linux")]

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use chrono::Utc;
use host_monitoring_server::{
    database_lock::{ApplicationLock, MaintenanceLock},
    store,
};
use uuid::Uuid;

const BOOTSTRAP_PASSWORD: &str = "lock-test-bootstrap-password";

fn database_url(path: &Path) -> String {
    format!("sqlite://{}", path.display())
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

fn server_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_host-monitoring-server"));
    command.env_remove("RUST_LOG");
    command
}

#[tokio::test]
async fn lock_identity_survives_working_directory_and_sqlite_restarts() {
    let directory = tempfile::tempdir().expect("create database directory");
    let database = directory.path().join("app.sqlite3");
    let url = database_url(&database);
    let static_dir = directory.path().join("static");
    std::fs::create_dir(&static_dir).unwrap();
    std::fs::create_dir(static_dir.join("assets")).unwrap();
    std::fs::write(static_dir.join("index.html"), "current").unwrap();
    std::fs::write(static_dir.join("assets/app.js"), "current").unwrap();

    // The application uses an absolute path in this process. Child commands
    // below use `sqlite:app.sqlite3` from a different working directory.
    let application = ApplicationLock::acquire(&url).expect("acquire application lock");
    assert!(ApplicationLock::acquire(&url).is_err());
    let online = MaintenanceLock::shared(&url).expect("online maintenance shares the lock");
    assert!(
        MaintenanceLock::exclusive(&url).is_err(),
        "offline maintenance entered while the application was live"
    );

    let pool = store::open_or_initialize(&application.database_url())
        .await
        .expect("open SQLite through the trusted directory descriptor");
    let host_id = Uuid::new_v4();
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO monitored_hosts(\
           host_id,name,os,arch,client_version,registered_at,last_seen_at\
         ) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(host_id)
    .bind("locked host")
    .bind("linux")
    .bind("x86_64")
    .bind(env!("CARGO_PKG_VERSION"))
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .expect("persist through trusted descriptor URL");
    assert!(database.is_file());
    assert!(
        sidecar(&database, "-wal").is_file(),
        "WAL was not created beside the configured database"
    );
    assert!(
        sidecar(&database, "-shm").is_file(),
        "SHM was not created beside the configured database"
    );

    let doctor = server_command()
        .arg("doctor")
        .current_dir(directory.path())
        .env("HOST_MONITORING_DATABASE_URL", "sqlite:app.sqlite3")
        .env("HOST_MONITORING_STATIC_DIR", &static_dir)
        .env("HOST_MONITORING_DEVELOPMENT", "true")
        .env(
            "HOST_MONITORING_CLIENT_AUTHORIZATION_KEY",
            "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=",
        )
        .output()
        .expect("run doctor through a relative database path");
    assert!(
        doctor.status.success(),
        "online doctor failed: {}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    let doctor: serde_json::Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(doctor["status"], "ok");
    assert_eq!(doctor["database_ready"], true);
    assert_eq!(doctor["retention_ready"], true);
    assert_eq!(doctor["integrity_ready"], true);
    assert_eq!(doctor["foreign_keys_ready"], true);

    let second_server = server_command()
        .arg("serve")
        .current_dir(directory.path())
        .env("HOST_MONITORING_DATABASE_URL", "sqlite:app.sqlite3")
        .env("HOST_MONITORING_BIND", "127.0.0.1:0")
        .env("HOST_MONITORING_STATIC_DIR", &static_dir)
        .env(
            "HOST_MONITORING_CLIENT_AUTHORIZATION_KEY",
            "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=",
        )
        .env("HOST_MONITORING_DEVELOPMENT", "true")
        .env(
            "HOST_MONITORING_BOOTSTRAP_ADMIN_PASSWORD",
            BOOTSTRAP_PASSWORD,
        )
        .output()
        .expect("run second server through a relative database path");
    assert!(!second_server.status.success());
    let stderr = String::from_utf8_lossy(&second_server.stderr);
    assert!(
        stderr.contains("another Host Monitoring process")
            || stderr.contains("database lock is already held"),
        "unexpected second-instance error: {stderr}"
    );
    assert!(!stderr.contains(BOOTSTRAP_PASSWORD));
    assert!(!stderr.contains(&url));

    pool.close().await;
    drop(online);
    drop(application);

    let offline = MaintenanceLock::exclusive(&url).expect("lock released after shutdown");
    assert!(ApplicationLock::acquire(&url).is_err());
    drop(offline);

    let restarted = ApplicationLock::acquire(&url).expect("reacquire lock after restart");
    let reopened = store::open_existing(&restarted.database_url())
        .await
        .expect("reopen SQLite through a new trusted directory descriptor");
    let stored_name: String =
        sqlx::query_scalar("SELECT name FROM monitored_hosts WHERE host_id=?")
            .bind(host_id)
            .fetch_one(&reopened)
            .await
            .expect("read persisted data after restart");
    assert_eq!(stored_name, "locked host");
    reopened.close().await;
}
