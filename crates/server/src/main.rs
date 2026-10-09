use anyhow::Context as _;
use clap::Parser;
use xsos::{
    config::{Cli, Command, ConfigCommand},
    database_lock::{ApplicationLock, MaintenanceLock},
    http::{AppState, product_descriptor, router},
    release_bundle, release_contract,
    retention::RetentionMaintenance,
    store,
    telemetry::TelemetryWriter,
};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let json = std::env::args_os().any(|argument| argument == "--json");
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            return if error.print().is_ok() {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            };
        }
        Err(_) => {
            let error = xcss_server_cli::ErrorEnvelope::with_code(
                xcss_server_cli::ErrorCode::new("invalid_cli_input").unwrap(),
                "Command arguments do not satisfy the current CLI contract; use --help.",
            );
            return xcss_server_cli::report_error(&error, json, 2);
        }
    };
    match execute(cli).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let envelope = if let Some(error) = error.downcast_ref::<xcss_config::ConfigError>() {
                error.envelope()
            } else if let Some(error) = error.downcast_ref::<xcss_server_cli::CliError>() {
                error.0.clone()
            } else if let Some(error) = error.downcast_ref::<xcss_state_file::Error>() {
                xcss_server_cli::state_error(error)
            } else if let Some(error) = error.downcast_ref::<xcss_server_cli::SnapshotError>() {
                xcss_server_cli::snapshot_error(error)
            } else {
                if !json {
                    eprintln!("{error:#}");
                }
                xcss_server_cli::ErrorEnvelope::with_code(
                    xcss_server_cli::ErrorCode::new("current_state_invalid").unwrap(),
                    "The command could not validate or operate on the current configuration and data.",
                )
            };
            xcss_server_cli::report_error(&envelope, json, 1)
        }
    }
}

async fn query_status(bind: std::net::SocketAddr, json: bool) -> anyhow::Result<()> {
    let report = xcss_server_cli::query_status(bind, "xsos")
        .await
        .map_err(xcss_server_cli::CliError)?;
    if !report.ready {
        return Err(
            xcss_server_cli::CliError(xcss_server_cli::ErrorEnvelope::with_code(
                xcss_server_cli::ErrorCode::new("service_not_ready").unwrap(),
                "The service answered but its business readiness checks failed.",
            ))
            .into(),
        );
    }
    xcss_server_cli::print_report(&report, json)?;
    Ok(())
}

async fn execute(mut cli: Cli) -> anyhow::Result<()> {
    cli.config = cli
        .config
        .as_deref()
        .map(xsos::config::normalize_config_path)
        .transpose()?;
    xsos::release_contract::ensure_supported_runtime()?;
    xcss_server_runtime::install_panic_hook();
    initialize_logging()?;
    let configuration = || {
        xsos::config::ValidatedConfig::from_sources(
            cli.config.as_deref(),
            cli.data_dir.as_deref(),
            cli.bind,
        )
    };
    match cli.command {
        Command::Init => {
            let config = configuration()?;
            let password = config
                .bootstrap_admin_password
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("bootstrap_admin_password is required for init"))?;
            xcss_admin_auth::validate_password(password)?;
            xcss_server_cli::runtime_allowed(&config.data_dir)
                .map_err(xcss_server_cli::CliError)?;
            xcss_server_cli::create_empty_private_directory(&config.data_dir)
                .map_err(xcss_server_cli::CliError)?;
            let directory = xcss_state_file::PrivateStateDirectory::open(&config.data_dir)?;
            xcss_server_cli::runtime_allowed(directory.path())
                .map_err(xcss_server_cli::CliError)?;
            let _maintenance = directory.try_maintenance_lock()?;
            xcss_server_cli::runtime_allowed(directory.path())
                .map_err(xcss_server_cli::CliError)?;
            xcss_server_cli::create_runtime_log_directory(&config.data_dir)
                .map_err(xcss_server_cli::CliError)?;
            let database = xsos::database_schema::database_path(&config.database_url)?;
            anyhow::ensure!(
                !database.try_exists()?,
                "database already exists; init never overwrites existing data"
            );
            let pool = store::open_or_initialize(&config.database_url).await?;
            store::ensure_admin_user(&pool, &config.bootstrap_admin_username, Some(password))
                .await?;
            xcss_sqlite::checkpoint(&pool).await?;
            pool.close().await;
            tracing::info!(event = "common.initialization.completed");
            println!(
                "{}",
                serde_json::json!({"status":"initialized", "ready":false})
            );
        }
        Command::Run { release_root } => {
            let release_root = release_root
                .as_deref()
                .map(release_bundle::resolve_run_root)
                .transpose()?;
            if let Some(root) = &release_root {
                release_bundle::verify_release(root)?;
            } else {
                anyhow::ensure!(
                    !release_contract::BinaryIdentity::current()?.is_release_bound(),
                    "source-bound release binaries require run --release-root"
                );
            }
            serve_with_config(configuration()?, release_root.as_deref()).await?;
        }
        Command::Config {
            command: ConfigCommand::Validate,
        } => {
            let config = configuration()?;
            validate_existing_configuration(&config).await?;
            println!(
                "{}",
                serde_json::json!({"status":"valid", "state_paths":std::iter::once(config.data_dir.clone()).chain(cli.config.as_ref().map(|path| path.canonicalize()).transpose()?).collect::<Vec<_>>(), "sources":config.sources, "schema_identity":xsos::database_schema::expected_identity()?})
            );
        }
        Command::Status => {
            query_status(configuration()?.bind, cli.json).await?;
        }
        Command::AdminResetPassword(args) => {
            let database = xsos::database_schema::database_path(&args.database_url)?;
            let directory =
                xcss_state_file::PrivateStateDirectory::open(database.parent().unwrap())?;
            xcss_server_cli::runtime_allowed(directory.path())
                .map_err(xcss_server_cli::CliError)?;
            let _common_lock = directory.try_maintenance_lock()?;
            xcss_server_cli::runtime_allowed(directory.path())
                .map_err(xcss_server_cli::CliError)?;
            xsos::database_schema::validate_configuration_database(&args.database_url)?;
            let maintenance = MaintenanceLock::exclusive(&args.database_url)?;
            let pool =
                xsos::database_schema::open_validated_location(&maintenance.database_url()).await?;
            let username = store::normalize_username(&args.username)?;
            let password = read_password_from_stdin()?;
            store::reset_admin_password(&pool, &username, &password).await?;
            println!(
                "{{\"status\":\"password-reset\",\"username\":{:?}}}",
                username
            );
        }
        Command::Doctor => {
            let config = configuration()?;
            let directory = xcss_state_file::PrivateStateDirectory::open(&config.data_dir)?;
            xcss_server_cli::runtime_allowed(directory.path())
                .map_err(xcss_server_cli::CliError)?;
            let _common_lock = directory.try_maintenance_lock()?;
            xcss_server_cli::runtime_allowed(directory.path())
                .map_err(xcss_server_cli::CliError)?;
            xsos::database_schema::validate_configuration_database(&config.database_url)?;
            let maintenance = MaintenanceLock::exclusive(&config.database_url)?;
            let pool =
                xsos::database_schema::open_validated_location(&maintenance.database_url()).await?;
            let database_ready = store::ready(&pool).await;
            let retention_ready = store::retention_ready(&pool).await;
            let integrity_ready = match xcss_sqlite::integrity_check(&pool).await {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(%error, "xsos doctor integrity check failed");
                    false
                }
            };
            let foreign_keys_ready = match xcss_sqlite::foreign_key_check(&pool).await {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(%error, "xsos doctor foreign-key check failed");
                    false
                }
            };
            let secrets = xsos::crypto::SecretBox::new(config.client_authorization_key);
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
        Command::WebAssets => {
            xsos::web_assets::verify()?;
            println!("{}", xsos::web_assets::MANIFEST);
        }
        Command::Identity => {
            println!("{}", xsos::release_contract::current_json()?);
        }
        Command::VerifyRelease(args) => {
            let report = release_bundle::verify_release(&args.root)?;
            println!("{}", serde_json::to_string(&report)?);
        }
    }
    Ok(())
}

async fn serve_with_config(
    config: xsos::config::ValidatedConfig,
    release_root: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    if release_root.is_some() {
        anyhow::ensure!(
            config.static_dir.is_none(),
            "formal releases require embedded Web assets"
        );
    }
    if config.administrator_origin
        == xcss_admin_auth::AdministratorOriginMode::LoopbackDevelopmentHttp
    {
        tracing::warn!(
            "XSOS_DEVELOPMENT is enabled; using an insecure loopback-only session cookie"
        );
    }
    let signals = xcss_server_runtime::ProcessSignals::install()?;
    let listeners = xcss_server_runtime::BoundListeners::bind([config.bind])?;
    let transport = xcss_server_runtime::HttpServer::new(listeners, signals);
    xcss_server_cli::runtime_allowed(&config.data_dir).map_err(xcss_server_cli::CliError)?;
    validate_existing_configuration(&config).await?;
    let directory = xcss_state_file::PrivateStateDirectory::open(&config.data_dir)?;
    xcss_server_cli::runtime_allowed(directory.path()).map_err(xcss_server_cli::CliError)?;
    let _common_lock = directory.try_instance_lock()?;
    xcss_server_cli::runtime_allowed(directory.path()).map_err(xcss_server_cli::CliError)?;
    let application_lock = ApplicationLock::acquire(&config.database_url)?;
    validate_existing_configuration(&config).await?;
    enable_runtime_logging(&config.data_dir)?;
    tracing::info!(event = "common.config.loaded");
    xsos::database_schema::validate_configuration_database(&config.database_url)?;
    let pool =
        xsos::database_schema::open_validated_location(&application_lock.database_url()).await?;
    store::require_administrator(&pool).await?;
    let secrets = xsos::crypto::SecretBox::new(config.client_authorization_key);
    store::validate_invite_authorizations(&pool, &secrets).await?;
    let (retention_status, retention_maintenance) =
        RetentionMaintenance::start(pool.clone(), config.retention);
    let (telemetry, telemetry_writer) = TelemetryWriter::start(pool.clone(), config.telemetry);
    let health_pool = pool.clone();
    let retention_pool = pool.clone();
    let runtime = xcss_server_runtime::ServerRuntime::builder(product_descriptor())
        .with_schema_identity(xsos::database_schema::expected_identity()?)
        .register_health_check(
            "database",
            xcss_server_runtime::health_check(move || {
                let pool = health_pool.clone();
                async move { store::ready(&pool).await }
            }),
        )
        .register_health_check(
            "retention-schema",
            xcss_server_runtime::health_check(move || {
                let pool = retention_pool.clone();
                async move { store::retention_ready(&pool).await }
            }),
        )
        .register_health_check(
            "retention-worker",
            xcss_server_runtime::health_check(move || {
                let status = retention_status.clone();
                async move { status.is_healthy() }
            }),
        )
        .register_background_task(
            "telemetry-writer",
            xcss_server_runtime::TaskCriticality::Critical,
            move |shutdown| telemetry_writer.run_until(shutdown),
        )
        .register_background_task(
            "telemetry-retention",
            xcss_server_runtime::TaskCriticality::Degrading,
            move |shutdown| retention_maintenance.run_until(shutdown),
        )
        .register_background_task(
            "shutdown-log",
            xcss_server_runtime::TaskCriticality::Degrading,
            |mut shutdown| async move {
                if !*shutdown.borrow() {
                    let _ = shutdown.changed().await;
                }
                tracing::info!(event = "common.runtime.shutdown_started");
                Ok(())
            },
        )
        .build()
        .await?;
    let runtime_handle = runtime.handle();
    tracing::info!(event = "common.runtime.started");
    let state = AppState::with_runtime(
        pool,
        config.administrator_origin,
        telemetry,
        runtime_handle.clone(),
        secrets,
    );
    tracing::info!(bind=%config.bind, "xsos server ready");
    runtime
        .serve(
            transport,
            router(state, config.static_dir)?.layer(axum::middleware::from_fn_with_state(
                "xsos".to_owned(),
                xcss_server_cli::service_identity_middleware,
            )),
        )
        .await?;
    tracing::info!(event = "common.runtime.stopped");
    Ok(())
}

fn read_password_from_stdin() -> anyhow::Result<String> {
    use std::io::Read as _;

    let mut bytes = Vec::new();
    std::io::stdin()
        .take(4097)
        .read_to_end(&mut bytes)
        .context("read administrator password from standard input")?;
    anyhow::ensure!(
        bytes.len() <= 4096,
        "administrator password input is too large"
    );
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    anyhow::ensure!(
        !bytes.contains(&b'\n') && !bytes.contains(&b'\r'),
        "administrator password must be one line"
    );
    String::from_utf8(bytes).context("administrator password must be UTF-8")
}

static LOG_LAYER: std::sync::OnceLock<xcss_log::FoundationStructuredLayer> =
    std::sync::OnceLock::new();
fn initialize_logging() -> anyhow::Result<()> {
    use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};
    let layer = xcss_log::FoundationStructuredLayer::new("xsos")?;
    LOG_LAYER
        .set(layer.clone())
        .map_err(|_| anyhow::anyhow!("logging already initialized"))?;
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(layer)
        .try_init()?;
    Ok(())
}
fn enable_runtime_logging(data_dir: &std::path::Path) -> anyhow::Result<()> {
    xcss_server_cli::validate_runtime_log_directory(data_dir).map_err(xcss_server_cli::CliError)?;
    let file = xcss_log::RotatingLogFile::open(
        data_dir.join("logs"),
        "xsos",
        xcss_log::LogRetention::default(),
    )?;
    LOG_LAYER
        .get()
        .ok_or_else(|| anyhow::anyhow!("logging is unavailable"))?
        .set_rotating_file(file)?;
    Ok(())
}

async fn validate_existing_configuration(
    config: &xsos::config::ValidatedConfig,
) -> anyhow::Result<()> {
    xcss_server_cli::validate_runtime_log_directory(&config.data_dir)
        .map_err(xcss_server_cli::CliError)?;
    xcss_state_file::PrivateStateDirectory::open(&config.data_dir)?;
    let snapshot = xsos::database_schema::validation_snapshot(
        xsos::database_schema::database_path(&config.database_url)?,
    )
    .await?;
    xsos::database_schema::validate_pool(snapshot.pool()).await?;
    xcss_sqlite::integrity_check(snapshot.pool()).await?;
    xcss_sqlite::foreign_key_check(snapshot.pool()).await?;
    store::require_administrator(snapshot.pool()).await?;
    store::validate_invite_authorizations(
        snapshot.pool(),
        &xsos::crypto::SecretBox::new(config.client_authorization_key),
    )
    .await?;
    snapshot.close().await;
    Ok(())
}
