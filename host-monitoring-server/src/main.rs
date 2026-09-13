use clap::Parser;
use host_monitoring_server::{
    config::{Cli, Command},
    database_lock::{ApplicationLock, MaintenanceLock},
    http::{AppState, product_descriptor, router},
    release_bundle, release_contract,
    retention::RetentionMaintenance,
    store,
    telemetry::TelemetryWriter,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    host_monitoring_server::release_contract::ensure_supported_runtime()?;
    sarmg_server_runtime::install_panic_hook();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();
    match Cli::parse().command {
        Command::Serve => {
            anyhow::ensure!(
                !release_contract::BinaryIdentity::current()?.is_release_bound(),
                "source-bound release binaries must use serve-release"
            );
            serve(None).await?;
        }
        Command::ServeRelease(args) => {
            release_bundle::verify_release(&args.root)?;
            serve(Some(&args.root)).await?;
        }
        Command::AdminCreate(database) => {
            let maintenance = MaintenanceLock::exclusive(&database.database_url)?;
            let pool = store::open_or_initialize(&maintenance.database_url()).await?;
            let username = std::env::var("HOST_MONITORING_BOOTSTRAP_ADMIN_USERNAME")
                .unwrap_or_else(|_| "admin".to_string());
            let username = store::normalize_username(&username)?;
            let password = std::env::var("HOST_MONITORING_BOOTSTRAP_ADMIN_PASSWORD").ok();
            store::ensure_admin_user(&pool, &username, password.as_deref()).await?;
            println!("{{\"status\":\"admin-ready\",\"username\":{username:?}}}");
        }
        Command::AdminResetPassword(args) => {
            let maintenance = MaintenanceLock::exclusive(&args.database_url)?;
            let pool = store::open_existing(&maintenance.database_url()).await?;
            let username = store::normalize_username(&args.username)?;
            store::reset_admin_password(&pool, &username, &args.password).await?;
            println!(
                "{{\"status\":\"password-reset\",\"username\":{:?}}}",
                username
            );
        }
        Command::Doctor => {
            let config = host_monitoring_server::config::ValidatedConfig::from_runtime()?;
            let maintenance = MaintenanceLock::shared(&config.database_url)?;
            let pool = store::open_existing(&maintenance.database_url()).await?;
            let database_ready = store::ready(&pool).await;
            let retention_ready = store::retention_ready(&pool).await;
            let integrity_ready = match sarmg_sqlite::integrity_check(&pool).await {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(%error, "Host Monitoring doctor integrity check failed");
                    false
                }
            };
            let foreign_keys_ready = match sarmg_sqlite::foreign_key_check(&pool).await {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(%error, "Host Monitoring doctor foreign-key check failed");
                    false
                }
            };
            let secrets =
                host_monitoring_server::crypto::SecretBox::new(config.client_authorization_key);
            let encrypted_values_ready = store::validate_invite_authorizations(&pool, &secrets)
                .await
                .is_ok();
            println!(
                "{{\"status\":\"{}\",\"bind\":\"{}\",\"database_ready\":{database_ready},\"retention_ready\":{retention_ready},\"integrity_ready\":{integrity_ready},\"foreign_keys_ready\":{foreign_keys_ready},\"encrypted_values_ready\":{encrypted_values_ready}}}",
                if database_ready
                    && retention_ready
                    && integrity_ready
                    && foreign_keys_ready
                    && encrypted_values_ready
                {
                    "ok"
                } else {
                    "degraded"
                },
                config.bind
            );
            if !database_ready
                || !retention_ready
                || !integrity_ready
                || !foreign_keys_ready
                || !encrypted_values_ready
            {
                anyhow::bail!("database is not ready");
            }
        }
        Command::Identity => {
            println!(
                "{}",
                host_monitoring_server::release_contract::current_json()?
            );
        }
        Command::VerifyRelease(args) => {
            let report = release_bundle::verify_release(&args.root)?;
            println!("{}", serde_json::to_string(&report)?);
        }
    }
    Ok(())
}

async fn serve(release_root: Option<&std::path::Path>) -> anyhow::Result<()> {
    let config = host_monitoring_server::config::ValidatedConfig::from_runtime()?;
    if let Some(root) = release_root {
        anyhow::ensure!(
            config.static_dir == root.join("web"),
            "HOST_MONITORING_STATIC_DIR must equal the verified release web directory"
        );
    }
    if config.administrator_origin
        == sarmg_admin_auth::AdministratorOriginMode::LoopbackDevelopmentHttp
    {
        tracing::warn!(
            "HOST_MONITORING_DEVELOPMENT is enabled; using an insecure loopback-only session cookie"
        );
    }
    let signals = sarmg_server_runtime::ProcessSignals::install()?;
    let listeners = sarmg_server_runtime::BoundListeners::bind([config.bind])?;
    let transport = sarmg_server_runtime::HttpServer::new(listeners, signals);
    let application_lock = ApplicationLock::acquire(&config.database_url)?;
    let pool = store::open_or_initialize(&application_lock.database_url()).await?;
    store::ensure_admin_user(
        &pool,
        &config.bootstrap_admin_username,
        config.bootstrap_admin_password.as_deref(),
    )
    .await?;
    let secrets = host_monitoring_server::crypto::SecretBox::new(config.client_authorization_key);
    store::validate_invite_authorizations(&pool, &secrets).await?;
    let (_, retention_maintenance) = RetentionMaintenance::start(pool.clone(), config.retention);
    let (telemetry, telemetry_writer) = TelemetryWriter::start(pool.clone(), config.telemetry);
    let health_pool = pool.clone();
    let retention_pool = pool.clone();
    let runtime = sarmg_server_runtime::ServerRuntime::builder(product_descriptor())
        .with_schema_identity(host_monitoring_server::database_schema::expected_identity()?)
        .register_health_check(
            "database",
            sarmg_server_runtime::health_check(move || {
                let pool = health_pool.clone();
                async move { store::ready(&pool).await }
            }),
        )
        .register_health_check(
            "retention-schema",
            sarmg_server_runtime::health_check(move || {
                let pool = retention_pool.clone();
                async move { store::retention_ready(&pool).await }
            }),
        )
        .register_background_task(
            "telemetry-writer",
            sarmg_server_runtime::TaskCriticality::Critical,
            move |shutdown| telemetry_writer.run_until(shutdown),
        )
        .register_background_task(
            "telemetry-retention",
            sarmg_server_runtime::TaskCriticality::Degrading,
            move |shutdown| retention_maintenance.run_until(shutdown),
        )
        .build()
        .await?;
    let runtime_handle = runtime.handle();
    let state = AppState::with_runtime(
        pool,
        config.administrator_origin,
        telemetry,
        runtime_handle.clone(),
    )
    .with_secrets(secrets);
    tracing::info!(bind=%config.bind, "host-monitoring server ready");
    runtime
        .serve(transport, router(state, config.static_dir)?)
        .await?;
    Ok(())
}
