use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use host_monitoring_server::{
    model,
    retention::{RetentionConfig, RetentionMaintenance, run_once_at},
    store::{self, ReportWrite},
    telemetry::{TelemetryWriter, TelemetryWriterConfig},
    token_hash,
};
use host_protocol::{
    Capability, ClientHealth, ClientReport, CpuSnapshot, HostIdentity, MemorySnapshot,
    SystemSnapshot,
};
use sqlx::{Sqlite, SqlitePool, pool::PoolConnection};
use tempfile::TempDir;
use tokio::task::JoinSet;
use uuid::Uuid;

struct TestDatabase {
    _directory: TempDir,
    database_url: String,
    pool: SqlitePool,
}

impl TestDatabase {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("retention.sqlite3");
        let database_url = format!("sqlite://{}", path.display());
        let pool = store::open_or_initialize(&database_url).await.unwrap();
        Self {
            _directory: directory,
            database_url,
            pool,
        }
    }

    async fn add_host(&self, name: &str) -> (Uuid, String) {
        let host_id = Uuid::new_v4();
        let token = format!("client-token-{host_id}");
        let now = Utc::now();
        sqlx::query(
            r#"INSERT INTO monitored_hosts(
                   host_id,name,os,arch,client_version,capabilities,
                   registered_at,last_seen_at,lifecycle_status
               ) VALUES(?,?,'linux','x86_64','test','[]',?,?,'active')"#,
        )
        .bind(host_id)
        .bind(name)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO client_credentials(credential_id,host_id,token_hash,issued_at) \
             VALUES(?,?,?,?)",
        )
        .bind(Uuid::new_v4())
        .bind(host_id)
        .bind(token_hash(&token))
        .bind(now)
        .execute(&self.pool)
        .await
        .unwrap();
        (host_id, token)
    }
}

fn timestamp(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}

fn retention_config(batch_size: usize, transactions: usize) -> RetentionConfig {
    RetentionConfig::new(
        Duration::from_secs(24 * 60 * 60),
        Duration::from_secs(30 * 24 * 60 * 60),
        Duration::from_secs(1),
        batch_size,
        transactions,
        Duration::from_millis(750),
        Duration::from_millis(1),
    )
    .unwrap()
}

async fn insert_raw(
    pool: &SqlitePool,
    host_id: Uuid,
    report_id: Uuid,
    collected_at: DateTime<Utc>,
    cpu: Option<f64>,
    memory: Option<f64>,
    temperature: Option<f64>,
) {
    sqlx::query(
        r#"INSERT INTO client_metric_reports(
               report_id,host_id,schema_version,collected_at,received_at,interval_seconds,payload,
               cpu_usage_percent,memory_usage_percent,max_temperature_celsius
           ) VALUES(?,?,?,?,?,10,NULL,?,?,?)"#,
    )
    .bind(report_id)
    .bind(host_id)
    .bind(host_protocol::CLIENT_REPORT_SCHEMA_VERSION)
    .bind(collected_at)
    .bind(collected_at + chrono::Duration::seconds(1))
    .bind(cpu)
    .bind(memory)
    .bind(temperature)
    .execute(pool)
    .await
    .unwrap();
}

async fn raw_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM client_metric_reports")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn aggregate_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM client_metric_hourly_aggregates")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn wait_until(mut condition: impl AsyncFnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !condition().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("condition did not become true");
}

#[tokio::test]
async fn utc_boundaries_nulls_latest_and_aggregate_expiry_are_exact() {
    let database = TestDatabase::new().await;
    let (host_id, _) = database.add_host("boundary-host").await;
    let now = timestamp("2026-08-29T12:30:00Z");
    let latest_id = Uuid::new_v4();
    insert_raw(
        &database.pool,
        host_id,
        latest_id,
        timestamp("2026-08-20T09:00:00Z"),
        Some(99.0),
        Some(99.0),
        Some(99.0),
    )
    .await;
    sqlx::query(
        "UPDATE monitored_hosts SET latest_report_id=?,latest_collected_at=? WHERE host_id=?",
    )
    .bind(latest_id)
    .bind(timestamp("2026-08-20T09:00:00Z"))
    .bind(host_id)
    .execute(&database.pool)
    .await
    .unwrap();

    let all_metrics_id = Uuid::new_v4();
    for (report_id, time, cpu, memory) in [
        (all_metrics_id, "2026-08-28T10:05:00Z", Some(10.0), None),
        (Uuid::new_v4(), "2026-08-28T10:20:00Z", None, Some(50.0)),
        (
            Uuid::new_v4(),
            "2026-08-28T10:55:00Z",
            Some(30.0),
            Some(70.0),
        ),
    ] {
        insert_raw(
            &database.pool,
            host_id,
            report_id,
            timestamp(time),
            cpu,
            memory,
            None,
        )
        .await;
    }
    sqlx::query(
        r#"UPDATE client_metric_reports SET
               network_received_bytes_per_second=100,
               network_transmitted_bytes_per_second=200,
               disk_read_bytes_per_second=300,
               disk_written_bytes_per_second=400,
               gpu_utilization_percent=50,
               gpu_memory_usage_percent=60
           WHERE report_id=?"#,
    )
    .bind(all_metrics_id)
    .execute(&database.pool)
    .await
    .unwrap();
    let just_before_raw_cutoff = Uuid::new_v4();
    insert_raw(
        &database.pool,
        host_id,
        just_before_raw_cutoff,
        timestamp("2026-08-28T12:29:59Z"),
        Some(40.0),
        None,
        None,
    )
    .await;
    let exactly_raw_cutoff = Uuid::new_v4();
    insert_raw(
        &database.pool,
        host_id,
        exactly_raw_cutoff,
        timestamp("2026-08-28T12:30:00Z"),
        Some(50.0),
        None,
        None,
    )
    .await;
    insert_raw(
        &database.pool,
        host_id,
        Uuid::new_v4(),
        timestamp("2026-07-30T11:59:00Z"),
        Some(1.0),
        None,
        None,
    )
    .await;
    insert_raw(
        &database.pool,
        host_id,
        Uuid::new_v4(),
        timestamp("2026-07-30T12:30:00Z"),
        Some(2.0),
        None,
        None,
    )
    .await;

    let outcome = run_once_at(&database.pool, retention_config(32, 3), now)
        .await
        .unwrap();
    assert_eq!(outcome.aggregated_reports, 6);
    assert_eq!(outcome.deleted_raw_reports, 5);
    assert_eq!(outcome.deleted_hourly_aggregates, 1);

    let remaining: Vec<Uuid> =
        sqlx::query_scalar("SELECT report_id FROM client_metric_reports ORDER BY report_id")
            .fetch_all(&database.pool)
            .await
            .unwrap();
    assert_eq!(remaining.len(), 3);
    assert!(remaining.contains(&latest_id));
    assert!(remaining.contains(&exactly_raw_cutoff));
    assert!(remaining.contains(&just_before_raw_cutoff));

    let series = store::history_series(
        &database.pool,
        host_id,
        timestamp("2026-08-28T10:00:00Z"),
        timestamp("2026-08-28T13:00:00Z"),
        720,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(series.source, "mixed");
    assert_eq!(series.step_seconds, 3600);
    assert_eq!(series.points.len(), 2);
    assert_eq!(series.points[0].sample_count, 3);
    assert_eq!(series.points[0].cpu_usage_percent.count, 2);
    assert_eq!(series.points[0].cpu_usage_percent.avg, Some(20.0));

    let row = sqlx::query(
        r#"SELECT interval_start,interval_end,sample_count,
                  cpu_usage_percent_count,cpu_usage_percent_min,
                  cpu_usage_percent_max,cpu_usage_percent_avg,
                  memory_usage_percent_count,memory_usage_percent_min,
                  memory_usage_percent_max,memory_usage_percent_avg,
                  max_temperature_celsius_count,max_temperature_celsius_min,
                  max_temperature_celsius_max,max_temperature_celsius_avg,
                  network_received_bytes_per_second_count,network_received_bytes_per_second_avg,
                  network_transmitted_bytes_per_second_count,network_transmitted_bytes_per_second_avg,
                  disk_read_bytes_per_second_count,disk_read_bytes_per_second_avg,
                  disk_written_bytes_per_second_count,disk_written_bytes_per_second_avg,
                  gpu_utilization_percent_count,gpu_utilization_percent_avg,
                  gpu_memory_usage_percent_count,gpu_memory_usage_percent_avg
             FROM client_metric_hourly_aggregates
            WHERE host_id=? AND bucket_start=?"#,
    )
    .bind(host_id)
    .bind(timestamp("2026-08-28T10:00:00Z"))
    .fetch_one(&database.pool)
    .await
    .unwrap();
    use sqlx::Row;
    assert_eq!(
        row.try_get::<DateTime<Utc>, _>("interval_start").unwrap(),
        timestamp("2026-08-28T10:05:00Z")
    );
    assert_eq!(
        row.try_get::<DateTime<Utc>, _>("interval_end").unwrap(),
        timestamp("2026-08-28T10:55:00Z")
    );
    assert_eq!(row.try_get::<i64, _>("sample_count").unwrap(), 3);
    assert_eq!(row.try_get::<i64, _>("cpu_usage_percent_count").unwrap(), 2);
    assert_eq!(
        row.try_get::<f64, _>("cpu_usage_percent_min").unwrap(),
        10.0
    );
    assert_eq!(
        row.try_get::<f64, _>("cpu_usage_percent_max").unwrap(),
        30.0
    );
    assert_eq!(
        row.try_get::<f64, _>("cpu_usage_percent_avg").unwrap(),
        20.0
    );
    assert_eq!(
        row.try_get::<i64, _>("memory_usage_percent_count").unwrap(),
        2
    );
    assert_eq!(
        row.try_get::<f64, _>("memory_usage_percent_min").unwrap(),
        50.0
    );
    assert_eq!(
        row.try_get::<f64, _>("memory_usage_percent_max").unwrap(),
        70.0
    );
    assert_eq!(
        row.try_get::<f64, _>("memory_usage_percent_avg").unwrap(),
        60.0
    );
    assert_eq!(
        row.try_get::<i64, _>("max_temperature_celsius_count")
            .unwrap(),
        0
    );
    for column in [
        "max_temperature_celsius_min",
        "max_temperature_celsius_max",
        "max_temperature_celsius_avg",
    ] {
        assert_eq!(row.try_get::<Option<f64>, _>(column).unwrap(), None);
    }
    for (count_column, average_column, expected) in [
        (
            "network_received_bytes_per_second_count",
            "network_received_bytes_per_second_avg",
            100.0,
        ),
        (
            "network_transmitted_bytes_per_second_count",
            "network_transmitted_bytes_per_second_avg",
            200.0,
        ),
        (
            "disk_read_bytes_per_second_count",
            "disk_read_bytes_per_second_avg",
            300.0,
        ),
        (
            "disk_written_bytes_per_second_count",
            "disk_written_bytes_per_second_avg",
            400.0,
        ),
        (
            "gpu_utilization_percent_count",
            "gpu_utilization_percent_avg",
            50.0,
        ),
        (
            "gpu_memory_usage_percent_count",
            "gpu_memory_usage_percent_avg",
            60.0,
        ),
    ] {
        assert_eq!(row.try_get::<i64, _>(count_column).unwrap(), 1);
        assert_eq!(row.try_get::<f64, _>(average_column).unwrap(), expected);
    }
    assert_eq!(aggregate_count(&database.pool).await, 3);
}

#[tokio::test]
async fn batches_are_bounded_and_completed_reruns_are_idempotent() {
    let database = TestDatabase::new().await;
    let (host_id, _) = database.add_host("batch-host").await;
    let now = timestamp("2026-08-29T12:30:00Z");
    for offset in 0..5 {
        insert_raw(
            &database.pool,
            host_id,
            Uuid::new_v4(),
            timestamp("2026-08-20T08:00:00Z") + chrono::Duration::minutes(offset),
            (offset % 2 == 0).then_some(10.0 + offset as f64),
            None,
            None,
        )
        .await;
    }
    let config = retention_config(2, 3);

    let first = run_once_at(&database.pool, config, now).await.unwrap();
    assert_eq!(
        (first.aggregated_reports, first.deleted_raw_reports),
        (2, 2)
    );
    assert_eq!(raw_count(&database.pool).await, 3);
    let second = run_once_at(&database.pool, config, now).await.unwrap();
    assert_eq!(
        (second.aggregated_reports, second.deleted_raw_reports),
        (2, 2)
    );
    assert_eq!(raw_count(&database.pool).await, 1);
    let third = run_once_at(&database.pool, config, now).await.unwrap();
    assert_eq!(
        (third.aggregated_reports, third.deleted_raw_reports),
        (1, 1)
    );
    assert_eq!(raw_count(&database.pool).await, 0);

    let before: (i64, i64, f64) = sqlx::query_as(
        "SELECT sample_count,cpu_usage_percent_count,cpu_usage_percent_avg \
         FROM client_metric_hourly_aggregates WHERE host_id=?",
    )
    .bind(host_id)
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(before, (5, 3, 12.0));
    let rerun = run_once_at(&database.pool, config, now).await.unwrap();
    assert_eq!(rerun.aggregated_reports, 0);
    let after: (i64, i64, f64) = sqlx::query_as(
        "SELECT sample_count,cpu_usage_percent_count,cpu_usage_percent_avg \
         FROM client_metric_hourly_aggregates WHERE host_id=?",
    )
    .bind(host_id)
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn aggregate_commit_survives_delete_failure_and_restart_without_double_counting() {
    let database = TestDatabase::new().await;
    let (host_id, _) = database.add_host("restart-host").await;
    let now = timestamp("2026-08-29T12:30:00Z");
    let report_id = Uuid::new_v4();
    insert_raw(
        &database.pool,
        host_id,
        report_id,
        timestamp("2026-08-20T08:15:00Z"),
        Some(25.0),
        None,
        None,
    )
    .await;
    sqlx::query(
        r#"CREATE TRIGGER fail_retention_delete
           BEFORE DELETE ON client_metric_reports
           WHEN OLD.aggregated_at IS NOT NULL
           BEGIN SELECT RAISE(FAIL,'forced retention delete failure'); END"#,
    )
    .execute(&database.pool)
    .await
    .unwrap();

    assert!(
        run_once_at(&database.pool, retention_config(8, 3), now)
            .await
            .is_err()
    );
    let marker: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT aggregated_at FROM client_metric_reports WHERE report_id=?")
            .bind(report_id)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert!(marker.is_some(), "aggregate transaction was not durable");
    let count_before: i64 =
        sqlx::query_scalar("SELECT sample_count FROM client_metric_hourly_aggregates")
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert_eq!(count_before, 1);

    database.pool.close().await;
    rusqlite::Connection::open(
        database
            .database_url
            .strip_prefix("sqlite://")
            .expect("test database URL"),
    )
    .unwrap()
    .execute("DROP TRIGGER fail_retention_delete", [])
    .unwrap();
    let reopened = store::open_existing(&database.database_url).await.unwrap();
    let retry = run_once_at(&reopened, retention_config(8, 3), now)
        .await
        .unwrap();
    assert_eq!(retry.aggregated_reports, 0);
    assert_eq!(retry.deleted_raw_reports, 1);
    let count_after: i64 =
        sqlx::query_scalar("SELECT sample_count FROM client_metric_hourly_aggregates")
            .fetch_one(&reopened)
            .await
            .unwrap();
    assert_eq!(count_after, count_before);
    reopened.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn startup_period_failure_and_shutdown_are_supervised() {
    let database = TestDatabase::new().await;
    let (host_id, _) = database.add_host("periodic-host").await;
    let old = Utc::now() - chrono::Duration::days(2);
    insert_raw(
        &database.pool,
        host_id,
        Uuid::new_v4(),
        old,
        Some(1.0),
        None,
        None,
    )
    .await;
    let config = retention_config(8, 3);
    let (maintenance, task) = RetentionMaintenance::start(database.pool.clone(), config);
    wait_until(|| async { maintenance.stats().aggregated_reports == 1 }).await;

    insert_raw(
        &database.pool,
        host_id,
        Uuid::new_v4(),
        old + chrono::Duration::minutes(1),
        Some(3.0),
        None,
        None,
    )
    .await;
    wait_until(|| async { maintenance.stats().aggregated_reports == 2 }).await;
    assert!(maintenance.stats().runs >= 2, "periodic tick did not run");
    task.shutdown().await.unwrap();
    assert!(!maintenance.is_running());

    let closed_directory = tempfile::tempdir().unwrap();
    let closed_pool = store::open_or_initialize(&format!(
        "sqlite://{}",
        closed_directory.path().join("closed.sqlite3").display()
    ))
    .await
    .unwrap();
    closed_pool.close().await;
    let (failed, failed_task) = RetentionMaintenance::start(closed_pool, config);
    wait_until(|| async { failed.stats().failures >= 1 }).await;
    assert!(failed.is_running(), "one failed run stopped the supervisor");
    failed_task.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_cancels_a_waiting_write_lock_without_waiting_for_sqlite_busy_timeout() {
    let database = TestDatabase::new().await;
    let mut blocker: PoolConnection<Sqlite> = database.pool.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (maintenance, task) =
        RetentionMaintenance::start(database.pool.clone(), retention_config(8, 3));
    wait_until(|| async { maintenance.stats().runs >= 1 }).await;
    tokio::time::sleep(Duration::from_millis(25)).await;
    let started = Instant::now();
    task.shutdown().await.unwrap();
    assert!(started.elapsed() < Duration::from_millis(500));
    sqlx::query("ROLLBACK")
        .execute(&mut *blocker)
        .await
        .unwrap();
}

fn report(host_id: Uuid, report_id: Uuid, collected_at: DateTime<Utc>) -> ClientReport {
    ClientReport {
        schema_version: host_protocol::CLIENT_REPORT_SCHEMA_VERSION,
        report_id: report_id.to_string(),
        collected_at,
        host: HostIdentity {
            id: host_id.to_string(),
            os: "linux".into(),
            os_version: Some("test-os".into()),
            kernel_version: Some("test-kernel".into()),
            arch: "x86_64".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        },
        interval_seconds: 10.0,
        system: SystemSnapshot {
            hardware: None,
            uptime_seconds: 60,
            cpu: CpuSnapshot {
                usage_percent: 42.5,
                logical_count: 1,
                physical_count: Some(1),
                per_core_percent: vec![42.5],
            },
            memory: MemorySnapshot {
                total_bytes: 1_000,
                used_bytes: 250,
                available_bytes: 750,
                swap_total_bytes: 0,
                swap_used_bytes: 0,
            },
            networks: vec![],
            disks: vec![],
            temperatures: vec![],
            gpus: vec![],
        },
        capabilities: vec![Capability::available("cpu", "test")],
        client: ClientHealth {
            spool_pending_batches: 0,
            collector_errors: 0,
        },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bounded_maintenance_does_not_starve_the_serial_telemetry_writer() {
    let database = TestDatabase::new().await;
    let (host_id, token) = database.add_host("concurrent-host").await;
    for offset in 0..48 {
        insert_raw(
            &database.pool,
            host_id,
            Uuid::new_v4(),
            Utc::now() - chrono::Duration::days(2) + chrono::Duration::minutes(offset),
            Some(offset as f64),
            None,
            None,
        )
        .await;
    }
    let retention = RetentionConfig::new(
        Duration::from_secs(24 * 60 * 60),
        Duration::from_secs(30 * 24 * 60 * 60),
        Duration::from_secs(1),
        4,
        30,
        Duration::from_millis(750),
        Duration::from_millis(2),
    )
    .unwrap();
    let (maintenance, retention_task) =
        RetentionMaintenance::start(database.pool.clone(), retention);
    let telemetry = TelemetryWriterConfig::new(
        64,
        8,
        Duration::from_millis(20),
        Duration::from_millis(10),
        Duration::from_secs(3),
        Duration::from_secs(2),
    )
    .unwrap();
    let (writer, writer_task) = TelemetryWriter::start(database.pool.clone(), telemetry);
    wait_until(|| async { maintenance.stats().runs >= 1 }).await;
    let mut submissions = JoinSet::new();
    let mut report_ids = Vec::new();
    for offset in 0..20 {
        let report_id = Uuid::new_v4();
        report_ids.push(report_id);
        let item = report(
            host_id,
            report_id,
            Utc::now() + chrono::Duration::milliseconds(offset),
        );
        let metrics = model::validate_report(&item).unwrap();
        let write = ReportWrite::new(item, token_hash(&token), metrics);
        let writer = writer.clone();
        submissions.spawn(async move { writer.submit(write).await });
    }
    while let Some(result) = submissions.join_next().await {
        assert!(result.unwrap().unwrap().0);
    }
    wait_until(|| async { maintenance.stats().aggregated_reports > 0 }).await;
    retention_task.shutdown().await.unwrap();
    writer_task.shutdown().await.unwrap();

    for report_id in report_ids {
        let current: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM client_metric_reports WHERE report_id=?)",
        )
        .bind(report_id)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(current, "current report was lost during retention");
    }
}

#[tokio::test]
async fn hardware_metrics_survive_hourly_rollup_with_null_coverage() {
    use sqlx::Row;
    let db = TestDatabase::new().await;
    let (host_id, _) = db.add_host("hardware").await;
    let time = timestamp("2026-01-01T00:10:00Z");
    let first = Uuid::new_v4();
    insert_raw(&db.pool, host_id, first, time, Some(1.0), Some(2.0), None).await;
    insert_raw(
        &db.pool,
        host_id,
        Uuid::new_v4(),
        time + chrono::Duration::minutes(1),
        None,
        None,
        None,
    )
    .await;
    sqlx::query("UPDATE client_metric_reports SET cpu_frequency_mhz=4200,gpu_power_watts=125,gpu_core_clock_mhz=2400,max_fan_rpm=1200,max_disk_temperature_celsius=42,max_disk_percentage_used=105 WHERE report_id=?")
        .bind(first).execute(&db.pool).await.unwrap();
    run_once_at(
        &db.pool,
        retention_config(64, 12),
        timestamp("2026-01-03T00:00:00Z"),
    )
    .await
    .unwrap();
    let row = sqlx::query("SELECT * FROM client_metric_hourly_aggregates WHERE host_id=?")
        .bind(host_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    for (name, expected) in [
        ("cpu_frequency_mhz", 4200.0),
        ("gpu_power_watts", 125.0),
        ("gpu_core_clock_mhz", 2400.0),
        ("max_fan_rpm", 1200.0),
        ("max_disk_temperature_celsius", 42.0),
        ("max_disk_percentage_used", 105.0),
    ] {
        assert_eq!(row.get::<i64, _>(format!("{name}_count").as_str()), 1);
        assert_eq!(row.get::<f64, _>(format!("{name}_avg").as_str()), expected);
    }
}
