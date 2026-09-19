use anyhow::Context;
use sarmg_server_target::SERVER_TARGET_TRIPLE;
use serde::{Deserialize, Serialize};

use crate::database_schema::{
    APPLICATION, APPLICATION_VERSION, SCHEMA_APPLICATION_VERSION, SCHEMA_REVISION, SCHEMA_SHA256,
};

pub const RELEASE_MANIFEST_FORMAT: &str = "host-monitoring-release-v2";
pub const SUPPORTED_SERVER_TARGET: &str = SERVER_TARGET_TRIPLE;
pub const BUILD_TARGET: &str = env!("HOST_MONITORING_BUILD_TARGET");
pub const SOURCE_REVISION: &str = env!("HOST_MONITORING_SOURCE_REVISION");

pub fn ensure_supported_runtime() -> anyhow::Result<()> {
    anyhow::ensure!(
        BUILD_TARGET == SUPPORTED_SERVER_TARGET
            && cfg!(all(
                target_arch = "x86_64",
                target_os = "linux",
                target_env = "gnu"
            )),
        "Host Monitoring Server binary target must be {SUPPORTED_SERVER_TARGET}"
    );

    let runtime = rustix::system::uname();
    validate_runtime_platform(runtime.sysname().to_bytes(), runtime.machine().to_bytes())
}

fn validate_runtime_platform(sysname: &[u8], machine: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
        sysname == b"Linux" && machine == b"x86_64",
        "Host Monitoring Server only runs on x86_64 GNU/Linux; runtime reported system={:?}, machine={:?}",
        String::from_utf8_lossy(sysname),
        String::from_utf8_lossy(machine)
    );
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseContract {
    pub manifest_format: String,
    pub application: String,
    pub version: String,
    pub schema_application_version: String,
    pub api_prefix: String,
    pub schema_revision: i64,
    pub schema_sha256: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinaryIdentity {
    pub manifest_format: String,
    pub application: String,
    pub version: String,
    pub schema_application_version: String,
    pub api_prefix: String,
    pub schema_revision: i64,
    pub schema_sha256: String,
    pub target: String,
    pub source_revision: String,
}

impl ReleaseContract {
    pub fn current() -> Self {
        Self {
            manifest_format: RELEASE_MANIFEST_FORMAT.to_owned(),
            application: APPLICATION.to_owned(),
            version: APPLICATION_VERSION.to_owned(),
            schema_application_version: SCHEMA_APPLICATION_VERSION.to_owned(),
            api_prefix: host_protocol::API_PREFIX.to_owned(),
            schema_revision: SCHEMA_REVISION,
            schema_sha256: SCHEMA_SHA256.to_owned(),
            target: BUILD_TARGET.to_owned(),
        }
    }
}

impl BinaryIdentity {
    pub fn current() -> anyhow::Result<Self> {
        let contract = embedded()?;
        Ok(Self {
            manifest_format: contract.manifest_format,
            application: contract.application,
            version: contract.version,
            schema_application_version: contract.schema_application_version,
            api_prefix: contract.api_prefix,
            schema_revision: contract.schema_revision,
            schema_sha256: contract.schema_sha256,
            target: contract.target,
            source_revision: SOURCE_REVISION.to_owned(),
        })
    }

    pub fn is_release_bound(&self) -> bool {
        self.source_revision.len() == 40
            && self
                .source_revision
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    }
}

pub fn parse_exact(input: &str) -> anyhow::Result<ReleaseContract> {
    let parsed: ReleaseContract =
        serde_json::from_str(input).context("release contract must be strict JSON")?;
    anyhow::ensure!(
        parsed == ReleaseContract::current(),
        "release contract is not the exact current Host Monitoring contract"
    );
    Ok(parsed)
}

pub fn embedded() -> anyhow::Result<ReleaseContract> {
    parse_exact(include_str!("../release.json"))
}

pub fn current_json() -> anyhow::Result<String> {
    let identity = BinaryIdentity::current()?;
    serde_json::to_string(&identity).context("serialize current binary identity")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_contract_is_the_exact_compiled_identity() {
        assert_eq!(embedded().unwrap(), ReleaseContract::current());
        assert_eq!(BUILD_TARGET, SUPPORTED_SERVER_TARGET);
    }

    #[test]
    fn runtime_platform_check_is_fail_closed() {
        assert!(validate_runtime_platform(b"Linux", b"x86_64").is_ok());
        assert!(validate_runtime_platform(b"Linux", b"aarch64").is_err());
        assert!(validate_runtime_platform(b"Windows_NT", b"x86_64").is_err());
        assert!(validate_runtime_platform(b"linux", b"x86_64").is_err());
        ensure_supported_runtime().unwrap();
    }

    #[test]
    fn other_versions_and_unknown_fields_are_rejected() {
        let mut other = serde_json::to_value(ReleaseContract::current()).unwrap();
        other["version"] = serde_json::json!("0.0.0");
        assert!(parse_exact(&serde_json::to_string(&other).unwrap()).is_err());

        let mut unknown = serde_json::to_value(ReleaseContract::current()).unwrap();
        unknown["unknown_extension"] = serde_json::json!(true);
        assert!(parse_exact(&serde_json::to_string(&unknown).unwrap()).is_err());
    }

    #[test]
    fn development_builds_are_explicitly_unbound() {
        let identity = BinaryIdentity::current().unwrap();
        assert_eq!(identity.source_revision, SOURCE_REVISION);
        assert_eq!(identity.is_release_bound(), SOURCE_REVISION != "unbound");
    }
}
