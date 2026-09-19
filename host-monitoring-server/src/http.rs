use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, DefaultBodyLimit, Extension, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use chrono::Utc;
use host_protocol::{
    ActivateClientRequest, ActivateClientResponse, ActivatePairingStatus,
    CLIENT_REPORT_MAX_BODY_BYTES, ClientPairingRequest, ClientPairingResponse,
    ClientPairingStatusResponse, ClientReport, ClientReportAck, CredentialStatus,
    CredentialStatusResponse, HOST_PAIRING_PROTOCOL_VERSION,
};
use sarmg_admin_auth::AdministratorOriginMode;
use sarmg_admin_core::AdministratorService;
use sarmg_admin_sqlite::SqliteAdministratorStore;
use tokio::sync::Mutex;
use tower_http::services::{ServeDir, ServeFile};

use crate::{
    error::{Error, FoundationErrorEnvelope, Result, database, framework_envelope},
    model::{
        ClientInstanceListQuery, ClientInstanceListResponse, CreateClientInstanceRequest,
        CreatedClientInstance, HistoryQuery, HistoryResponse, HostDetailResponse, HostListQuery,
        HostListResponse, UpdateClientAuthorizationRequest, UpdateMonitoringRemarkRequest,
        canonical_uuid, validate_pairing, validate_report,
    },
    store,
    telemetry::{
        TelemetrySubmitError, TelemetryWriter, TelemetryWriterConfig, TelemetryWriterTask,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub secrets: crate::crypto::SecretBox,
    administrator: Arc<AdministratorService<SqliteAdministratorStore>>,
    administrator_origin: AdministratorOriginMode,
    runtime: sarmg_server_runtime::RuntimeHandle,
    pairing_admission: crate::pairing_admission::PairingAdmission,
    report_buckets: Arc<Mutex<ReportBuckets>>,
    telemetry: TelemetryWriter,
    #[cfg(test)]
    _telemetry_task: Option<Arc<TelemetryWriterTask>>,
}

impl AppState {
    #[cfg(test)]
    pub fn new(
        pool: sqlx::SqlitePool,
        origin: AdministratorOriginMode,
        secrets: crate::crypto::SecretBox,
    ) -> Self {
        let (mut state, task) =
            Self::with_telemetry_config(pool, origin, TelemetryWriterConfig::production(), secrets);
        state._telemetry_task = Some(Arc::new(task));
        state
    }

    pub fn with_telemetry_config(
        pool: sqlx::SqlitePool,
        origin: AdministratorOriginMode,
        config: TelemetryWriterConfig,
        secrets: crate::crypto::SecretBox,
    ) -> (Self, TelemetryWriterTask) {
        let (telemetry, task) = TelemetryWriter::start(pool.clone(), config);
        (
            Self::with_telemetry_writer(pool, origin, telemetry, secrets),
            task,
        )
    }

    pub fn with_telemetry_writer(
        pool: sqlx::SqlitePool,
        origin: AdministratorOriginMode,
        telemetry: TelemetryWriter,
        secrets: crate::crypto::SecretBox,
    ) -> Self {
        let runtime = sarmg_server_runtime::platform_handle(product_descriptor())
            .expect("the compiled Host Monitoring descriptor is valid");
        Self::with_runtime(pool, origin, telemetry, runtime, secrets)
    }

    pub fn with_runtime(
        pool: sqlx::SqlitePool,
        administrator_origin: AdministratorOriginMode,
        telemetry: TelemetryWriter,
        runtime: sarmg_server_runtime::RuntimeHandle,
        secrets: crate::crypto::SecretBox,
    ) -> Self {
        let administrator = Arc::new(AdministratorService::new(SqliteAdministratorStore::new(
            pool.clone(),
        )));
        Self {
            pool,
            secrets,
            administrator,
            administrator_origin,
            runtime,
            pairing_admission: crate::pairing_admission::PairingAdmission::production(),
            report_buckets: Arc::new(Mutex::new(ReportBuckets::production())),
            telemetry,
            #[cfg(test)]
            _telemetry_task: None,
        }
    }

    #[cfg(test)]
    fn with_pairing_admission(
        pool: sqlx::SqlitePool,
        origin: AdministratorOriginMode,
        pairing_admission: crate::pairing_admission::PairingAdmission,
    ) -> Self {
        let mut state = Self::new(pool, origin, crate::crypto::SecretBox::new([0x42; 32]));
        state.pairing_admission = pairing_admission;
        state
    }
}

pub fn product_descriptor() -> sarmg_server_runtime::ProductDescriptor {
    sarmg_server_runtime::ProductDescriptor {
        id: "host-monitoring".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        foundation_revision: env!("SARMG_FOUNDATION_REVISION").into(),
        profile: "server-control-plane".into(),
        capabilities: vec![
            "admin-persistent".into(),
            "server-runtime".into(),
            "server-health".into(),
        ],
    }
}

#[derive(Clone, Debug)]
struct Principal {
    subject: String,
}

const REPORT_BUCKET_BURST: f64 = 64.0;
const REPORT_BUCKET_REFILL_PER_SECOND: f64 = 16.0;
const REPORT_BUCKET_CAPACITY: usize = 16_384;
const REPORT_BUCKET_TTL: Duration = Duration::from_secs(15 * 60);

struct TokenBucket {
    tokens: f64,
    updated: Instant,
    last_seen: Instant,
}

impl TokenBucket {
    fn full(now: Instant) -> Self {
        Self {
            tokens: REPORT_BUCKET_BURST,
            updated: now,
            last_seen: now,
        }
    }

    fn tokens_at(&self, now: Instant) -> f64 {
        (self.tokens
            + now.saturating_duration_since(self.updated).as_secs_f64()
                * REPORT_BUCKET_REFILL_PER_SECOND)
            .min(REPORT_BUCKET_BURST)
    }

    fn allow_at(&mut self, now: Instant) -> std::result::Result<(), Duration> {
        self.tokens = self.tokens_at(now);
        self.updated = now;
        self.last_seen = now;
        if self.tokens < 1.0 {
            return Err(Duration::from_secs_f64(
                (1.0 - self.tokens) / REPORT_BUCKET_REFILL_PER_SECOND,
            ));
        }
        self.tokens -= 1.0;
        Ok(())
    }
}

struct ReportBuckets {
    entries: HashMap<String, TokenBucket>,
    capacity: usize,
    entry_ttl: Duration,
}

impl ReportBuckets {
    fn production() -> Self {
        Self::new(REPORT_BUCKET_CAPACITY, REPORT_BUCKET_TTL)
    }

    fn new(capacity: usize, entry_ttl: Duration) -> Self {
        assert!(capacity > 0);
        assert!(!entry_ttl.is_zero());
        Self {
            entries: HashMap::new(),
            capacity,
            entry_ttl,
        }
    }

    fn allow(&mut self, host_id: &str) -> std::result::Result<(), Duration> {
        self.allow_at(host_id, Instant::now())
    }

    fn allow_at(&mut self, host_id: &str, now: Instant) -> std::result::Result<(), Duration> {
        self.entries
            .retain(|_, entry| now.saturating_duration_since(entry.last_seen) < self.entry_ttl);
        if !self.entries.contains_key(host_id) {
            self.make_room(now)?;
            self.entries
                .insert(host_id.to_owned(), TokenBucket::full(now));
        }
        self.entries
            .get_mut(host_id)
            .expect("the report bucket exists")
            .allow_at(now)
    }

    fn make_room(&mut self, now: Instant) -> std::result::Result<(), Duration> {
        if self.entries.len() < self.capacity {
            return Ok(());
        }
        let evictable = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.tokens_at(now) >= REPORT_BUCKET_BURST)
            .min_by_key(|(_, entry)| entry.last_seen)
            .map(|(host_id, _)| host_id.clone());
        if let Some(host_id) = evictable {
            self.entries.remove(&host_id);
            return Ok(());
        }
        let retry = self
            .entries
            .values()
            .map(|entry| {
                Duration::from_secs_f64(
                    (REPORT_BUCKET_BURST - entry.tokens_at(now)).max(0.0)
                        / REPORT_BUCKET_REFILL_PER_SECOND,
                )
            })
            .min()
            .unwrap_or(self.entry_ttl);
        Err(retry.max(Duration::from_nanos(1)))
    }
}

pub fn router(
    state: AppState,
    static_dir: PathBuf,
) -> std::result::Result<Router, sarmg_admin_core::Error> {
    let platform = sarmg_server_runtime::platform_router(
        state.runtime.clone(),
        "host-monitoring",
        state.administrator_origin,
        Arc::clone(&state.administrator),
    )?;
    let console = Router::new()
        .route("/api/v2/monitoring/hosts", get(list_hosts))
        .route("/api/v2/monitoring/hosts/{host_id}", get(host_detail))
        .route(
            "/api/v2/monitoring/hosts/{host_id}/history",
            get(host_history),
        )
        .route(
            "/api/v2/monitoring/client-instances",
            get(list_instances).post(create_instance),
        )
        .route(
            "/api/v2/monitoring/client-instances/{request_id}",
            axum::routing::delete(cancel_instance),
        )
        .route(
            "/api/v2/monitoring/client-instances/{request_id}/authorization",
            axum::routing::put(update_instance_authorization),
        )
        .route(
            "/api/v2/monitoring/managed-instances/{host_id}",
            axum::routing::patch(update_remark).delete(delete_host),
        )
        .route(
            host_protocol::CLIENT_ADMIN_ACTIVATE_PATH,
            post(activate_admin),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            console_admission,
        ));
    let client = Router::new()
        .route(host_protocol::CLIENT_REPORT_PATH, post(report))
        .route(
            host_protocol::CLIENT_CREDENTIAL_STATUS_PATH,
            get(credential_status),
        )
        .route(
            host_protocol::CLIENT_PAIRING_REQUESTS_PATH,
            post(create_pairing),
        )
        .route(
            host_protocol::CLIENT_PAIRING_REQUEST_PATH,
            get(pairing_public),
        )
        .route(
            host_protocol::CLIENT_PAIRING_STATUS_PATH,
            post(pairing_status),
        )
        .route(
            host_protocol::CLIENT_ACTIVATE_PATH,
            post(activate_capability),
        )
        .layer(DefaultBodyLimit::max(CLIENT_REPORT_MAX_BODY_BYTES));
    let product = console.merge(client).with_state(state);
    Ok(Router::new()
        .merge(platform)
        .merge(product)
        .route("/api", any(api_not_found))
        .route("/api/{*path}", any(api_not_found))
        .route_service(
            "/activate/{request_id}",
            axum::routing::get_service(ServeFile::new(static_dir.join("index.html"))),
        )
        .fallback_service(ServeDir::new(static_dir))
        .layer(middleware::from_fn(normalize_api_errors)))
}

async fn api_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

async fn normalize_api_errors(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let is_api = path == "/api" || path.starts_with("/api/");
    let response = next.run(request).await;
    if !is_api
        || !(response.status().is_client_error() || response.status().is_server_error())
        || response
            .extensions()
            .get::<FoundationErrorEnvelope>()
            .is_some()
        || response
            .extensions()
            .get::<sarmg_admin_axum::FoundationErrorResponse>()
            .is_some()
    {
        return response;
    }

    let envelope = framework_envelope(response.status());
    let body = serde_json::to_vec(&envelope)
        .expect("the Foundation error envelope always serializes as JSON");
    let (mut parts, _) = response.into_parts();
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.remove(header::CONTENT_ENCODING);
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    parts.extensions.insert(FoundationErrorEnvelope);
    Response::from_parts(parts, Body::from(body))
}

async fn console_admission(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let identity = match sarmg_admin_axum::authenticate_request(
        &state.administrator,
        request.headers(),
        request.uri(),
        request.method(),
        "host-monitoring",
        state.administrator_origin,
    )
    .await
    {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    let principal = Principal {
        subject: identity.administrator_id.to_string(),
    };
    request.extensions_mut().insert(principal);
    next.run(request).await
}

async fn create_instance(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(request): Json<CreateClientInstanceRequest>,
) -> Result<Response> {
    state
        .pairing_admission
        .check_invite_account(&principal.subject)?;
    let name = request.validated()?;
    let (result, activation_code) =
        store::create_invite(&state.pool, &state.secrets, &name, &principal.subject)
            .await
            .map_err(database)?;
    match result {
        store::CreateInviteResult::Created(summary) => {
            let mut response = (
                StatusCode::CREATED,
                Json(CreatedClientInstance {
                    summary,
                    activation_code: activation_code.expect("created invite has code"),
                }),
            )
                .into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            Ok(response)
        }
        store::CreateInviteResult::Conflict => {
            Err(Error::Conflict("a pending invite already exists".into()))
        }
    }
}

async fn list_instances(
    State(state): State<AppState>,
    Query(query): Query<ClientInstanceListQuery>,
) -> Result<Response> {
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);
    let (instances, hosts, total) = store::list_invites(&state.pool, &state.secrets, limit, offset)
        .await
        .map_err(database)?;
    Ok((
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        Json(ClientInstanceListResponse {
            instances,
            hosts,
            total,
            limit,
            offset,
        }),
    )
        .into_response())
}

async fn update_instance_authorization(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Json(request): Json<UpdateClientAuthorizationRequest>,
) -> Result<Response> {
    let id = canonical_uuid(&id, "client instance request id")?;
    let code = request.validated()?;
    let instance = store::rotate_invite_authorization(
        &state.pool,
        &state.secrets,
        id,
        &code,
        &principal.subject,
    )
    .await
    .map_err(database)?
    .ok_or_else(|| Error::NotFound("client instance invite not found".into()))?;
    Ok((
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        Json(instance),
    )
        .into_response())
}

async fn cancel_instance(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let id = canonical_uuid(&id, "client instance request id")?;
    match store::cancel_invite(&state.pool, id, &principal.subject)
        .await
        .map_err(database)?
    {
        store::CancelInviteResult::Cancelled | store::CancelInviteResult::Deleted => {
            Ok(StatusCode::NO_CONTENT)
        }
        store::CancelInviteResult::NotFound => {
            Err(Error::NotFound("client instance invite not found".into()))
        }
        store::CancelInviteResult::NotPending => Err(Error::Conflict(
            "only a pending invite can be cancelled or a cancelled invite deleted".into(),
        )),
    }
}

async fn create_pairing(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(request): Json<ClientPairingRequest>,
) -> Result<Response> {
    validate_pairing(&request)?;
    state
        .pairing_admission
        .check_create(peer.ip(), &request.host.id)?;
    match store::create_pairing(&state.pool, &request)
        .await
        .map_err(database)?
    {
        store::CreatePairingResult::Ready {
            request_id,
            expires_at,
            created,
        } => {
            let mut response = (
                if created {
                    StatusCode::CREATED
                } else {
                    StatusCode::OK
                },
                Json(ClientPairingResponse {
                    request_id: request_id.to_string(),
                    activation_url: activation_url(request_id),
                    expires_in: (expires_at - Utc::now()).num_seconds().max(1) as u64,
                    poll_interval: 5,
                }),
            )
                .into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            Ok(response)
        }
        store::CreatePairingResult::Expired => {
            Err(Error::BadRequest("pairing request expired".into()))
        }
        store::CreatePairingResult::Conflict => Err(Error::Conflict(
            "polling secret or client token is already in use".into(),
        )),
        store::CreatePairingResult::AtCapacity => Err(Error::RateLimited {
            message: "too many pending pairing requests",
            retry_after: 60,
        }),
        store::CreatePairingResult::DeviceAtCapacity => Err(Error::RateLimited {
            message: "too many pending pairing requests for this device",
            retry_after: 60,
        }),
    }
}

async fn pairing_public(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
) -> Result<Response> {
    let id = canonical_uuid(&id, "pairing request id")?;
    state.pairing_admission.check_poll(peer.ip(), id)?;
    let value = store::pairing_public(&state.pool, id)
        .await
        .map_err(database)?
        .ok_or_else(|| Error::NotFound("pairing request not found".into()))?;
    let mut response = Json(value).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn pairing_status(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response> {
    let id = canonical_uuid(&id, "pairing request id")?;
    state.pairing_admission.check_poll(peer.ip(), id)?;
    let secret = authorization(&headers, "pairing").ok_or(Error::Unauthorized)?;
    if !(32..=256).contains(&secret.len()) || secret.chars().any(char::is_whitespace) {
        return Err(Error::Unauthorized);
    }
    let (status, instance_id) = store::pairing_status(&state.pool, id, &crate::token_hash(secret))
        .await
        .map_err(database)?
        .ok_or(Error::Unauthorized)?;
    let mut response = Json(ClientPairingStatusResponse {
        status,
        instance_id,
    })
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn activate_admin(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(principal): Extension<Principal>,
    Json(request): Json<ActivateClientRequest>,
) -> Result<Response> {
    state
        .pairing_admission
        .check_invite_account(&principal.subject)?;
    activate(&state, peer.ip(), request, &principal.subject).await
}

async fn activate_capability(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(request): Json<ActivateClientRequest>,
) -> Result<Response> {
    activate(&state, peer.ip(), request, "client-capability").await
}

async fn activate(
    state: &AppState,
    source: std::net::IpAddr,
    request: ActivateClientRequest,
    actor: &str,
) -> Result<Response> {
    let id = canonical_uuid(&request.request_id, "pairing request id")?;
    state.pairing_admission.check_activation(source, id)?;
    if request.activation_code.len() > 256
        || request.activation_code.chars().any(char::is_whitespace)
    {
        return Err(Error::Unauthorized);
    }
    match store::activate(
        &state.pool,
        &state.secrets,
        id,
        &crate::token_hash(&request.activation_code),
        actor,
    )
    .await
    .map_err(database)?
    {
        store::ActivateResult::Active(instance) => {
            let mut response = Json(ActivateClientResponse {
                instance_id: instance.to_string(),
                status: ActivatePairingStatus::Active,
            })
            .into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            Ok(response)
        }
        store::ActivateResult::NotFound => Err(Error::NotFound("pairing request not found".into())),
        store::ActivateResult::InvalidCode => Err(Error::Unauthorized),
        store::ActivateResult::Expired => Err(Error::BadRequest(
            "pairing request or activation code expired".into(),
        )),
        store::ActivateResult::Conflict => Err(Error::Conflict(
            "activation code or pairing request already used".into(),
        )),
    }
}

async fn credential_status(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    let credential = authorization(&headers, "bearer").ok_or(Error::Unauthorized)?;
    if !(32..=256).contains(&credential.len()) || credential.chars().any(char::is_whitespace) {
        return Err(Error::Unauthorized);
    }
    let host = store::host_for_token(&state.pool, &crate::token_hash(credential))
        .await
        .map_err(database)?
        .ok_or(Error::Unauthorized)?;
    let mut response = Json(CredentialStatusResponse {
        status: CredentialStatus::Authorized,
        host_id: host.to_string(),
        instance_id: host.to_string(),
        protocol_version: HOST_PAIRING_PROTOCOL_VERSION,
    })
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

fn activation_url(request_id: uuid::Uuid) -> String {
    format!(
        "{}{request_id}",
        host_protocol::BROWSER_ACTIVATION_PATH_PREFIX
    )
}

async fn report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(report): Json<ClientReport>,
) -> Result<Response> {
    let credential = authorization(&headers, "bearer").ok_or(Error::Unauthorized)?;
    let credential_hash = crate::token_hash(credential);
    let host = store::host_for_token(&state.pool, &credential_hash)
        .await
        .map_err(database)?
        .ok_or(Error::Unauthorized)?;
    if host.to_string() != report.host.id {
        return Err(Error::ClientHostMismatch);
    }
    let metrics = validate_report(&report)?;
    let mut buckets = state.report_buckets.lock().await;
    let admission = buckets.allow(&report.host.id);
    drop(buckets);
    if let Err(delay) = admission {
        return Err(Error::RateLimited {
            message: "client report rate exceeded",
            retry_after: delay
                .as_secs()
                .saturating_add(u64::from(delay.subsec_nanos() != 0))
                .max(1),
        });
    }
    let host_id = report.host.id.clone();
    let report_id = report.report_id.clone();
    let result = state
        .telemetry
        .submit(store::ReportWrite::new(report, credential_hash, metrics))
        .await;
    let (accepted, received_at) = match result {
        Ok(value) => value,
        Err(TelemetrySubmitError::QueueFull) => {
            return Err(Error::RateLimited {
                message: "telemetry queue is full",
                retry_after: 1,
            });
        }
        Err(TelemetrySubmitError::WriterUnavailable) => {
            return Err(Error::RetryableUnavailable {
                message: "telemetry writer is unavailable",
                retry_after: 1,
            });
        }
        Err(TelemetrySubmitError::ResponseDeadline) => {
            return Err(Error::RetryableUnavailable {
                message: "telemetry persistence exceeded its response deadline",
                retry_after: 1,
            });
        }
        Err(error) if error.is_unauthorized() => return Err(Error::Unauthorized),
        Err(error) if error.is_report_id_conflict() => {
            return Err(Error::Conflict(
                "report_id already belongs to another host".into(),
            ));
        }
        Err(error) => {
            tracing::warn!(
                %host_id,
                %report_id,
                error = %error,
                "telemetry writer could not persist a validated report"
            );
            return Err(Error::RetryableUnavailable {
                message: "telemetry persistence is unavailable",
                retry_after: 1,
            });
        }
    };
    Ok((
        StatusCode::ACCEPTED,
        Json(ClientReportAck {
            host_id,
            report_id,
            accepted,
            received_at,
        }),
    )
        .into_response())
}

async fn list_hosts(
    State(state): State<AppState>,
    Query(query): Query<HostListQuery>,
) -> Result<Json<HostListResponse>> {
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let offset = query.offset.unwrap_or(0).max(0);
    let (hosts, total) = store::list_hosts(&state.pool, limit, offset)
        .await
        .map_err(database)?;
    Ok(Json(HostListResponse {
        hosts,
        total,
        limit,
        offset,
    }))
}

async fn host_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<HostDetailResponse>> {
    let id = canonical_uuid(&id, "host id")?;
    let (host, latest) = store::get_host(&state.pool, id)
        .await
        .map_err(database)?
        .ok_or_else(|| Error::NotFound("monitored host not found".into()))?;
    Ok(Json(HostDetailResponse { host, latest }))
}

async fn host_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Response> {
    let id = canonical_uuid(&id, "host id")?;
    if query.from.zip(query.to).is_some_and(|(from, to)| from > to) {
        return Err(Error::BadRequest(
            "history from must not be after to".into(),
        ));
    }
    if let Some(resolution) = query.resolution.as_deref() {
        if resolution != "auto" || query.limit.is_some() {
            return Err(Error::BadRequest(
                "chart history requires resolution=auto and max_points instead of limit".into(),
            ));
        }
        let to = query.to.unwrap_or_else(Utc::now);
        let from = query
            .from
            .unwrap_or_else(|| to - chrono::Duration::hours(1));
        if from >= to || to - from > chrono::Duration::days(31) {
            return Err(Error::BadRequest(
                "chart history range must be greater than zero and at most 31 days".into(),
            ));
        }
        let max_points = query.max_points.unwrap_or(720);
        if !(100..=1000).contains(&max_points) {
            return Err(Error::BadRequest(
                "history max_points must be between 100 and 1000".into(),
            ));
        }
        let series = store::history_series(&state.pool, id, from, to, max_points)
            .await
            .map_err(database)?
            .ok_or_else(|| Error::NotFound("monitored host not found".into()))?;
        return Ok(Json(series).into_response());
    }
    if query.max_points.is_some() {
        return Err(Error::BadRequest(
            "history max_points requires resolution=auto".into(),
        ));
    }
    let points = store::history(
        &state.pool,
        id,
        query.from,
        query.to,
        query.limit.unwrap_or(300).clamp(1, 1000),
    )
    .await
    .map_err(database)?
    .ok_or_else(|| Error::NotFound("monitored host not found".into()))?;
    Ok(Json(HistoryResponse {
        host_id: id.to_string(),
        points,
    })
    .into_response())
}

async fn update_remark(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Json(request): Json<UpdateMonitoringRemarkRequest>,
) -> Result<StatusCode> {
    let id = canonical_uuid(&id, "host id")?;
    let remark = request.validated()?;
    if store::update_remark(&state.pool, id, &remark, &principal.subject)
        .await
        .map_err(database)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Error::NotFound("monitored host not found".into()))
    }
}

async fn delete_host(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let id = canonical_uuid(&id, "host id")?;
    if store::delete_host(&state.pool, id, &principal.subject)
        .await
        .map_err(database)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Error::NotFound("monitored host not found".into()))
    }
}

fn authorization<'a>(headers: &'a HeaderMap, expected_scheme: &str) -> Option<&'a str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, value) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case(expected_scheme) && !value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_static_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web")
    }

    async fn app() -> Router {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        store::initialize_empty(&pool).await.unwrap();
        router(
            AppState::new(
                pool,
                AdministratorOriginMode::LoopbackDevelopmentHttp,
                crate::crypto::SecretBox::new([0x42; 32]),
            ),
            test_static_dir(),
        )
        .unwrap()
    }

    async fn app_with_admin(username: &str, password: &str) -> Router {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        store::initialize_empty(&pool).await.unwrap();
        store::ensure_admin_user(&pool, username, Some(password))
            .await
            .unwrap();
        router(
            AppState::new(
                pool,
                AdministratorOriginMode::LoopbackDevelopmentHttp,
                crate::crypto::SecretBox::new([0x42; 32]),
            ),
            test_static_dir(),
        )
        .unwrap()
    }

    async fn app_with_pairing_admission(
        pairing_admission: crate::pairing_admission::PairingAdmission,
    ) -> Router {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        store::initialize_empty(&pool).await.unwrap();
        router(
            AppState::with_pairing_admission(
                pool,
                AdministratorOriginMode::LoopbackDevelopmentHttp,
                pairing_admission,
            ),
            test_static_dir(),
        )
        .unwrap()
    }

    fn login_request(body: impl Into<Body>, peer: &str) -> Request<Body> {
        let mut request = Request::post("/api/v2/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::HOST, "127.0.0.1")
            .header(header::ORIGIN, "http://127.0.0.1")
            .header(sarmg_admin_auth::SEC_FETCH_SITE_HEADER, "same-origin")
            .body(body.into())
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            peer.parse::<SocketAddr>().expect("test peer address"),
        ));
        request
    }

    fn pairing_request(
        host_id: uuid::Uuid,
        peer: &str,
        nonce: char,
        protocol_version: u16,
        client_version: &str,
    ) -> Request<Body> {
        let body = serde_json::json!({
            "protocol_version": protocol_version,
            "host": {
                "id": host_id,
                "os": "linux",
                "os_version": "test",
                "kernel_version": "test",
                "arch": "x86_64",
                "client_version": client_version
            },
            "token_hash": nonce.to_string().repeat(64),
            "polling_secret_hash": if nonce == 'a' { "b".repeat(64) } else { "c".repeat(64) }
        });
        let mut request = Request::post(host_protocol::CLIENT_PAIRING_REQUESTS_PATH)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            peer.parse::<SocketAddr>().expect("test peer address"),
        ));
        request
    }

    async fn body_bytes(response: Response) -> Vec<u8> {
        response
            .into_body()
            .collect()
            .await
            .expect("collect response body")
            .to_bytes()
            .to_vec()
    }

    async fn error_envelope(response: Response) -> sarmg_error::ErrorEnvelope {
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        serde_json::from_slice(&body_bytes(response).await)
            .expect("strict Foundation error envelope")
    }

    #[test]
    fn pairing_activation_url_targets_the_current_application() {
        let request_id = uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        assert_eq!(
            activation_url(request_id),
            "/activate/00000000-0000-4000-8000-000000000001"
        );
    }

    #[test]
    fn report_rate_state_is_bounded_expires_and_does_not_reset_depleted_hosts() {
        let now = Instant::now();
        let mut buckets = ReportBuckets::new(1, Duration::from_secs(10));
        for _ in 0..REPORT_BUCKET_BURST as usize {
            buckets.allow_at("host-a", now).unwrap();
        }
        assert!(buckets.allow_at("host-a", now).is_err());
        assert!(
            buckets.allow_at("host-b", now).is_err(),
            "identifier rotation must not evict a depleted active bucket"
        );
        assert_eq!(buckets.entries.len(), 1);
        assert!(buckets.entries.contains_key("host-a"));

        buckets
            .allow_at("host-b", now + Duration::from_secs(4))
            .unwrap();
        assert_eq!(buckets.entries.len(), 1);
        assert!(buckets.entries.contains_key("host-b"));

        buckets
            .allow_at("host-c", now + Duration::from_secs(15))
            .unwrap();
        assert_eq!(buckets.entries.len(), 1);
        assert!(buckets.entries.contains_key("host-c"));
    }

    #[tokio::test]
    async fn health_is_public_and_console_routes_require_a_session_cookie() {
        assert_eq!(
            app()
                .await
                .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            app()
                .await
                .oneshot(
                    Request::get("/api/v2/monitoring/hosts")
                        .body(Body::empty())
                        .unwrap()
                )
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn pairing_endpoint_uses_protocol_version_instead_of_client_release() {
        let application = app().await;
        let accepted = application
            .clone()
            .oneshot(pairing_request(
                uuid::Uuid::new_v4(),
                "192.0.2.80:41000",
                'a',
                host_protocol::HOST_PAIRING_PROTOCOL_VERSION,
                "0.9.999",
            ))
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::CREATED);

        let rejected = application
            .oneshot(pairing_request(
                uuid::Uuid::new_v4(),
                "192.0.2.81:41000",
                'd',
                2,
                "0.9.999",
            ))
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        let envelope = error_envelope(rejected).await;
        assert_eq!(envelope.code.as_str(), "unsupported_client_protocol");
    }

    #[tokio::test]
    async fn activation_deep_link_serves_the_web_entry_without_exposing_admin_api() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("index.html"),
            "<html>activation app</html>",
        )
        .unwrap();
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        store::initialize_empty(&pool).await.unwrap();
        let app = router(
            AppState::new(
                pool,
                AdministratorOriginMode::LoopbackDevelopmentHttp,
                crate::crypto::SecretBox::new([0x42; 32]),
            ),
            directory.path().to_owned(),
        )
        .unwrap();
        let response = app
            .clone()
            .oneshot(
                Request::get("/activate/00000000-0000-4000-8000-000000000001")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response).await, b"<html>activation app</html>");
        let response = app
            .oneshot(
                Request::get("/api/v2/monitoring/client-instances")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn login_route_is_public() {
        let response = app_with_admin("admin", "correct-password")
            .await
            .oneshot(login_request(
                r#"{"username":"admin","password":"correct-password"}"#,
                "192.0.2.10:41000",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key(header::SET_COOKIE));
    }

    #[tokio::test]
    async fn uri_authority_is_used_only_when_host_is_absent() {
        let application = app_with_admin("admin", "correct-password").await;
        let body = r#"{"username":"admin","password":"wrong-password"}"#;

        let mut authority_only = login_request(body, "192.0.2.11:41000");
        authority_only.headers_mut().remove(header::HOST);
        *authority_only.uri_mut() = "http://127.0.0.1/api/v2/auth/login".parse().unwrap();
        assert_eq!(
            application
                .clone()
                .oneshot(authority_only)
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );

        let mut ambiguous = login_request(body, "192.0.2.12:41000");
        *ambiguous.uri_mut() = "http://127.0.0.1/api/v2/auth/login".parse().unwrap();
        assert_eq!(
            application.oneshot(ambiguous).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn login_json_body_is_bounded_before_password_work() {
        let body = format!(
            r#"{{"username":"admin","password":"{}"}}"#,
            "x".repeat(sarmg_admin_core::ADMIN_BODY_MAX_BYTES)
        );
        let response = app()
            .await
            .oneshot(login_request(body, "192.0.2.20:41000"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn pairing_source_limit_precedes_sqlite_and_returns_retry_after() {
        let admission = crate::pairing_admission::PairingAdmission::for_test(
            1,
            8,
            std::time::Duration::from_secs(300),
        );
        let app = app_with_pairing_admission(admission).await;
        let first = app
            .clone()
            .oneshot(pairing_request(
                uuid::Uuid::new_v4(),
                "192.0.2.50:41000",
                'a',
                host_protocol::HOST_PAIRING_PROTOCOL_VERSION,
                env!("CARGO_PKG_VERSION"),
            ))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::CREATED);

        let limited = app
            .oneshot(pairing_request(
                uuid::Uuid::new_v4(),
                "192.0.2.50:41001",
                'd',
                host_protocol::HOST_PAIRING_PROTOCOL_VERSION,
                env!("CARGO_PKG_VERSION"),
            ))
            .await
            .unwrap();
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(
            limited.headers()[header::RETRY_AFTER]
                .to_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                >= 1
        );
        let envelope = error_envelope(limited).await;
        assert_eq!(envelope.code.as_str(), "too_many_requests");
        assert_eq!(envelope.message, "pairing source rate exceeded");
        assert!(envelope.retryable);
        assert_eq!(envelope.details["retry_after_seconds"], 60);
    }
}
