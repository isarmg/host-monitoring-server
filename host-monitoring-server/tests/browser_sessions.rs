use std::path::PathBuf;

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{Method, Request, StatusCode, header},
};
use host_monitoring_server::{
    http::{AppState, router},
    store,
    telemetry::{TelemetryWriterConfig, TelemetryWriterTask},
};
use http_body_util::BodyExt;
use sarmg_admin_auth::{AdministratorOriginMode, CSRF_HEADER};
use sarmg_contracts::AdministratorSession;
use serde_json::json;
use sqlx::SqlitePool;
use tower::ServiceExt;

struct Fixture {
    app: Router,
    pool: SqlitePool,
    _telemetry: TelemetryWriterTask,
}

async fn fixture() -> Fixture {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open test database");
    store::initialize_empty(&pool)
        .await
        .expect("initialize schema");
    store::ensure_admin_user(&pool, "admin", Some("correct-password"))
        .await
        .expect("seed administrator");
    let (state, telemetry) = AppState::with_telemetry_config(
        pool.clone(),
        AdministratorOriginMode::LoopbackDevelopmentHttp,
        TelemetryWriterConfig::production(),
        host_monitoring_server::crypto::SecretBox::new([0x42; 32]),
    );
    Fixture {
        app: router(state, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web"))
            .expect("compose platform router"),
        pool,
        _telemetry: telemetry,
    }
}

fn request(method: Method, uri: &str, cookie: Option<&str>, csrf: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(uri)
        .header(header::HOST, "127.0.0.1")
        .header(header::ORIGIN, "http://127.0.0.1")
        .header(sarmg_admin_auth::SEC_FETCH_SITE_HEADER, "same-origin");
    if let Some(value) = cookie {
        builder = builder.header(header::COOKIE, value);
    }
    if let Some(value) = csrf {
        builder = builder.header(CSRF_HEADER, value);
    }
    let body = if method == Method::POST && uri.ends_with("/login") {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(json!({"username":"admin","password":"correct-password"}).to_string())
    } else {
        Body::empty()
    };
    let mut request = builder.body(body).expect("request");
    request.extensions_mut().insert(ConnectInfo(
        "192.0.2.10:41000"
            .parse::<std::net::SocketAddr>()
            .expect("peer address"),
    ));
    request
}

#[tokio::test]
async fn foundation_login_session_csrf_and_logout_are_used_end_to_end() {
    let fixture = fixture().await;
    let login = fixture
        .app
        .clone()
        .oneshot(request(Method::POST, "/api/v2/auth/login", None, None))
        .await
        .expect("login response");
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .expect("cookie")
        .split(';')
        .next()
        .expect("cookie pair")
        .to_owned();
    let body = login.into_body().collect().await.expect("body").to_bytes();
    let session: AdministratorSession = serde_json::from_slice(&body).expect("session contract");
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sarmg_admin_sessions")
        .fetch_one(&fixture.pool)
        .await
        .expect("session count");
    assert_eq!(stored, 1);

    let protected = fixture
        .app
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/v2/monitoring/hosts",
            Some(&cookie),
            None,
        ))
        .await
        .expect("protected response");
    assert_eq!(protected.status(), StatusCode::OK);
    let rejected_mutation = fixture
        .app
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/v2/monitoring/client-instances",
            Some(&cookie),
            None,
        ))
        .await
        .expect("business mutation csrf rejection");
    assert_eq!(rejected_mutation.status(), StatusCode::FORBIDDEN);
    let body = rejected_mutation
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let error: sarmg_contracts::ErrorEnvelope =
        serde_json::from_slice(&body).expect("Foundation error");
    assert_eq!(error.code.as_str(), "auth.csrf_rejected");
    let rejected = fixture
        .app
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/v2/auth/logout",
            Some(&cookie),
            None,
        ))
        .await
        .expect("csrf rejection");
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    let logout = fixture
        .app
        .clone()
        .oneshot(request(
            Method::POST,
            "/api/v2/auth/logout",
            Some(&cookie),
            Some(&session.csrf_token),
        ))
        .await
        .expect("logout response");
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    let reload = fixture
        .app
        .oneshot(request(
            Method::GET,
            "/api/v2/auth/session",
            Some(&cookie),
            None,
        ))
        .await
        .expect("reload response");
    assert_eq!(reload.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn disabling_foundation_administrator_invalidates_existing_session() {
    let fixture = fixture().await;
    let login = fixture
        .app
        .clone()
        .oneshot(request(Method::POST, "/api/v2/auth/login", None, None))
        .await
        .expect("login response");
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .expect("cookie")
        .split(';')
        .next()
        .expect("cookie pair")
        .to_owned();
    sqlx::query("UPDATE _sarmg_administrators SET active=0,session_version=session_version+1")
        .execute(&fixture.pool)
        .await
        .expect("disable administrator");
    let response = fixture
        .app
        .oneshot(request(
            Method::GET,
            "/api/v2/auth/session",
            Some(&cookie),
            None,
        ))
        .await
        .expect("session response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
