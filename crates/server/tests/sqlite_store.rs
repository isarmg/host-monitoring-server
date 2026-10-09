use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions};
use uuid::Uuid;
use xsos::{database_schema, model, store, token_hash};
use xsos_protocol::{
    Capability, ClientHealth, ClientPairingMode, ClientPairingRequest, ClientReport, CpuSnapshot,
    DiskSnapshot, GpuSnapshot, HostIdentity, MemorySnapshot, NetworkSnapshot, PairingStatus,
    SystemSnapshot,
};

fn database_path() -> PathBuf {
    std::env::temp_dir().join(format!("xsos-sqlite-regression-{}.db", Uuid::new_v4()))
}

async fn open_database(path: &Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true);
    SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .expect("open temporary SQLite database")
}

fn host(id: Uuid, os: &str) -> HostIdentity {
    HostIdentity {
        id: id.to_string(),
        os: os.into(),
        os_version: Some("test-os-version".into()),
        kernel_version: Some("test-kernel".into()),
        arch: "x86_64".into(),
        client_version: env!("CARGO_PKG_VERSION").into(),
    }
}

#[tokio::test]
async fn instance_list_returns_every_instance_in_case_insensitive_name_order() {
    let path = database_path();
    let pool = open_database(&path).await;
    store::initialize_empty(&pool).await.unwrap();
    let secrets = xsos::crypto::SecretBox::new([0x42; 32]);
    for name in ["zulu", "Bravo", "alpha"] {
        let (result, _) = store::create_invite(&pool, &secrets, name, "admin")
            .await
            .unwrap();
        assert!(matches!(result, store::CreateInviteResult::Created(_)));
    }

    let names = store::list_invites(&pool, &secrets)
        .await
        .unwrap()
        .0
        .into_iter()
        .map(|instance| instance.display_name)
        .collect::<Vec<_>>();
    assert_eq!(names, ["alpha", "Bravo", "zulu"]);
    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn report_logs_include_every_receipt_in_the_selected_half_open_day() {
    let path = database_path();
    let pool = open_database(&path).await;
    store::initialize_empty(&pool).await.unwrap();
    let host_id = Uuid::new_v4();
    let other_host = Uuid::new_v4();
    let from = DateTime::parse_from_rfc3339("2026-09-23T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let to = from + Duration::days(1);
    let mut tx = pool.begin().await.unwrap();
    for id in [host_id, other_host] {
        sqlx::query(
            "INSERT INTO monitored_hosts(host_id,name,os,arch,client_version,registered_at,last_seen_at) \
             VALUES(?,'Test','linux','x86_64','test',?,?)",
        )
        .bind(id)
        .bind(from)
        .bind(from)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    for index in 0..1001 {
        sqlx::query(
            "INSERT INTO client_metric_reports(report_id,host_id,schema_version,collected_at,received_at,interval_seconds) \
             VALUES(?,?,3,?,?,10)",
        )
        .bind(Uuid::new_v4())
        .bind(host_id)
        .bind(from - Duration::hours(1))
        .bind(from + Duration::microseconds(index))
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    for (host, received) in [
        (host_id, from - Duration::microseconds(1)),
        (host_id, to),
        (other_host, from),
    ] {
        sqlx::query(
            "INSERT INTO client_metric_reports(report_id,host_id,schema_version,collected_at,received_at,interval_seconds) \
             VALUES(?,?,3,?,?,10)",
        )
        .bind(Uuid::new_v4())
        .bind(host)
        .bind(from - Duration::hours(1))
        .bind(received)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();

    let scope = "selected-day-history-test";
    let mut cursor = None;
    let mut logs = Vec::new();
    let mut identities = std::collections::HashSet::new();
    loop {
        let page = store::report_logs_page(&pool, host_id, from, to, cursor, scope)
            .await
            .unwrap()
            .unwrap();
        assert!(page.rows.len() <= xsos::pagination::PAGE_SIZE);
        for row in &page.rows {
            assert!(
                identities.insert(row.report_id),
                "history repeated across pages"
            );
        }
        logs.extend(page.rows);
        let Some(next) = page.next_cursor else { break };
        cursor = xsos::pagination::Cursor::decode(Some(&next), scope).unwrap();
    }
    assert_eq!(logs.len(), 1001);
    assert_eq!(identities.len(), 1001);
    assert!(
        logs.windows(2)
            .all(|pair| pair[0].received_at >= pair[1].received_at)
    );
    assert_eq!(
        logs.first().unwrap().received_at,
        from + Duration::microseconds(1000)
    );
    assert_eq!(logs.last().unwrap().received_at, from);
    assert!(logs.iter().all(|row| row.collected_at < from));
    assert!(
        store::report_logs(&pool, Uuid::new_v4(), from, to)
            .await
            .unwrap()
            .is_none()
    );
    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

fn report(host_id: Uuid, collected_at: DateTime<Utc>) -> ClientReport {
    ClientReport {
        schema_version: xsos_protocol::CLIENT_REPORT_SCHEMA_VERSION,
        report_id: Uuid::new_v4().to_string(),
        collected_at,
        host: host(host_id, "linux-updated"),
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
            networks: vec![NetworkSnapshot {
                name: "eth0".into(),
                received_bytes_total: 100,
                transmitted_bytes_total: 200,
                received_bytes_per_second: 12.0,
                transmitted_bytes_per_second: 34.0,
                packets_received_total: 10,
                packets_transmitted_total: 20,
                receive_errors_total: 0,
                transmit_errors_total: 0,
            }],
            disks: vec![DiskSnapshot {
                name: "disk0".into(),
                mount_point: "/".into(),
                file_system: "testfs".into(),
                total_bytes: 2_000,
                available_bytes: 1_500,
                read_bytes_total: 300,
                written_bytes_total: 400,
                read_bytes_per_second: 56.0,
                written_bytes_per_second: 78.0,
                is_read_only: false,
            }],
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

#[tokio::test]
async fn future_clock_correction_replaces_latest_without_promoting_delayed_backlog() {
    let path = database_path();
    let pool = open_database(&path).await;
    store::initialize_empty(&pool).await.unwrap();
    let host_id = Uuid::new_v4();
    let credential = token_hash("future-clock-correction-token");
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO monitored_hosts(host_id,name,os,arch,client_version,registered_at,last_seen_at) \
         VALUES(?,'Clock test','linux','x86_64','test',?,?)",
    )
    .bind(host_id)
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO client_credentials(credential_id,host_id,token_hash,issued_at) VALUES(?,?,?,?)")
        .bind(Uuid::new_v4())
        .bind(host_id)
        .bind(&credential)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

    let ahead = report(host_id, now + Duration::minutes(4));
    let ahead_metrics = model::validate_report(&ahead).unwrap();
    let (_, first_received) = store::store_report(&pool, &ahead, &credential, &ahead_metrics)
        .await
        .unwrap();
    let corrected = report(host_id, first_received);
    assert!(corrected.collected_at < ahead.collected_at);
    let corrected_metrics = model::validate_report(&corrected).unwrap();
    store::store_report(&pool, &corrected, &credential, &corrected_metrics)
        .await
        .unwrap();
    let (_, latest) = store::get_host(&pool, host_id).await.unwrap().unwrap();
    assert_eq!(latest, Some(corrected.clone()));

    let delayed = report(host_id, first_received - Duration::minutes(5));
    let delayed_metrics = model::validate_report(&delayed).unwrap();
    store::store_report(&pool, &delayed, &credential, &delayed_metrics)
        .await
        .unwrap();
    let (_, latest) = store::get_host(&pool, host_id).await.unwrap().unwrap();
    assert_eq!(latest, Some(corrected));

    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[test]
fn gpu_temperature_uses_the_common_physical_bounds() {
    let mut value = report(Uuid::new_v4(), Utc::now());
    value.system.gpus.push(GpuSnapshot {
        id: "gpu0".into(),
        vendor: "test".into(),
        name: "test gpu".into(),
        utilization_percent: None,
        memory_total_bytes: None,
        memory_used_bytes: None,
        temperature_celsius: Some(75.0),
        power_watts: None,
        core_clock_mhz: None,
        memory_clock_mhz: None,
        pcie_rx_bytes_per_second: None,
        pcie_tx_bytes_per_second: None,
        source: "test".into(),
    });
    assert!(model::validate_report(&value).is_ok());
    for invalid in [-273.16, 1000.01, f64::NAN, f64::INFINITY] {
        value.system.gpus[0].temperature_celsius = Some(invalid);
        assert!(model::validate_report(&value).is_err());
    }
    value.system.gpus[0].temperature_celsius = None;
    assert!(model::validate_report(&value).is_ok());
}

#[tokio::test]
async fn pending_pairings_are_capped_per_device_without_breaking_idempotent_retries() {
    let path = database_path();
    let pool = open_database(&path).await;
    store::initialize_empty(&pool)
        .await
        .expect("initialize current schema");
    let host_id = Uuid::new_v4();
    let mut first = None;

    for index in 0..4 {
        let request = ClientPairingRequest {
            protocol_version: xsos_protocol::HOST_PAIRING_PROTOCOL_VERSION,
            mode: ClientPairingMode::Fresh,
            host: host(host_id, "linux"),
            token_hash: token_hash(&format!("client-token-{index}")),
            polling_secret_hash: token_hash(&format!("polling-secret-{index}")),
        };
        let result = store::create_pairing(&pool, &request)
            .await
            .expect("create pairing request within device budget");
        assert!(matches!(
            result,
            store::CreatePairingResult::Ready { created: true, .. }
        ));
        if index == 0 {
            first = Some(request);
        }
    }

    let rejected = ClientPairingRequest {
        protocol_version: xsos_protocol::HOST_PAIRING_PROTOCOL_VERSION,
        mode: ClientPairingMode::Fresh,
        host: host(host_id, "linux"),
        token_hash: token_hash("client-token-over-budget"),
        polling_secret_hash: token_hash("polling-secret-over-budget"),
    };
    assert!(matches!(
        store::create_pairing(&pool, &rejected).await.unwrap(),
        store::CreatePairingResult::DeviceAtCapacity
    ));

    let replay = store::create_pairing(&pool, &first.unwrap())
        .await
        .expect("replay existing pairing request");
    assert!(matches!(
        replay,
        store::CreatePairingResult::Ready { created: false, .. }
    ));
}

#[tokio::test]
async fn issued_token_cannot_create_a_pairing_after_its_pairing_row_is_gone() {
    let path = database_path();
    let pool = open_database(&path).await;
    store::initialize_empty(&pool).await.unwrap();
    let old_host = Uuid::new_v4();
    let credential_id = Uuid::new_v4();
    let issued_hash = token_hash("issued-token-with-pruned-pairing-row");
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO monitored_hosts(host_id,name,os,arch,client_version,registered_at,last_seen_at) \
         VALUES(?,'Old Host','linux','x86_64','test',?,?)",
    )
    .bind(old_host)
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO client_credentials(credential_id,host_id,token_hash,issued_at) VALUES(?,?,?,?)")
        .bind(credential_id)
        .bind(old_host)
        .bind(&issued_hash)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    // A retained credential can outlive its pairing request. Both live and
    // revoked credentials still reserve their bearer token hash.
    let mut request = ClientPairingRequest {
        protocol_version: xsos_protocol::HOST_PAIRING_PROTOCOL_VERSION,
        mode: ClientPairingMode::Fresh,
        host: host(Uuid::new_v4(), "linux"),
        token_hash: issued_hash,
        polling_secret_hash: token_hash("reused-token-polling-a"),
    };
    assert!(matches!(
        store::create_pairing(&pool, &request).await.unwrap(),
        store::CreatePairingResult::Conflict
    ));
    sqlx::query("UPDATE client_credentials SET revoked_at=? WHERE credential_id=?")
        .bind(now)
        .bind(credential_id)
        .execute(&pool)
        .await
        .unwrap();
    request.polling_secret_hash = token_hash("reused-token-polling-b");
    assert!(matches!(
        store::create_pairing(&pool, &request).await.unwrap(),
        store::CreatePairingResult::Conflict
    ));
    let created: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM client_pairing_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(created, 0);

    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn expired_pairing_is_persisted_and_retained_for_twenty_four_hours() {
    let path = database_path();
    let pool = open_database(&path).await;
    store::initialize_empty(&pool).await.unwrap();
    let polling_secret_hash = token_hash("retained-expired-polling-secret");
    let request = ClientPairingRequest {
        protocol_version: xsos_protocol::HOST_PAIRING_PROTOCOL_VERSION,
        mode: ClientPairingMode::Fresh,
        host: host(Uuid::new_v4(), "linux"),
        token_hash: token_hash("retained-expired-client-token"),
        polling_secret_hash: polling_secret_hash.clone(),
    };
    let store::CreatePairingResult::Ready { request_id, .. } =
        store::create_pairing(&pool, &request).await.unwrap()
    else {
        panic!("pairing request was not created")
    };
    sqlx::query("UPDATE client_pairing_requests SET expires_at=? WHERE request_id=?")
        .bind(Utc::now() - Duration::hours(1))
        .bind(request_id)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        store::pairing_status(&pool, request_id, &polling_secret_hash)
            .await
            .unwrap(),
        Some((PairingStatus::Expired, None))
    );
    let status: String =
        sqlx::query_scalar("SELECT status FROM client_pairing_requests WHERE request_id=?")
            .bind(request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "expired");
    assert!(
        store::pairing_request_exists(&pool, request_id)
            .await
            .unwrap()
    );

    sqlx::query("UPDATE client_pairing_requests SET expires_at=? WHERE request_id=?")
        .bind(Utc::now() - Duration::hours(25))
        .bind(request_id)
        .execute(&pool)
        .await
        .unwrap();
    let replacement = ClientPairingRequest {
        polling_secret_hash: token_hash("replacement-polling-secret"),
        token_hash: token_hash("replacement-client-token"),
        ..request
    };
    assert!(matches!(
        store::create_pairing(&pool, &replacement).await.unwrap(),
        store::CreatePairingResult::Ready { created: true, .. }
    ));
    assert!(
        !store::pairing_request_exists(&pool, request_id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn cancelled_code_cannot_authorize_a_new_pairing() {
    let path = database_path();
    let pool = open_database(&path).await;
    store::initialize_empty(&pool).await.unwrap();
    let secrets = xsos::crypto::SecretBox::new([0x42; 32]);
    let (result, code) = store::create_invite(&pool, &secrets, "Cancel me", "admin")
        .await
        .unwrap();
    let store::CreateInviteResult::Created(invite) = result else {
        panic!("create failed")
    };
    let code = code.unwrap();
    assert!(matches!(
        store::cancel_invite(&pool, Uuid::parse_str(&invite.request_id).unwrap(), "admin")
            .await
            .unwrap(),
        store::CancelInviteResult::Cancelled
    ));
    let pairing = ClientPairingRequest {
        protocol_version: xsos_protocol::HOST_PAIRING_PROTOCOL_VERSION,
        mode: ClientPairingMode::Fresh,
        host: host(Uuid::new_v4(), "linux"),
        token_hash: token_hash("device-secret"),
        polling_secret_hash: token_hash("polling-secret"),
    };
    let store::CreatePairingResult::Ready { request_id, .. } =
        store::create_pairing(&pool, &pairing).await.unwrap()
    else {
        panic!("pair failed")
    };
    assert!(matches!(
        store::activate(&pool, &secrets, request_id, &token_hash(&code), "admin")
            .await
            .unwrap(),
        store::ActivateResult::Conflict
    ));
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('client_instance_invites')")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(!columns.iter().any(|name| name == "expires_at"));
    assert_eq!(
        store::list_invites(&pool, &secrets).await.unwrap().0[0].status,
        "cancelled"
    );
    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn direct_delete_removes_a_pending_instance_in_one_operation() {
    let path = database_path();
    let pool = open_database(&path).await;
    store::initialize_empty(&pool).await.unwrap();
    let secrets = xsos::crypto::SecretBox::new([0x42; 32]);
    let (result, _) = store::create_invite(&pool, &secrets, "Delete me", "admin")
        .await
        .unwrap();
    let store::CreateInviteResult::Created(invite) = result else {
        panic!("create failed")
    };
    assert!(
        store::delete_invite(&pool, Uuid::parse_str(&invite.request_id).unwrap(), "admin")
            .await
            .unwrap()
    );
    assert!(
        store::list_invites(&pool, &secrets)
            .await
            .unwrap()
            .0
            .is_empty()
    );
    assert!(
        !store::delete_invite(&pool, Uuid::parse_str(&invite.request_id).unwrap(), "admin")
            .await
            .unwrap()
    );
    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn rotating_instance_authorization_revokes_old_credential_and_requires_new_code() {
    let path = database_path();
    let pool = open_database(&path).await;
    store::initialize_empty(&pool).await.unwrap();
    let secrets = xsos::crypto::SecretBox::new([0x42; 32]);
    let (created, old_code) = store::create_invite(&pool, &secrets, "Rotate me", "admin")
        .await
        .unwrap();
    let store::CreateInviteResult::Created(invite) = created else {
        panic!("create failed")
    };
    let old_code = old_code.unwrap();
    let encrypted: Vec<u8> = sqlx::query_scalar(
        "SELECT authorization_code_enc FROM client_instance_invites WHERE invite_id=?",
    )
    .bind(Uuid::parse_str(&invite.request_id).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !encrypted
            .windows(old_code.len())
            .any(|value| value == old_code.as_bytes())
    );

    let old_token_hash = token_hash("old-client-token");
    let first = ClientPairingRequest {
        protocol_version: xsos_protocol::HOST_PAIRING_PROTOCOL_VERSION,
        mode: ClientPairingMode::Fresh,
        host: host(Uuid::new_v4(), "linux"),
        token_hash: old_token_hash.clone(),
        polling_secret_hash: token_hash("old-polling-token"),
    };
    let store::CreatePairingResult::Ready { request_id, .. } =
        store::create_pairing(&pool, &first).await.unwrap()
    else {
        panic!("pairing request failed")
    };
    assert!(matches!(
        store::activate(&pool, &secrets, request_id, &token_hash(&old_code), "admin")
            .await
            .unwrap(),
        store::ActivateResult::Active(_)
    ));
    assert!(
        store::host_for_token(&pool, &old_token_hash)
            .await
            .unwrap()
            .is_some()
    );
    let instance_id = Uuid::parse_str(&invite.instance_id).unwrap();
    let (before_first_report, latest) = store::get_host(&pool, instance_id).await.unwrap().unwrap();
    assert_eq!(before_first_report.status, "offline");
    assert!(latest.is_none());
    assert_eq!(store::host_statistics(&pool).await.unwrap().total.online, 0);

    let old_report = report(instance_id, Utc::now() - Duration::minutes(1));
    let old_metrics = model::validate_report(&old_report).unwrap();
    assert!(
        store::store_report(&pool, &old_report, &old_token_hash, &old_metrics)
            .await
            .unwrap()
            .0
    );
    assert_eq!(
        store::get_host(&pool, instance_id)
            .await
            .unwrap()
            .unwrap()
            .0
            .status,
        "online"
    );
    assert_eq!(store::host_statistics(&pool).await.unwrap().total.online, 1);

    let new_code = "a1".repeat(18);
    let rotated = store::rotate_invite_authorization(
        &pool,
        &secrets,
        Uuid::parse_str(&invite.request_id).unwrap(),
        &new_code,
        "admin",
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(rotated.status, "pending");
    assert_eq!(rotated.authorization_code, new_code);
    assert!(
        store::host_for_token(&pool, &old_token_hash)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store::pairing_status(&pool, request_id, &first.polling_secret_hash)
            .await
            .unwrap(),
        Some((PairingStatus::Denied, None))
    );
    assert_eq!(
        store::pairing_public(&pool, request_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "denied"
    );

    let new_token_hash = token_hash("new-client-token");
    let second = ClientPairingRequest {
        protocol_version: xsos_protocol::HOST_PAIRING_PROTOCOL_VERSION,
        mode: ClientPairingMode::RecoverIdentity,
        host: host(instance_id, "linux"),
        token_hash: new_token_hash.clone(),
        polling_secret_hash: token_hash("new-polling-token"),
    };
    let store::CreatePairingResult::Ready { request_id, .. } =
        store::create_pairing(&pool, &second).await.unwrap()
    else {
        panic!("replacement pairing request failed")
    };
    assert!(matches!(
        store::activate(&pool, &secrets, request_id, &token_hash(&old_code), "admin")
            .await
            .unwrap(),
        store::ActivateResult::InvalidCode
    ));
    assert!(matches!(
        store::activate(&pool, &secrets, request_id, &token_hash(&new_code), "admin")
            .await
            .unwrap(),
        store::ActivateResult::Active(_)
    ));
    assert!(
        store::host_for_token(&pool, &new_token_hash)
            .await
            .unwrap()
            .is_some()
    );
    let (rebound, latest) = store::get_host(&pool, instance_id).await.unwrap().unwrap();
    assert_eq!(rebound.status, "offline");
    assert!(rebound.latest_collected_at.is_none());
    assert!(rebound.capabilities.is_empty());
    assert!(rebound.metrics.cpu_usage_percent.is_none());
    assert!(latest.is_none());
    assert_eq!(store::host_statistics(&pool).await.unwrap().total.online, 0);
    let old_payload: Option<String> =
        sqlx::query_scalar("SELECT payload FROM client_metric_reports WHERE report_id=?")
            .bind(Uuid::parse_str(&old_report.report_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(old_payload.is_none());
    assert_eq!(
        store::history(&pool, instance_id, None, None, 10)
            .await
            .unwrap()
            .unwrap()
            .len(),
        1
    );

    // A replacement client may have an earlier clock than the previous one.
    // Its first accepted report must become current regardless of that clock.
    let new_report = report(instance_id, old_report.collected_at - Duration::minutes(1));
    let new_metrics = model::validate_report(&new_report).unwrap();
    assert!(
        store::store_report(&pool, &new_report, &new_token_hash, &new_metrics)
            .await
            .unwrap()
            .0
    );
    let (current, latest) = store::get_host(&pool, instance_id).await.unwrap().unwrap();
    assert_eq!(current.status, "online");
    assert_eq!(current.latest_collected_at, Some(new_report.collected_at));
    assert_eq!(latest, Some(new_report));
    assert_eq!(store::host_statistics(&pool).await.unwrap().total.online, 1);

    let final_code = "b2".repeat(18);
    store::rotate_invite_authorization(
        &pool,
        &secrets,
        Uuid::parse_str(&invite.request_id).unwrap(),
        &final_code,
        "admin",
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(
        store::cancel_invite(&pool, Uuid::parse_str(&invite.request_id).unwrap(), "admin")
            .await
            .unwrap(),
        store::CancelInviteResult::Cancelled
    ));
    assert!(matches!(
        store::cancel_invite(&pool, Uuid::parse_str(&invite.request_id).unwrap(), "admin")
            .await
            .unwrap(),
        store::CancelInviteResult::Deleted
    ));
    let remaining: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM monitored_hosts) + (SELECT count(*) FROM client_credentials) + (SELECT count(*) FROM client_metric_reports) + (SELECT count(*) FROM client_metric_hourly_aggregates) + (SELECT count(*) FROM client_pairing_requests) + (SELECT count(*) FROM client_instance_invites)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0);
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(audits > 0);

    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn recovery_preserves_an_absent_host_identity_and_never_overwrites_an_existing_host() {
    let path = database_path();
    let pool = open_database(&path).await;
    store::initialize_empty(&pool).await.unwrap();
    let secrets = xsos::crypto::SecretBox::new([0x42; 32]);
    let old_host_id = Uuid::new_v4();

    let (created, code) = store::create_invite(&pool, &secrets, "Recovered host", "admin")
        .await
        .unwrap();
    let store::CreateInviteResult::Created(invite) = created else {
        panic!("invite creation failed")
    };
    assert_ne!(invite.instance_id, old_host_id.to_string());
    let code = code.unwrap();
    let credential_hash = token_hash("recovered-host-token");
    let request = ClientPairingRequest {
        protocol_version: xsos_protocol::HOST_PAIRING_PROTOCOL_VERSION,
        mode: ClientPairingMode::RecoverIdentity,
        host: host(old_host_id, "windows"),
        token_hash: credential_hash.clone(),
        polling_secret_hash: token_hash("recovered-host-polling"),
    };
    let store::CreatePairingResult::Ready { request_id, .. } =
        store::create_pairing(&pool, &request).await.unwrap()
    else {
        panic!("recovery request creation failed")
    };
    assert!(matches!(
        store::activate(&pool, &secrets, request_id, &token_hash(&code), "admin")
            .await
            .unwrap(),
        store::ActivateResult::Active(id) if id == old_host_id
    ));
    assert_eq!(
        store::host_for_token(&pool, &credential_hash)
            .await
            .unwrap(),
        Some(old_host_id)
    );
    let rebound = store::list_invites(&pool, &secrets).await.unwrap().0;
    assert_eq!(rebound[0].instance_id, old_host_id.to_string());
    assert_eq!(rebound[0].authorization_code, code);

    let same_invite_request = ClientPairingRequest {
        protocol_version: xsos_protocol::HOST_PAIRING_PROTOCOL_VERSION,
        mode: ClientPairingMode::RecoverIdentity,
        host: host(old_host_id, "linux"),
        token_hash: token_hash("same-invite-active-host-token"),
        polling_secret_hash: token_hash("same-invite-active-host-polling"),
    };
    let store::CreatePairingResult::Ready { request_id, .. } =
        store::create_pairing(&pool, &same_invite_request)
            .await
            .unwrap()
    else {
        panic!("second recovery request creation failed")
    };
    assert!(matches!(
        store::activate(&pool, &secrets, request_id, &token_hash(&code), "admin")
            .await
            .unwrap(),
        store::ActivateResult::Conflict
    ));

    let (second, second_code) = store::create_invite(&pool, &secrets, "Collision", "admin")
        .await
        .unwrap();
    let store::CreateInviteResult::Created(_) = second else {
        panic!("second invite creation failed")
    };
    let second_request = ClientPairingRequest {
        protocol_version: xsos_protocol::HOST_PAIRING_PROTOCOL_VERSION,
        mode: ClientPairingMode::RecoverIdentity,
        host: host(old_host_id, "linux"),
        token_hash: token_hash("collision-token"),
        polling_secret_hash: token_hash("collision-polling"),
    };
    let store::CreatePairingResult::Ready { request_id, .. } =
        store::create_pairing(&pool, &second_request).await.unwrap()
    else {
        panic!("second recovery request creation failed")
    };
    assert!(matches!(
        store::activate(
            &pool,
            &secrets,
            request_id,
            &token_hash(&second_code.unwrap()),
            "admin",
        )
        .await
        .unwrap(),
        store::ActivateResult::Conflict
    ));

    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn current_sqlite_supports_pair_activate_report_rename_and_delete() {
    let path = database_path();
    let pool = open_database(&path).await;
    store::initialize_empty(&pool)
        .await
        .expect("initialize current schema");

    let secrets = xsos::crypto::SecretBox::new([0x42; 32]);
    let (invite_result, activation_code) =
        store::create_invite(&pool, &secrets, "Server One", "admin")
            .await
            .expect("create invite");
    let store::CreateInviteResult::Created(invite) = invite_result else {
        panic!("fresh database unexpectedly rejected an invite");
    };
    let activation_code = activation_code.expect("created invite has an activation code");
    let invite_id = Uuid::parse_str(&invite.request_id).expect("canonical invite id");
    assert!(
        store::update_instance_name(&pool, invite_id, "Pending Server", "admin")
            .await
            .expect("rename pending instance")
    );
    assert_eq!(
        store::list_invites(&pool, &secrets).await.unwrap().0[0].display_name,
        "Pending Server"
    );
    // Code age is not an authorization deadline; only explicit cancel/use invalidates it.
    sqlx::query("UPDATE client_instance_invites SET created_at='2000-01-01T00:00:00Z'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        store::list_invites(&pool, &secrets).await.unwrap().0[0].status,
        "pending"
    );
    let instance_id = Uuid::parse_str(&invite.instance_id).expect("canonical instance id");

    let client_token = "client-token-for-sqlite-regression";
    let polling_secret = "polling-secret-for-sqlite-regression";
    let client_token_hash = token_hash(client_token);
    let polling_secret_hash = token_hash(polling_secret);
    let pairing = ClientPairingRequest {
        protocol_version: xsos_protocol::HOST_PAIRING_PROTOCOL_VERSION,
        mode: ClientPairingMode::Fresh,
        host: host(Uuid::new_v4(), "linux"),
        token_hash: client_token_hash.clone(),
        polling_secret_hash: polling_secret_hash.clone(),
    };
    let pairing_result = store::create_pairing(&pool, &pairing)
        .await
        .expect("create pairing request");
    let request_id = match pairing_result {
        store::CreatePairingResult::Ready {
            request_id,
            created: true,
            ..
        } => request_id,
        _ => panic!("fresh pairing request was not created"),
    };
    let pairing_created_at: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT created_at FROM client_pairing_requests WHERE request_id=?")
            .bind(request_id)
            .fetch_one(&pool)
            .await
            .expect("pairing request includes created_at");
    assert!(pairing_created_at.is_some());

    let activated = store::activate(
        &pool,
        &secrets,
        request_id,
        &token_hash(&activation_code),
        "admin",
    )
    .await
    .expect("activate pairing");
    match activated {
        store::ActivateResult::Active(id) => assert_eq!(id, instance_id),
        _ => panic!("valid invite did not activate the pairing"),
    }
    assert_eq!(
        store::pairing_status(&pool, request_id, &polling_secret_hash)
            .await
            .expect("read pairing status"),
        Some((PairingStatus::Active, Some(instance_id.to_string())))
    );
    assert_eq!(
        store::host_for_token(&pool, &client_token_hash)
            .await
            .expect("resolve credential"),
        Some(instance_id)
    );

    let collected_at = Utc::now() - Duration::seconds(1);
    let mut report = report(instance_id, collected_at);
    report.system.hardware = Some(xsos_protocol::HardwareSnapshot {
        collected_at,
        cpu: xsos_protocol::CpuHardware {
            model: Some("Modern CPU".into()),
            frequency_mhz: Some(4200.0),
            ..Default::default()
        },
        networks: vec![],
        physical_networks: vec![],
        sensors: vec![],
        disk_health: vec![],
        inventory_collected_at: Some(collected_at),
        memory_modules: vec![xsos_protocol::MemoryModule {
            id: "dimm0".into(),
            model: Some("M323R2GA3BB0".into()),
            vendor: Some("Samsung".into()),
            memory_type: Some("DDR5".into()),
            capacity_bytes: Some(16 * 1024 * 1024 * 1024),
            speed_mt_s: Some(5600.0),
            configured_speed_mt_s: Some(4800.0),
            source: "linux-smbios".into(),
            ..Default::default()
        }],
        devices: vec![xsos_protocol::HardwareDevice {
            id: "alsa:card0".into(),
            kind: xsos_protocol::HardwareDeviceKind::Audio,
            name: "Realtek ALC295".into(),
            model: Some("ALC295".into()),
            vendor: Some("Realtek".into()),
            vendor_id: None,
            product_id: None,
            revision: None,
            version: None,
            bus: Some("HDA".into()),
            driver: None,
            connection: None,
            speed_mbps: None,
            source: "linux-alsa".into(),
        }],
    });
    let metrics = model::validate_report(&report).expect("valid report fixture");
    let (accepted, received_at) = store::store_report(&pool, &report, &client_token_hash, &metrics)
        .await
        .expect("store telemetry report");
    assert!(accepted);

    let (summary, latest) = store::get_host(&pool, instance_id)
        .await
        .expect("read host")
        .expect("activated host exists");
    assert_eq!(summary.name, "Pending Server");
    assert_eq!(summary.os, "linux-updated");
    assert_eq!(summary.last_seen_at, received_at);
    assert_eq!(summary.latest_collected_at, Some(collected_at));
    assert_eq!(summary.metrics.cpu_usage_percent, Some(42.5));
    assert_eq!(summary.metrics.cpu_frequency_mhz, Some(4200.0));
    assert_eq!(latest, Some(report.clone()));

    let clock_rollback = self::report(instance_id, collected_at - Duration::minutes(10));
    let rollback_metrics = model::validate_report(&clock_rollback).unwrap();
    let (_, rollback_received_at) = store::store_report(
        &pool,
        &clock_rollback,
        &client_token_hash,
        &rollback_metrics,
    )
    .await
    .expect("accept report after the client clock moves backwards");
    let (after_rollback, latest_after_rollback) =
        store::get_host(&pool, instance_id).await.unwrap().unwrap();
    assert_eq!(after_rollback.last_seen_at, rollback_received_at);
    assert_eq!(after_rollback.latest_collected_at, Some(collected_at));
    assert_eq!(latest_after_rollback, Some(report.clone()));

    let credential_last_used: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT last_used_at FROM client_credentials WHERE token_hash=?")
            .bind(&client_token_hash)
            .fetch_one(&pool)
            .await
            .expect("read credential timestamp");
    assert_eq!(credential_last_used, Some(rollback_received_at));

    let history = store::history(
        &pool,
        instance_id,
        Some(collected_at - Duration::seconds(1)),
        Some(collected_at + Duration::seconds(1)),
        10,
    )
    .await
    .expect("query bounded history")
    .expect("host exists");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].report_id, report.report_id);

    assert!(
        store::update_instance_name(&pool, invite_id, "Renamed Server", "admin")
            .await
            .expect("rename active instance")
    );
    assert_eq!(
        store::get_host(&pool, instance_id)
            .await
            .expect("read renamed host")
            .expect("renamed host exists")
            .0
            .name,
        "Renamed Server"
    );
    assert_eq!(
        store::list_invites(&pool, &secrets).await.unwrap().0[0].display_name,
        "Renamed Server"
    );

    assert!(
        store::delete_host(&pool, instance_id, "admin")
            .await
            .expect("delete host")
    );
    let remaining: i64 = sqlx::query_scalar(
        "SELECT \
           (SELECT count(*) FROM monitored_hosts) + \
           (SELECT count(*) FROM client_metric_reports) + \
           (SELECT count(*) FROM client_credentials) + \
           (SELECT count(*) FROM client_pairing_requests) + \
           (SELECT count(*) FROM client_instance_invites)",
    )
    .fetch_one(&pool)
    .await
    .expect("count deleted host data");
    assert_eq!(remaining, 0);
    let complete_audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE created_at IS NOT NULL")
            .fetch_one(&pool)
            .await
            .expect("count complete audit events");
    assert_eq!(complete_audits, 5);

    pool.close().await;
    let reopened = open_database(&path).await;
    database_schema::validate_pool(&reopened)
        .await
        .expect("reopened database retains exact current schema");
    assert!(
        store::get_host(&reopened, instance_id)
            .await
            .expect("read reopened database")
            .is_none()
    );
    reopened.close().await;
    std::fs::remove_file(path).expect("remove temporary SQLite database");
}

#[test]
fn hardware_validation_rejects_old_protocol_and_invalid_readings() {
    use xsos_protocol::*;
    let mut value = report(Uuid::new_v4(), Utc::now());
    value.schema_version = 0;
    assert!(model::validate_report(&value).is_err());
    value.schema_version = 3;
    assert!(model::validate_report(&value).is_err());
    value.schema_version = CLIENT_REPORT_SCHEMA_VERSION;
    value.system.hardware = Some(HardwareSnapshot {
        inventory_collected_at: None,
        memory_modules: vec![],
        devices: vec![],
        collected_at: value.collected_at,
        cpu: CpuHardware {
            frequency_mhz: Some(4200.0),
            ..Default::default()
        },
        networks: vec![],
        physical_networks: vec![PhysicalNetworkAdapter {
            id: "pci-0000:03:00.0".into(),
            name: "Intel I225-V".into(),
            interface_name: Some("eth0".into()),
            mac_address: Some("02:00:00:00:00:01".into()),
            link_speed_mbps: Some(2500.0),
            source: "linux-sysfs-net-device".into(),
        }],
        sensors: vec![HardwareSensor {
            id: "fan1".into(),
            label: "CPU fan".into(),
            kind: SensorKind::FanRpm,
            value: 1200.0,
            source: "test".into(),
        }],
        disk_health: vec![DiskHealth {
            device: "/dev/nvme0".into(),
            collected_at: value.collected_at,
            percentage_used: Some(105.0),
            temperature_celsius: Some(42.0),
            source: "smartctl-json".into(),
            ..Default::default()
        }],
    });
    let summary = model::validate_report(&value).unwrap();
    assert_eq!(summary.cpu_frequency_mhz, Some(4200.0));
    assert_eq!(summary.max_fan_rpm, Some(1200.0));
    assert_eq!(summary.max_disk_percentage_used, Some(105.0));
    assert_eq!(summary.max_disk_temperature_celsius, Some(42.0));
    let mut duplicate = value.clone();
    let adapter = duplicate
        .system
        .hardware
        .as_ref()
        .unwrap()
        .physical_networks[0]
        .clone();
    duplicate
        .system
        .hardware
        .as_mut()
        .unwrap()
        .physical_networks
        .push(adapter);
    assert!(model::validate_report(&duplicate).is_err());
    for invalid in [f64::NAN, f64::INFINITY, -1.0] {
        value.system.hardware.as_mut().unwrap().sensors[0].value = invalid;
        assert!(model::validate_report(&value).is_err());
    }
    value.system.hardware.as_mut().unwrap().sensors[0].kind = SensorKind::VoltageVolts;
    assert!(model::validate_report(&value).is_ok()); // negative rails are valid
}
