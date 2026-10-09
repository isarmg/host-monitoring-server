use std::{
    collections::BTreeMap,
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use xcss_config::{ConfigSource, EnvMapping, EnvValueKind, Override};

use crate::retention::{
    DEFAULT_AGGREGATE_RETENTION_DAYS, DEFAULT_MAINTENANCE_INTERVAL_SECONDS,
    DEFAULT_RAW_RETENTION_DAYS, DEFAULT_RETENTION_BATCH_SIZE, DEFAULT_RETENTION_RUN_MILLISECONDS,
    DEFAULT_RETENTION_TRANSACTIONS, DEFAULT_RETENTION_YIELD_MILLISECONDS, RetentionConfig,
};
use crate::telemetry::TelemetryWriterConfig;
use xcss_admin_auth::AdministratorOriginMode;

#[derive(Debug, Parser)]
#[command(name = "xsos", version,
    long_version = concat!(env!("CARGO_PKG_VERSION"), " source=", env!("XSOS_SOURCE_REVISION"), " foundation=", env!("XCSS_FOUNDATION_REVISION")), about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    pub bind: Option<SocketAddr>,
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Explicitly initialize a private data directory and first administrator.
    Init,
    /// Run a previously initialized instance.
    Run {
        #[arg(long)]
        release_root: Option<PathBuf>,
    },
    /// Inspect current configuration and data without changing either.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Query the live service readiness; offline instances never report ready.
    Status,
    /// Run a deployment health check against the configured instance.
    Doctor,
    /// Print the exact machine-readable binary release identity.
    Identity,
    /// Print the immutable browser asset inventory compiled into this binary.
    WebAssets,
    /// Verify an immutable release tree with the binary contained in that tree.
    VerifyRelease(ReleaseRoot),
    /// Reset an existing local administrator password.
    AdminResetPassword(AdminResetPassword),
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Validate,
}

#[derive(Debug, Clone, clap::Args)]
pub struct ReleaseRoot {
    #[arg(long)]
    pub root: PathBuf,
}

#[derive(Debug, Clone, clap::Args)]
pub struct AdminResetPassword {
    #[arg(long)]
    pub database_url: String,
    #[arg(long)]
    pub username: String,
}

#[derive(Clone)]
pub struct ValidatedConfig {
    pub bind: SocketAddr,
    pub database_url: String,
    pub administrator_origin: AdministratorOriginMode,
    pub bootstrap_admin_username: String,
    pub bootstrap_admin_password: Option<String>,
    pub telemetry: TelemetryWriterConfig,
    pub retention: RetentionConfig,
    pub static_dir: Option<PathBuf>,
    pub client_authorization_key: [u8; 32],
    pub data_dir: PathBuf,
    pub sources: BTreeMap<String, ConfigSource>,
}

impl ValidatedConfig {
    pub fn from_runtime() -> anyhow::Result<Self> {
        Self::from_sources(None, None, None)
    }

    pub fn from_sources(
        config: Option<&Path>,
        data_dir: Option<&Path>,
        bind: Option<SocketAddr>,
    ) -> anyhow::Result<Self> {
        let config = config.map(normalize_config_path).transpose()?;
        let file = config
            .as_deref()
            .map(xcss_config::read_private_file)
            .transpose()?;
        let environment = xcss_config::read_environment(&ENVIRONMENT, |name| env::var(name).ok())?;
        let mut command_line = Vec::new();
        if let Some(path) = data_dir {
            command_line.push(Override::new(
                "/data_dir",
                path.to_string_lossy().into_owned(),
            ));
        }
        if let Some(bind) = bind {
            command_line.push(Override::new("/bind", bind.to_string()));
        }
        let loaded = xcss_config::resolve_validated(
            &Settings::default(),
            file.as_deref(),
            &environment,
            &command_line,
            validate_intrinsic,
        )?;
        let settings = loaded.value;
        let database_url = match settings.database_url {
            Some(url) => url,
            None => format!(
                "sqlite://{}",
                settings
                    .data_dir
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("data_dir or database_url is required"))?
                    .join("xsos.sqlite3")
                    .display()
            ),
        };
        let database_path = crate::database_schema::database_path(&database_url)?;
        let data_dir = settings.data_dir.unwrap_or_else(|| {
            database_path
                .parent()
                .expect("database has a parent")
                .to_path_buf()
        });
        anyhow::ensure!(
            data_dir.is_absolute() && database_path.parent() == Some(data_dir.as_path()),
            "database must be a direct child of the absolute data_dir"
        );
        let bind: SocketAddr = settings
            .bind
            .parse()
            .map_err(|_| anyhow::anyhow!("bind must be a socket address"))?;
        let cookie_mode = cookie_mode(bind, settings.development)?;
        let static_dir = development_static_dir(
            settings.development_web_dir.as_deref(),
            settings.development,
        )?;
        let bootstrap_admin_username =
            xcss_admin_auth::normalize_administrator_username(&settings.bootstrap_admin_username)?;
        let telemetry = TelemetryWriterConfig::new(
            settings.telemetry_queue_capacity,
            settings.telemetry_batch_size,
            Duration::from_millis(settings.telemetry_flush_milliseconds),
            Duration::from_millis(settings.telemetry_enqueue_wait_milliseconds),
            Duration::from_millis(settings.telemetry_request_timeout_milliseconds),
            Duration::from_millis(settings.telemetry_shutdown_drain_milliseconds),
        )?;
        let days = |value: u64| {
            value
                .checked_mul(86400)
                .map(Duration::from_secs)
                .ok_or_else(|| anyhow::anyhow!("retention days exceed the supported duration"))
        };
        let retention = RetentionConfig::new(
            days(settings.raw_retention_days)?,
            days(settings.aggregate_retention_days)?,
            Duration::from_secs(settings.retention_interval_seconds),
            settings.retention_batch_size,
            settings.retention_max_transactions_per_run,
            Duration::from_millis(settings.retention_max_run_milliseconds),
            Duration::from_millis(settings.retention_yield_milliseconds),
        )?;
        let client_authorization_key = STANDARD
            .decode(
                settings
                    .client_authorization_key
                    .ok_or_else(|| anyhow::anyhow!("client_authorization_key is required"))?,
            )?
            .try_into()
            .map_err(|_| {
                anyhow::anyhow!("client_authorization_key must decode to exactly 32 bytes")
            })?;
        Ok(Self {
            bind,
            database_url,
            administrator_origin: cookie_mode,
            bootstrap_admin_username,
            bootstrap_admin_password: settings.bootstrap_admin_password,
            telemetry,
            retention,
            static_dir,
            client_authorization_key,
            data_dir,
            sources: loaded.sources,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    database_url: Option<String>,
    data_dir: Option<PathBuf>,
    bind: String,
    development: bool,
    bootstrap_admin_username: String,
    bootstrap_admin_password: Option<String>,
    client_authorization_key: Option<String>,
    development_web_dir: Option<String>,
    telemetry_queue_capacity: usize,
    telemetry_batch_size: usize,
    telemetry_flush_milliseconds: u64,
    telemetry_enqueue_wait_milliseconds: u64,
    telemetry_request_timeout_milliseconds: u64,
    telemetry_shutdown_drain_milliseconds: u64,
    raw_retention_days: u64,
    aggregate_retention_days: u64,
    retention_interval_seconds: u64,
    retention_batch_size: usize,
    retention_max_transactions_per_run: usize,
    retention_max_run_milliseconds: u64,
    retention_yield_milliseconds: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            database_url: None,
            data_dir: None,
            bind: "127.0.0.1:18105".into(),
            development: false,
            bootstrap_admin_username: "admin".into(),
            bootstrap_admin_password: None,
            client_authorization_key: None,
            development_web_dir: None,
            telemetry_queue_capacity: 256,
            telemetry_batch_size: 64,
            telemetry_flush_milliseconds: 25,
            telemetry_enqueue_wait_milliseconds: 10,
            telemetry_request_timeout_milliseconds: 10_000,
            telemetry_shutdown_drain_milliseconds: 15_000,
            raw_retention_days: DEFAULT_RAW_RETENTION_DAYS,
            aggregate_retention_days: DEFAULT_AGGREGATE_RETENTION_DAYS,
            retention_interval_seconds: DEFAULT_MAINTENANCE_INTERVAL_SECONDS,
            retention_batch_size: DEFAULT_RETENTION_BATCH_SIZE,
            retention_max_transactions_per_run: DEFAULT_RETENTION_TRANSACTIONS,
            retention_max_run_milliseconds: DEFAULT_RETENTION_RUN_MILLISECONDS,
            retention_yield_milliseconds: DEFAULT_RETENTION_YIELD_MILLISECONDS,
        }
    }
}

const ENVIRONMENT: [EnvMapping<'static>; 21] = [
    EnvMapping {
        variable: "XSOS_DATABASE_URL",
        path: "/database_url",
        kind: EnvValueKind::String,
    },
    EnvMapping {
        variable: "XSOS_DATA_DIR",
        path: "/data_dir",
        kind: EnvValueKind::String,
    },
    EnvMapping {
        variable: "XSOS_BIND",
        path: "/bind",
        kind: EnvValueKind::String,
    },
    EnvMapping {
        variable: "XSOS_DEVELOPMENT",
        path: "/development",
        kind: EnvValueKind::Boolean,
    },
    EnvMapping {
        variable: "XSOS_BOOTSTRAP_ADMIN_USERNAME",
        path: "/bootstrap_admin_username",
        kind: EnvValueKind::String,
    },
    EnvMapping {
        variable: "XSOS_BOOTSTRAP_ADMIN_PASSWORD",
        path: "/bootstrap_admin_password",
        kind: EnvValueKind::String,
    },
    EnvMapping {
        variable: "XSOC_AUTHORIZATION_KEY",
        path: "/client_authorization_key",
        kind: EnvValueKind::String,
    },
    EnvMapping {
        variable: "XCSS_DEV_WEB_DIR",
        path: "/development_web_dir",
        kind: EnvValueKind::String,
    },
    EnvMapping {
        variable: "XSOS_TELEMETRY_QUEUE_CAPACITY",
        path: "/telemetry_queue_capacity",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_TELEMETRY_BATCH_SIZE",
        path: "/telemetry_batch_size",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_TELEMETRY_FLUSH_MILLISECONDS",
        path: "/telemetry_flush_milliseconds",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_TELEMETRY_ENQUEUE_WAIT_MILLISECONDS",
        path: "/telemetry_enqueue_wait_milliseconds",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_TELEMETRY_REQUEST_TIMEOUT_MILLISECONDS",
        path: "/telemetry_request_timeout_milliseconds",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_TELEMETRY_SHUTDOWN_DRAIN_MILLISECONDS",
        path: "/telemetry_shutdown_drain_milliseconds",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_RAW_RETENTION_DAYS",
        path: "/raw_retention_days",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_AGGREGATE_RETENTION_DAYS",
        path: "/aggregate_retention_days",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_RETENTION_INTERVAL_SECONDS",
        path: "/retention_interval_seconds",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_RETENTION_BATCH_SIZE",
        path: "/retention_batch_size",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_RETENTION_MAX_TRANSACTIONS_PER_RUN",
        path: "/retention_max_transactions_per_run",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_RETENTION_MAX_RUN_MILLISECONDS",
        path: "/retention_max_run_milliseconds",
        kind: EnvValueKind::UnsignedInteger,
    },
    EnvMapping {
        variable: "XSOS_RETENTION_YIELD_MILLISECONDS",
        path: "/retention_yield_milliseconds",
        kind: EnvValueKind::UnsignedInteger,
    },
];

fn development_static_dir(
    value: Option<&str>,
    development: bool,
) -> anyhow::Result<Option<PathBuf>> {
    let Some(value) = value else { return Ok(None) };
    anyhow::ensure!(
        development && crate::release_contract::SOURCE_REVISION == "unbound",
        "directory Web assets are allowed only in an unbound development build"
    );
    Ok(Some(validate_static_dir(value, false)?))
}

fn validate_static_dir(value: &str, production: bool) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !production,
        "production Web assets are embedded in the binary"
    );
    let root = Path::new(value);
    anyhow::ensure!(
        root.is_absolute(),
        "development Web directory must be absolute"
    );
    xcss_web_assets::DirectoryAssets::new(root)?;
    anyhow::ensure!(
        root.join("index.html").is_file() && root.join("assets").is_dir(),
        "development Web directory must contain index.html and assets"
    );
    let mut entries = fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    anyhow::ensure!(
        entries
            == [
                std::ffi::OsString::from("assets"),
                std::ffi::OsString::from("index.html")
            ],
        "development Web directory must contain only the declared browser entry and assets"
    );
    Ok(fs::canonicalize(root)?)
}

fn cookie_mode(bind: SocketAddr, development: bool) -> anyhow::Result<AdministratorOriginMode> {
    if development && !bind.ip().is_loopback() {
        anyhow::bail!("XSOS_DEVELOPMENT requires a loopback XSOS_BIND");
    }
    Ok(if development {
        AdministratorOriginMode::LoopbackDevelopmentHttp
    } else {
        AdministratorOriginMode::ProductionHttps
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_assets_need_no_external_directory_and_production_rejects_overrides() {
        assert!(development_static_dir(None, false).unwrap().is_none());
        assert!(development_static_dir(Some("/not-needed"), false).is_err());
    }

    #[test]
    fn insecure_development_cookie_mode_is_explicit_and_loopback_only() {
        assert_eq!(
            cookie_mode("127.0.0.1:18105".parse().unwrap(), false).unwrap(),
            AdministratorOriginMode::ProductionHttps
        );
        assert_eq!(
            cookie_mode("127.0.0.1:18105".parse().unwrap(), true).unwrap(),
            AdministratorOriginMode::LoopbackDevelopmentHttp
        );
        assert!(cookie_mode("0.0.0.0:18105".parse().unwrap(), true).is_err());
    }

    #[test]
    fn product_cli_rejects_unknown_maintenance_commands() {
        for removed in ["migrate", "backup-create", "backup-verify", "restore"] {
            assert!(
                Cli::try_parse_from(["xsos", removed]).is_err(),
                "removed product command {removed} was still accepted"
            );
        }
    }

    #[test]
    fn administrator_username_uses_the_foundation_canonical_form() {
        assert_eq!(
            xcss_admin_auth::normalize_administrator_username("  Release.Admin  ").unwrap(),
            "release.admin"
        );
        for rejected in ["ab", "admin@example.test", "管理员", "admin\n"] {
            assert!(xcss_admin_auth::normalize_administrator_username(rejected).is_err());
        }
    }

    #[test]
    fn immutable_release_commands_require_an_explicit_root() {
        let root = "/opt/isarmg/xsos/releases/0.12.0";
        assert!(Cli::try_parse_from(["xsos", "run", "--release-root", root]).is_ok());
        assert!(Cli::try_parse_from(["xsos", "verify-release", "--root", root]).is_ok());
        assert!(Cli::try_parse_from(["xsos", "run", "--release-root"]).is_err());
        assert!(Cli::try_parse_from(["xsos", "verify-release"]).is_err());
    }

    #[test]
    fn static_directory_is_absolute_and_exact() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("index.html"), "current").unwrap();
        fs::create_dir(directory.path().join("assets")).unwrap();
        fs::write(directory.path().join("assets/app.js"), "current").unwrap();

        assert_eq!(
            validate_static_dir(directory.path().to_str().unwrap(), false).unwrap(),
            directory.path().canonicalize().unwrap()
        );
        assert!(validate_static_dir(directory.path().to_str().unwrap(), true).is_err());
        assert!(validate_static_dir("web/dist", false).is_err());
        fs::write(directory.path().join("unexpected"), "not current").unwrap();
        assert!(validate_static_dir(directory.path().to_str().unwrap(), false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn static_directory_rejects_symbolic_and_hard_linked_assets() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let index = directory.path().join("index.html");
        fs::write(&index, "current").unwrap();
        fs::create_dir(directory.path().join("assets")).unwrap();
        fs::write(directory.path().join("assets/app.js"), "current").unwrap();
        let alias = directory.path().join("alias.html");
        fs::hard_link(&index, &alias).unwrap();
        assert!(validate_static_dir(directory.path().to_str().unwrap(), false).is_err());

        fs::remove_file(&alias).unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), directory.path().join("linked.js")).unwrap();
        assert!(validate_static_dir(directory.path().to_str().unwrap(), false).is_err());
    }
}

fn validate_intrinsic(
    settings: &Settings,
    source: ConfigSource,
) -> Result<(), xcss_config::ConfigError> {
    let invalid =
        |path| xcss_config::ConfigError::new(xcss_config::Reason::InvalidValue, path, source);
    settings
        .bind
        .parse::<SocketAddr>()
        .map_err(|_| invalid("/bind"))?;
    if let Some(key) = &settings.client_authorization_key
        && STANDARD
            .decode(key)
            .map_err(|_| invalid("/client_authorization_key"))?
            .len()
            != 32
    {
        return Err(invalid("/client_authorization_key"));
    }
    xcss_admin_auth::normalize_administrator_username(&settings.bootstrap_admin_username)
        .map_err(|_| invalid("/bootstrap_admin_username"))?;
    if let Some(password) = &settings.bootstrap_admin_password {
        xcss_admin_auth::validate_password(password)
            .map_err(|_| invalid("/bootstrap_admin_password"))?;
    }
    if settings
        .data_dir
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err(invalid("/data_dir"));
    }
    if let Some(url) = &settings.database_url {
        crate::database_schema::database_path(url).map_err(|_| invalid("/database_url"))?;
    }
    for (path, value, min, max) in [
        (
            "/telemetry_queue_capacity",
            settings.telemetry_queue_capacity as u64,
            1,
            crate::telemetry::MAX_QUEUE_CAPACITY as u64,
        ),
        (
            "/telemetry_batch_size",
            settings.telemetry_batch_size as u64,
            1,
            crate::telemetry::MAX_BATCH_SIZE as u64,
        ),
        (
            "/retention_batch_size",
            settings.retention_batch_size as u64,
            1,
            crate::retention::MAX_RETENTION_BATCH_SIZE as u64,
        ),
        (
            "/retention_max_transactions_per_run",
            settings.retention_max_transactions_per_run as u64,
            3,
            crate::retention::MAX_RETENTION_TRANSACTIONS as u64,
        ),
        (
            "/telemetry_flush_milliseconds",
            settings.telemetry_flush_milliseconds,
            1,
            1000,
        ),
        (
            "/telemetry_enqueue_wait_milliseconds",
            settings.telemetry_enqueue_wait_milliseconds,
            1,
            250,
        ),
        (
            "/telemetry_request_timeout_milliseconds",
            settings.telemetry_request_timeout_milliseconds,
            100,
            30000,
        ),
        (
            "/telemetry_shutdown_drain_milliseconds",
            settings.telemetry_shutdown_drain_milliseconds,
            100,
            60000,
        ),
        ("/raw_retention_days", settings.raw_retention_days, 1, 365),
        (
            "/aggregate_retention_days",
            settings.aggregate_retention_days,
            1,
            3650,
        ),
        (
            "/retention_interval_seconds",
            settings.retention_interval_seconds,
            1,
            86400,
        ),
        (
            "/retention_max_run_milliseconds",
            settings.retention_max_run_milliseconds,
            100,
            10000,
        ),
        (
            "/retention_yield_milliseconds",
            settings.retention_yield_milliseconds,
            1,
            100,
        ),
    ] {
        if !(min..=max).contains(&value) {
            return Err(invalid(path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod precedence_contract_tests {
    use super::*;
    #[test]
    fn a_higher_priority_override_cannot_hide_an_invalid_file_value() {
        let file = serde_json::to_vec(&serde_json::json!({"telemetry_queue_capacity":0})).unwrap();
        let error = xcss_config::resolve_validated(
            &Settings::default(),
            Some(&file),
            &[],
            &[Override::new(
                "/telemetry_queue_capacity",
                serde_json::json!(16),
            )],
            validate_intrinsic,
        )
        .err()
        .expect("invalid lower layer must fail");
        assert_eq!(error.path, "/telemetry_queue_capacity");
        assert_eq!(error.source, ConfigSource::File);
        assert!(
            !serde_json::to_string(&error.envelope())
                .unwrap()
                .contains("private-invalid-secret")
        );
    }
}

/// Normalize the CLI file authority before private reads and resource reporting.
pub fn normalize_config_path(path: &Path) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !path
            .components()
            .any(|part| part == std::path::Component::ParentDir),
        "config path cannot contain parent traversal"
    );
    Ok(std::path::absolute(path)?)
}
