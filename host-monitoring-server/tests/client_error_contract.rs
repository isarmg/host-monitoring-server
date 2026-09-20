use std::path::PathBuf;

use axum::{
    Router,
    body::Body,
    http::{Request, Response, StatusCode, header},
};
use chrono::Utc;
use host_monitoring_server::{
    http::{AppState, router},
    store,
    telemetry::{TelemetryWriterConfig, TelemetryWriterTask},
    token_hash,
};
use host_protocol::{
    ClientHealth, ClientReport, CpuSnapshot, HostIdentity, MemorySnapshot, SystemSnapshot,
};
use http_body_util::BodyExt;
use sarmg_error::ErrorEnvelope;
use sqlx::SqlitePool;
use tower::ServiceExt;
use uuid::Uuid;

struct Fixture {
    app: Router,
    pool: SqlitePool,
    _telemetry_writer: TelemetryWriterTask,
}

async fn fixture() -> Fixture {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open test database");
    store::initialize_empty(&pool)
        .await
        .expect("initialize current test schema");
    let (state, telemetry_writer) = AppState::with_telemetry_config(
        pool.clone(),
        sarmg_admin_auth::AdministratorOriginMode::LoopbackDevelopmentHttp,
        TelemetryWriterConfig::production(),
        host_monitoring_server::crypto::SecretBox::new([0x42; 32]),
    );
    Fixture {
        app: router(state, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web"))
            .expect("compose platform router"),
        pool,
        _telemetry_writer: telemetry_writer,
    }
}

fn report(host_id: Uuid) -> ClientReport {
    ClientReport {
        schema_version: host_protocol::CLIENT_REPORT_SCHEMA_VERSION,
        report_id: Uuid::new_v4().to_string(),
        collected_at: Utc::now(),
        host: HostIdentity {
            id: host_id.to_string(),
            os: "linux".into(),
            os_version: None,
            kernel_version: None,
            arch: "x86_64".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        },
        interval_seconds: 10.0,
        system: SystemSnapshot {
            uptime_seconds: 1,
            cpu: CpuSnapshot {
                usage_percent: 0.0,
                logical_count: 1,
                physical_count: Some(1),
                per_core_percent: vec![0.0],
            },
            memory: MemorySnapshot {
                total_bytes: 1,
                used_bytes: 0,
                available_bytes: 1,
                swap_total_bytes: 0,
                swap_used_bytes: 0,
            },
            networks: Vec::new(),
            disks: Vec::new(),
            temperatures: Vec::new(),
            gpus: Vec::new(),
        },
        capabilities: Vec::new(),
        client: ClientHealth {
            spool_pending_batches: 0,
            collector_errors: 0,
        },
    }
}

async fn send_report(fixture: &Fixture, token: &str, report: &ClientReport) -> Response<Body> {
    fixture
        .app
        .clone()
        .oneshot(
            Request::post(host_protocol::CLIENT_REPORT_PATH)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(report).unwrap()))
                .unwrap(),
        )
        .await
        .expect("router response")
}

async fn response_contract(response: Response<Body>) -> (StatusCode, String, Vec<u8>) {
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .expect("error response content type")
        .to_str()
        .unwrap()
        .to_owned();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes()
        .to_vec();
    serde_json::from_slice::<ErrorEnvelope>(&body).expect("strict Foundation error envelope");
    (status, content_type, body)
}

#[tokio::test]
async fn server_error_envelopes_define_the_client_credential_contract() {
    let fixture = fixture().await;

    let unknown_host_report = report(Uuid::new_v4());
    let (status, _, body) = response_contract(
        send_report(&fixture, "revoked-or-unknown-token", &unknown_host_report).await,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let envelope: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(envelope.code.as_str(), "unauthorized");
    assert!(!envelope.retryable);

    let credential_host = Uuid::new_v4();
    let token = "current-host-credential";
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO monitored_hosts(\
           host_id,name,os,arch,client_version,registered_at,last_seen_at\
         ) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(credential_host)
    .bind("Bound Host")
    .bind("linux")
    .bind("x86_64")
    .bind(env!("CARGO_PKG_VERSION"))
    .bind(now)
    .bind(now)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO client_credentials(credential_id,host_id,token_hash,issued_at) VALUES(?,?,?,?)",
    )
    .bind(Uuid::new_v4())
    .bind(credential_host)
    .bind(token_hash(token))
    .bind(now)
    .execute(&fixture.pool)
    .await
    .unwrap();

    let mismatched_report = report(Uuid::new_v4());
    let (status, _, body) =
        response_contract(send_report(&fixture, token, &mismatched_report).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let envelope: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(envelope.code.as_str(), "client_host_mismatch");
    assert!(!envelope.retryable);
}

#[tokio::test]
async fn framework_api_rejections_are_replaced_by_the_same_strict_envelope() {
    let fixture = fixture().await;
    let malformed = fixture
        .app
        .clone()
        .oneshot(
            Request::post(host_protocol::CLIENT_REPORT_PATH)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, _, body) = response_contract(malformed).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let envelope: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(envelope.code.as_str(), "bad_request");
    assert!(!envelope.retryable);

    let unknown = fixture
        .app
        .clone()
        .oneshot(
            Request::get("/api/v3/removed-route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, _, body) = response_contract(unknown).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let envelope: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(envelope.code.as_str(), "not_found");
    assert!(!envelope.retryable);

    let wrong_method = fixture
        .app
        .clone()
        .oneshot(
            Request::get(host_protocol::CLIENT_REPORT_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, _, body) = response_contract(wrong_method).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    let envelope: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(envelope.code.as_str(), "method_not_allowed");
    assert!(!envelope.retryable);
}

#[tokio::test]
async fn missing_pairing_status_uses_the_dedicated_transaction_code() {
    let fixture = fixture().await;
    let request_id = Uuid::new_v4();
    let mut request = Request::post(format!(
        "/api/v2/host-monitor/pairing-requests/{request_id}/status"
    ))
    .header(header::AUTHORIZATION, format!("Pairing {}", "a".repeat(64)))
    .body(Body::empty())
    .unwrap();
    request.extensions_mut().insert(axum::extract::ConnectInfo(
        "127.0.0.1:42000".parse::<std::net::SocketAddr>().unwrap(),
    ));
    let response = fixture.app.clone().oneshot(request).await.unwrap();
    let (status, _, body) = response_contract(response).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let envelope: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
    assert_eq!(envelope.code.as_str(), "pairing_transaction_not_found");
    assert_eq!(envelope.details["request_id"], request_id.to_string());
    assert!(!envelope.retryable);
}

#[tokio::test]
async fn credential_status_requires_a_current_token_and_returns_the_bound_identity() {
    let fixture = fixture().await;
    let host_id = Uuid::new_v4();
    let token = "credential-status-current-token-0123456789";
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO monitored_hosts(\
           host_id,name,os,arch,client_version,registered_at,last_seen_at\
         ) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(host_id)
    .bind("Credential status")
    .bind("linux")
    .bind("x86_64")
    .bind("0.9.999")
    .bind(now)
    .bind(now)
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO client_credentials(credential_id,host_id,token_hash,issued_at) VALUES(?,?,?,?)",
    )
    .bind(Uuid::new_v4())
    .bind(host_id)
    .bind(token_hash(token))
    .bind(now)
    .execute(&fixture.pool)
    .await
    .unwrap();

    let accepted = fixture
        .app
        .clone()
        .oneshot(
            Request::get(host_protocol::CLIENT_CREDENTIAL_STATUS_PATH)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(accepted.headers()[header::CACHE_CONTROL], "no-store");
    let body = accepted.into_body().collect().await.unwrap().to_bytes();
    let status: host_protocol::CredentialStatusResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(status.status, host_protocol::CredentialStatus::Authorized);
    assert_eq!(status.host_id, host_id.to_string());
    assert_eq!(status.instance_id, host_id.to_string());
    assert_eq!(
        status.protocol_version,
        host_protocol::HOST_PAIRING_PROTOCOL_VERSION
    );

    let rejected = fixture
        .app
        .clone()
        .oneshot(
            Request::get(host_protocol::CLIENT_CREDENTIAL_STATUS_PATH)
                .header(
                    header::AUTHORIZATION,
                    "Bearer unknown-credential-status-token-0123456789",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    let envelope: ErrorEnvelope =
        serde_json::from_slice(&rejected.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(envelope.code.as_str(), "unauthorized");
}
