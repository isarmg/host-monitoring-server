use std::{
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use clap::{Parser, Subcommand};

use crate::retention::{
    DEFAULT_AGGREGATE_RETENTION_DAYS, DEFAULT_MAINTENANCE_INTERVAL_SECONDS,
    DEFAULT_RAW_RETENTION_DAYS, DEFAULT_RETENTION_BATCH_SIZE, DEFAULT_RETENTION_RUN_MILLISECONDS,
    DEFAULT_RETENTION_TRANSACTIONS, DEFAULT_RETENTION_YIELD_MILLISECONDS, RetentionConfig,
};
use crate::telemetry::TelemetryWriterConfig;
use sarmg_admin_auth::AdministratorOriginMode;

#[derive(Debug, Parser)]
#[command(name = "host-monitoring-server", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Serve an unbound development build; source-bound release binaries reject this command.
    Serve,
    /// Verify and serve the exact immutable release tree for this binary version.
    ServeRelease(ReleaseRoot),
    /// Run a deployment health check against the configured instance.
    Doctor,
    /// Print the exact machine-readable binary release identity.
    Identity,
    /// Verify an immutable release tree with the binary contained in that tree.
    VerifyRelease(ReleaseRoot),
    /// Create the initial local administrator in the configured database.
    AdminCreate(Database),
    /// Reset an existing local administrator password.
    AdminResetPassword(AdminResetPassword),
}

#[derive(Debug, Clone, clap::Args)]
pub struct ReleaseRoot {
    #[arg(long)]
    pub root: PathBuf,
}

#[derive(Debug, Clone, clap::Args)]
pub struct Database {
    #[arg(long)]
    pub database_url: String,
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
    pub static_dir: PathBuf,
    pub client_authorization_key: [u8; 32],
}

impl ValidatedConfig {
    pub fn from_runtime() -> anyhow::Result<Self> {
        let database_url = required("HOST_MONITORING_DATABASE_URL")?;
        if !database_url.starts_with("sqlite:") && !database_url.starts_with("sqlite://") {
            anyhow::bail!("HOST_MONITORING_DATABASE_URL must be a SQLite URL");
        }
        let bind: SocketAddr = value("HOST_MONITORING_BIND", "127.0.0.1:18105")
            .parse()
            .map_err(|_| anyhow::anyhow!("HOST_MONITORING_BIND must be a socket address"))?;
        let development = parse_bool("HOST_MONITORING_DEVELOPMENT", false)?;
        let cookie_mode = cookie_mode(bind, development)?;
        let static_dir =
            validate_static_dir(&required("HOST_MONITORING_STATIC_DIR")?, !development)?;
        let bootstrap_admin_username = sarmg_admin_auth::normalize_administrator_username(&value(
            "HOST_MONITORING_BOOTSTRAP_ADMIN_USERNAME",
            "admin",
        ))
        .map_err(|error| {
            anyhow::anyhow!("HOST_MONITORING_BOOTSTRAP_ADMIN_USERNAME is invalid: {error}")
        })?;
        let telemetry = TelemetryWriterConfig::new(
            parse_usize("HOST_MONITORING_TELEMETRY_QUEUE_CAPACITY", 256)?,
            parse_usize("HOST_MONITORING_TELEMETRY_BATCH_SIZE", 64)?,
            Duration::from_millis(parse_u64(
                "HOST_MONITORING_TELEMETRY_FLUSH_MILLISECONDS",
                25,
            )?),
            Duration::from_millis(parse_u64(
                "HOST_MONITORING_TELEMETRY_ENQUEUE_WAIT_MILLISECONDS",
                10,
            )?),
            Duration::from_millis(parse_u64(
                "HOST_MONITORING_TELEMETRY_REQUEST_TIMEOUT_MILLISECONDS",
                10_000,
            )?),
            Duration::from_millis(parse_u64(
                "HOST_MONITORING_TELEMETRY_SHUTDOWN_DRAIN_MILLISECONDS",
                15_000,
            )?),
        )?;
        let retention = RetentionConfig::new(
            parse_days(
                "HOST_MONITORING_RAW_RETENTION_DAYS",
                DEFAULT_RAW_RETENTION_DAYS,
            )?,
            parse_days(
                "HOST_MONITORING_AGGREGATE_RETENTION_DAYS",
                DEFAULT_AGGREGATE_RETENTION_DAYS,
            )?,
            Duration::from_secs(parse_u64(
                "HOST_MONITORING_RETENTION_INTERVAL_SECONDS",
                DEFAULT_MAINTENANCE_INTERVAL_SECONDS,
            )?),
            parse_usize(
                "HOST_MONITORING_RETENTION_BATCH_SIZE",
                DEFAULT_RETENTION_BATCH_SIZE,
            )?,
            parse_usize(
                "HOST_MONITORING_RETENTION_MAX_TRANSACTIONS_PER_RUN",
                DEFAULT_RETENTION_TRANSACTIONS,
            )?,
            Duration::from_millis(parse_u64(
                "HOST_MONITORING_RETENTION_MAX_RUN_MILLISECONDS",
                DEFAULT_RETENTION_RUN_MILLISECONDS,
            )?),
            Duration::from_millis(parse_u64(
                "HOST_MONITORING_RETENTION_YIELD_MILLISECONDS",
                DEFAULT_RETENTION_YIELD_MILLISECONDS,
            )?),
        )?;
        Ok(Self {
            bind,
            database_url,
            administrator_origin: cookie_mode,
            bootstrap_admin_username,
            bootstrap_admin_password: env::var("HOST_MONITORING_BOOTSTRAP_ADMIN_PASSWORD").ok(),
            telemetry,
            retention,
            static_dir,
            client_authorization_key: STANDARD
                .decode(required("HOST_MONITORING_CLIENT_AUTHORIZATION_KEY")?)?
                .try_into()
                .map_err(|_| {
                    anyhow::anyhow!(
                        "HOST_MONITORING_CLIENT_AUTHORIZATION_KEY must decode to exactly 32 bytes"
                    )
                })?,
        })
    }
}

fn validate_static_dir(value: &str, production: bool) -> anyhow::Result<PathBuf> {
    let configured = Path::new(value);
    anyhow::ensure!(
        configured.is_absolute(),
        "HOST_MONITORING_STATIC_DIR must be an absolute path"
    );
    let root = fs::canonicalize(configured)
        .map_err(|error| anyhow::anyhow!("resolve HOST_MONITORING_STATIC_DIR: {error}"))?;
    validate_static_tree(&root, production)?;
    anyhow::ensure!(
        root.join("index.html").is_file() && root.join("assets").is_dir(),
        "HOST_MONITORING_STATIC_DIR must contain index.html and the current assets directory"
    );
    let mut root_entries = fs::read_dir(&root)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    root_entries.sort();
    anyhow::ensure!(
        root_entries
            == [
                std::ffi::OsString::from("assets"),
                std::ffi::OsString::from("index.html"),
            ],
        "HOST_MONITORING_STATIC_DIR is not the exact current asset layout"
    );
    Ok(root)
}

fn validate_static_tree(root: &Path, production: bool) -> anyhow::Result<()> {
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut entries = 0_usize;
    while let Some((path, depth)) = pending.pop() {
        anyhow::ensure!(depth <= 32, "static asset tree exceeds maximum depth");
        entries += 1;
        anyhow::ensure!(entries <= 10_000, "static asset tree is too large");
        let metadata = fs::symlink_metadata(&path)?;
        anyhow::ensure!(
            !metadata.file_type().is_symlink() && (metadata.is_dir() || metadata.is_file()),
            "static assets must contain only real directories and regular files"
        );
        #[cfg(unix)]
        validate_static_metadata(&metadata, production)?;
        if metadata.is_dir() {
            for child in fs::read_dir(&path)? {
                pending.push((child?.path(), depth + 1));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn validate_static_metadata(metadata: &fs::Metadata, production: bool) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    if production {
        anyhow::ensure!(
            metadata.uid() != rustix::process::geteuid().as_raw(),
            "production static assets must not be owned by the service account"
        );
        anyhow::ensure!(
            metadata.mode() & 0o022 == 0,
            "production static assets must not be group- or world-writable"
        );
    }
    if metadata.is_file() {
        anyhow::ensure!(
            metadata.nlink() == 1,
            "static asset files must have exactly one hard link"
        );
    }
    Ok(())
}

fn required(name: &str) -> anyhow::Result<String> {
    env::var(name).map_err(|_| anyhow::anyhow!("{name} is required"))
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

fn parse_u64(name: &str, default: u64) -> anyhow::Result<u64> {
    value(name, &default.to_string())
        .parse()
        .map_err(|_| anyhow::anyhow!("{name} must be an unsigned integer"))
}

fn parse_usize(name: &str, default: usize) -> anyhow::Result<usize> {
    usize::try_from(parse_u64(name, default as u64)?)
        .map_err(|_| anyhow::anyhow!("{name} is too large for this platform"))
}

fn parse_days(name: &str, default: u64) -> anyhow::Result<Duration> {
    let days = parse_u64(name, default)?;
    let seconds = days
        .checked_mul(24 * 60 * 60)
        .ok_or_else(|| anyhow::anyhow!("{name} is too large"))?;
    Ok(Duration::from_secs(seconds))
}

fn parse_bool(name: &str, default: bool) -> anyhow::Result<bool> {
    value(name, if default { "true" } else { "false" })
        .parse()
        .map_err(|_| anyhow::anyhow!("{name} must be true or false"))
}

fn cookie_mode(bind: SocketAddr, development: bool) -> anyhow::Result<AdministratorOriginMode> {
    if development && !bind.ip().is_loopback() {
        anyhow::bail!("HOST_MONITORING_DEVELOPMENT requires a loopback HOST_MONITORING_BIND");
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
    fn product_cli_has_no_schema_upgrade_backup_or_restore_commands() {
        for removed in ["migrate", "backup-create", "backup-verify", "restore"] {
            assert!(
                Cli::try_parse_from(["host-monitoring-server", removed]).is_err(),
                "removed product command {removed} was still accepted"
            );
        }
    }

    #[test]
    fn administrator_username_uses_the_foundation_canonical_form() {
        assert_eq!(
            sarmg_admin_auth::normalize_administrator_username("  Release.Admin  ").unwrap(),
            "release.admin"
        );
        for rejected in ["ab", "admin@example.test", "管理员", "admin\n"] {
            assert!(sarmg_admin_auth::normalize_administrator_username(rejected).is_err());
        }
    }

    #[test]
    fn immutable_release_commands_require_an_explicit_root() {
        let root = "/opt/isarmg/host-monitoring/releases/0.9.31";
        assert!(
            Cli::try_parse_from(["host-monitoring-server", "serve-release", "--root", root])
                .is_ok()
        );
        assert!(
            Cli::try_parse_from(["host-monitoring-server", "verify-release", "--root", root])
                .is_ok()
        );
        assert!(Cli::try_parse_from(["host-monitoring-server", "serve-release"]).is_err());
        assert!(Cli::try_parse_from(["host-monitoring-server", "verify-release"]).is_err());
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
