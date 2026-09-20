use chrono::{DateTime, Utc};
use host_protocol::{
    CLIENT_REPORT_MAX_CAPABILITIES, CLIENT_REPORT_MAX_CAPABILITY_MESSAGE_BYTES,
    CLIENT_REPORT_MAX_CAPABILITY_NAME_BYTES, CLIENT_REPORT_MAX_CAPABILITY_SOURCE_BYTES,
    CLIENT_REPORT_MAX_CLIENT_VERSION_BYTES, CLIENT_REPORT_MAX_CPU_CORES,
    CLIENT_REPORT_MAX_DISK_NAME_BYTES, CLIENT_REPORT_MAX_DISKS,
    CLIENT_REPORT_MAX_FILE_SYSTEM_BYTES, CLIENT_REPORT_MAX_GPU_ID_BYTES,
    CLIENT_REPORT_MAX_GPU_NAME_BYTES, CLIENT_REPORT_MAX_GPU_SOURCE_BYTES,
    CLIENT_REPORT_MAX_GPU_VENDOR_BYTES, CLIENT_REPORT_MAX_GPUS, CLIENT_REPORT_MAX_HOST_ARCH_BYTES,
    CLIENT_REPORT_MAX_HOST_OS_BYTES, CLIENT_REPORT_MAX_HOST_VERSION_BYTES,
    CLIENT_REPORT_MAX_INTERVAL_SECONDS, CLIENT_REPORT_MAX_MOUNT_POINT_BYTES,
    CLIENT_REPORT_MAX_NETWORK_NAME_BYTES, CLIENT_REPORT_MAX_NETWORKS,
    CLIENT_REPORT_MAX_TEMPERATURE_ID_BYTES, CLIENT_REPORT_MAX_TEMPERATURE_LABEL_BYTES,
    CLIENT_REPORT_MAX_TEMPERATURE_SOURCE_BYTES, CLIENT_REPORT_MAX_TEMPERATURES,
    CLIENT_REPORT_MIN_INTERVAL_SECONDS, CLIENT_REPORT_SCHEMA_VERSION, Capability,
    ClientPairingRequest, ClientReport, HOST_PAIRING_PROTOCOL_VERSION, HostIdentity,
};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateClientInstanceRequest {
    pub display_name: Option<String>,
}

impl CreateClientInstanceRequest {
    pub fn validated(self) -> Result<String> {
        let display_name = self
            .display_name
            .as_deref()
            .unwrap_or("新实例")
            .trim()
            .to_owned();
        validate_required("display_name", &display_name, 255)?;
        validate_instance_name(&display_name)?;
        Ok(display_name)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateMonitoringRemarkRequest {
    pub remark: String,
}

impl UpdateMonitoringRemarkRequest {
    pub fn validated(self) -> Result<String> {
        let value = self.remark.trim().to_owned();
        validate_required("remark", &value, 255)?;
        validate_instance_name(&value)?;
        Ok(value)
    }
}

fn validate_instance_name(value: &str) -> Result<()> {
    if value.chars().count() > 32 {
        return Err(Error::BadRequest(
            "instance name must contain at most 32 characters".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod instance_name_tests {
    use super::*;

    #[test]
    fn instance_creation_has_no_expiration_option() {
        assert!(
            serde_json::from_value::<CreateClientInstanceRequest>(
                serde_json::json!({"display_name":"new","expires_in_minutes":15})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<CreateClientInstanceRequest>(
                serde_json::json!({"display_name":"new"})
            )
            .unwrap()
            .validated()
            .is_ok()
        );
        assert_eq!(
            serde_json::from_value::<CreateClientInstanceRequest>(serde_json::json!({}))
                .unwrap()
                .validated()
                .unwrap(),
            "新实例"
        );
    }

    #[test]
    fn create_and_rename_limit_names_to_32_unicode_characters() {
        for character in ["a", "中", "😀"] {
            for count in [32, 33] {
                let name = character.repeat(count);
                assert_eq!(
                    CreateClientInstanceRequest {
                        display_name: Some(name.clone()),
                    }
                    .validated()
                    .is_ok(),
                    count == 32
                );
                assert_eq!(
                    UpdateMonitoringRemarkRequest { remark: name }
                        .validated()
                        .is_ok(),
                    count == 32
                );
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientInstanceSummary {
    pub request_id: String,
    pub instance_id: String,
    pub display_name: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub authorization_code: String,
}

#[derive(Debug, Serialize)]
pub struct ClientInstanceListResponse {
    pub instances: Vec<ClientInstanceSummary>,
    pub hosts: Vec<HostSummary>,
}

#[derive(Debug, Serialize)]
pub struct CreatedClientInstance {
    #[serde(flatten)]
    pub summary: ClientInstanceSummary,
    pub activation_code: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateClientAuthorizationRequest {
    pub authorization_code: String,
}

impl UpdateClientAuthorizationRequest {
    pub fn validated(self) -> Result<String> {
        validate_activation_code(&self.authorization_code)?;
        Ok(self.authorization_code)
    }
}

pub fn validate_activation_code(value: &str) -> Result<()> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte.is_ascii_lowercase())
    {
        return Err(Error::BadRequest(
            "authorization_code must contain exactly 32 lowercase letters or digits".into(),
        ));
    }
    Ok(())
}

pub fn validate_stored_activation_code(value: &str) -> Result<()> {
    if validate_activation_code(value).is_ok()
        || (value.starts_with("uci_")
            && value.len() == 36
            && value[4..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
    {
        Ok(())
    } else {
        Err(Error::BadRequest(
            "stored authorization_code format is invalid".into(),
        ))
    }
}

#[derive(Debug, Serialize)]
pub struct ClientPairingPublicSummary {
    pub request_id: String,
    pub os: String,
    pub arch: String,
    pub client_version: String,
    pub status: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricSummary {
    pub cpu_usage_percent: Option<f64>,
    pub memory_usage_percent: Option<f64>,
    pub network_received_bytes_per_second: Option<f64>,
    pub network_transmitted_bytes_per_second: Option<f64>,
    pub disk_read_bytes_per_second: Option<f64>,
    pub disk_written_bytes_per_second: Option<f64>,
    pub max_temperature_celsius: Option<f64>,
    pub gpu_utilization_percent: Option<f64>,
    pub gpu_memory_usage_percent: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct HostSummary {
    pub id: String,
    pub name: String,
    pub os: String,
    pub os_version: Option<String>,
    pub kernel_version: Option<String>,
    pub arch: String,
    pub client_version: String,
    pub registered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub latest_collected_at: Option<DateTime<Utc>>,
    pub status: String,
    pub capabilities: Vec<Capability>,
    #[serde(flatten)]
    pub metrics: MetricSummary,
}

#[derive(Debug, Serialize)]
pub struct HostListResponse {
    pub hosts: Vec<HostSummary>,
    pub statistics: HostStatistics,
}

#[derive(Debug, Default, Serialize)]
pub struct HostCount {
    pub total: i64,
    pub online: i64,
}

#[derive(Debug, Default, Serialize)]
pub struct HostStatistics {
    pub total: HostCount,
    pub windows: HostCount,
    pub linux: HostCount,
    pub macos: HostCount,
}

#[derive(Debug, Serialize)]
pub struct HostDetailResponse {
    pub host: HostSummary,
    pub latest: Option<ClientReport>,
}

#[derive(Debug, Serialize)]
pub struct HistoryPoint {
    pub report_id: String,
    pub collected_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    #[serde(flatten)]
    pub metrics: MetricSummary,
}

#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub host_id: String,
    pub points: Vec<HistoryPoint>,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct MetricAggregate {
    pub count: i64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub avg: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct HistoryBucket {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub sample_count: i64,
    pub cpu_usage_percent: MetricAggregate,
    pub memory_usage_percent: MetricAggregate,
    pub network_received_bytes_per_second: MetricAggregate,
    pub network_transmitted_bytes_per_second: MetricAggregate,
    pub disk_read_bytes_per_second: MetricAggregate,
    pub disk_written_bytes_per_second: MetricAggregate,
    pub max_temperature_celsius: MetricAggregate,
    pub gpu_utilization_percent: MetricAggregate,
    pub gpu_memory_usage_percent: MetricAggregate,
}

#[derive(Debug, Serialize)]
pub struct HistorySeriesResponse {
    pub host_id: String,
    pub requested_from: DateTime<Utc>,
    pub requested_to: DateTime<Utc>,
    pub actual_from: DateTime<Utc>,
    pub actual_to: DateTime<Utc>,
    pub step_seconds: i64,
    pub source: String,
    pub points: Vec<HistoryBucket>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryQuery {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
    pub resolution: Option<String>,
    pub max_points: Option<i64>,
}

pub fn validate_pairing(request: &ClientPairingRequest) -> Result<()> {
    if request.protocol_version != HOST_PAIRING_PROTOCOL_VERSION {
        return Err(Error::UnsupportedClientProtocol {
            received: request.protocol_version,
            supported: HOST_PAIRING_PROTOCOL_VERSION,
        });
    }
    validate_host(&request.host)?;
    validate_hash("token_hash", &request.token_hash)?;
    validate_hash("polling_secret_hash", &request.polling_secret_hash)?;
    if request.token_hash == request.polling_secret_hash {
        return Err(Error::BadRequest(
            "token_hash and polling_secret_hash must differ".into(),
        ));
    }
    Ok(())
}

pub fn validate_report(report: &ClientReport) -> Result<MetricSummary> {
    validate_host(&report.host)?;
    canonical_uuid(&report.report_id, "report_id")?;
    if report.schema_version != CLIENT_REPORT_SCHEMA_VERSION {
        return Err(Error::BadRequest(
            "unsupported client report schema_version".into(),
        ));
    }
    if !report.interval_seconds.is_finite()
        || !(CLIENT_REPORT_MIN_INTERVAL_SECONDS..=CLIENT_REPORT_MAX_INTERVAL_SECONDS as f64)
            .contains(&report.interval_seconds)
    {
        return Err(Error::BadRequest(
            "interval_seconds is outside the supported range".into(),
        ));
    }
    if report.collected_at > Utc::now() + chrono::Duration::minutes(5) {
        return Err(Error::BadRequest(
            "collected_at is too far in the future".into(),
        ));
    }
    if report.capabilities.len() > CLIENT_REPORT_MAX_CAPABILITIES
        || report.system.cpu.per_core_percent.len() > CLIENT_REPORT_MAX_CPU_CORES
        || report.system.networks.len() > CLIENT_REPORT_MAX_NETWORKS
        || report.system.disks.len() > CLIENT_REPORT_MAX_DISKS
        || report.system.temperatures.len() > CLIENT_REPORT_MAX_TEMPERATURES
        || report.system.gpus.len() > CLIENT_REPORT_MAX_GPUS
    {
        return Err(Error::BadRequest("report contains too many devices".into()));
    }
    if report.system.cpu.logical_count == 0
        || report.system.cpu.per_core_percent.len() != report.system.cpu.logical_count as usize
    {
        return Err(Error::BadRequest(
            "cpu core count and per-core values disagree".into(),
        ));
    }
    percent("cpu.usage_percent", report.system.cpu.usage_percent)?;
    for value in &report.system.cpu.per_core_percent {
        percent("cpu.per_core_percent", *value)?;
    }
    if report.system.memory.used_bytes > report.system.memory.total_bytes
        || report.system.memory.available_bytes > report.system.memory.total_bytes
        || report.system.memory.swap_used_bytes > report.system.memory.swap_total_bytes
    {
        return Err(Error::BadRequest("memory counters exceed totals".into()));
    }
    for capability in &report.capabilities {
        validate_required(
            "capability.name",
            &capability.name,
            CLIENT_REPORT_MAX_CAPABILITY_NAME_BYTES,
        )?;
        validate_required(
            "capability.source",
            &capability.source,
            CLIENT_REPORT_MAX_CAPABILITY_SOURCE_BYTES,
        )?;
        if let Some(message) = &capability.message {
            validate_optional(
                "capability.message",
                message,
                CLIENT_REPORT_MAX_CAPABILITY_MESSAGE_BYTES,
            )?;
        }
    }
    for network in &report.system.networks {
        validate_required(
            "network.name",
            &network.name,
            CLIENT_REPORT_MAX_NETWORK_NAME_BYTES,
        )?;
        nonnegative(
            "network.received_bytes_per_second",
            network.received_bytes_per_second,
        )?;
        nonnegative(
            "network.transmitted_bytes_per_second",
            network.transmitted_bytes_per_second,
        )?;
    }
    for disk in &report.system.disks {
        validate_optional("disk.name", &disk.name, CLIENT_REPORT_MAX_DISK_NAME_BYTES)?;
        validate_required(
            "disk.mount_point",
            &disk.mount_point,
            CLIENT_REPORT_MAX_MOUNT_POINT_BYTES,
        )?;
        validate_optional(
            "disk.file_system",
            &disk.file_system,
            CLIENT_REPORT_MAX_FILE_SYSTEM_BYTES,
        )?;
        if disk.available_bytes > disk.total_bytes {
            return Err(Error::BadRequest(
                "disk available bytes exceed total".into(),
            ));
        }
        nonnegative("disk.read_bytes_per_second", disk.read_bytes_per_second)?;
        nonnegative(
            "disk.written_bytes_per_second",
            disk.written_bytes_per_second,
        )?;
    }
    for sensor in &report.system.temperatures {
        validate_optional(
            "temperature.id",
            &sensor.id,
            CLIENT_REPORT_MAX_TEMPERATURE_ID_BYTES,
        )?;
        validate_optional(
            "temperature.label",
            &sensor.label,
            CLIENT_REPORT_MAX_TEMPERATURE_LABEL_BYTES,
        )?;
        validate_optional(
            "temperature.source",
            &sensor.source,
            CLIENT_REPORT_MAX_TEMPERATURE_SOURCE_BYTES,
        )?;
        validate_temperature(sensor.celsius)?;
        validate_temperature(sensor.max_celsius)?;
        validate_temperature(sensor.critical_celsius)?;
    }
    for gpu in &report.system.gpus {
        validate_optional("gpu.id", &gpu.id, CLIENT_REPORT_MAX_GPU_ID_BYTES)?;
        validate_optional(
            "gpu.vendor",
            &gpu.vendor,
            CLIENT_REPORT_MAX_GPU_VENDOR_BYTES,
        )?;
        validate_optional("gpu.name", &gpu.name, CLIENT_REPORT_MAX_GPU_NAME_BYTES)?;
        validate_optional(
            "gpu.source",
            &gpu.source,
            CLIENT_REPORT_MAX_GPU_SOURCE_BYTES,
        )?;
        if let Some(value) = gpu.utilization_percent {
            percent("gpu.utilization_percent", value)?;
        }
        validate_temperature(gpu.temperature_celsius)?;
        if gpu
            .memory_used_bytes
            .zip(gpu.memory_total_bytes)
            .is_some_and(|(used, total)| used > total)
        {
            return Err(Error::BadRequest("GPU memory usage exceeds total".into()));
        }
        for value in [
            gpu.power_watts,
            gpu.core_clock_mhz,
            gpu.memory_clock_mhz,
            gpu.pcie_rx_bytes_per_second,
            gpu.pcie_tx_bytes_per_second,
        ]
        .into_iter()
        .flatten()
        {
            nonnegative("gpu metric", value)?;
        }
    }
    Ok(metric_summary(report))
}

pub fn validate_host(host: &HostIdentity) -> Result<()> {
    canonical_uuid(&host.id, "host.id")?;
    validate_required("host.os", &host.os, CLIENT_REPORT_MAX_HOST_OS_BYTES)?;
    validate_required("host.arch", &host.arch, CLIENT_REPORT_MAX_HOST_ARCH_BYTES)?;
    validate_required(
        "host.client_version",
        &host.client_version,
        CLIENT_REPORT_MAX_CLIENT_VERSION_BYTES,
    )?;
    if !host
        .client_version
        .bytes()
        .all(|byte| byte == b' ' || byte.is_ascii_graphic())
    {
        return Err(Error::BadRequest(
            "host.client_version must contain printable ASCII".into(),
        ));
    }
    validate_optional(
        "host.os_version",
        host.os_version.as_deref().unwrap_or(""),
        CLIENT_REPORT_MAX_HOST_VERSION_BYTES,
    )?;
    validate_optional(
        "host.kernel_version",
        host.kernel_version.as_deref().unwrap_or(""),
        CLIENT_REPORT_MAX_HOST_VERSION_BYTES,
    )?;
    Ok(())
}

pub fn canonical_uuid(value: &str, field: &str) -> Result<uuid::Uuid> {
    let parsed = uuid::Uuid::parse_str(value)
        .map_err(|_| Error::BadRequest(format!("{field} must be a canonical UUID")))?;
    if parsed.to_string() != value {
        return Err(Error::BadRequest(format!(
            "{field} must be lowercase and hyphenated"
        )));
    }
    Ok(parsed)
}

fn validate_hash(field: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(Error::BadRequest(format!(
            "{field} must be lowercase SHA-256 hex"
        )));
    }
    Ok(())
}

fn validate_required(field: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::BadRequest(format!("{field} must not be empty")));
    }
    validate_optional(field, value, max)
}

fn validate_optional(field: &str, value: &str, max: usize) -> Result<()> {
    if value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::BadRequest(format!("invalid {field}")));
    }
    Ok(())
}

fn percent(field: &str, value: f64) -> Result<()> {
    if !value.is_finite() || !(0.0..=100.0).contains(&value) {
        return Err(Error::BadRequest(format!("invalid {field}")));
    }
    Ok(())
}

fn validate_temperature(value: Option<f64>) -> Result<()> {
    if value.is_some_and(|value| !value.is_finite() || !(-273.15..=1000.0).contains(&value)) {
        return Err(Error::BadRequest("invalid temperature".into()));
    }
    Ok(())
}

fn nonnegative(field: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value < 0.0 {
        return Err(Error::BadRequest(format!("invalid {field}")));
    }
    Ok(())
}

fn metric_summary(report: &ClientReport) -> MetricSummary {
    let gpu_memory = report
        .system
        .gpus
        .iter()
        .filter_map(|gpu| gpu.memory_used_bytes.zip(gpu.memory_total_bytes))
        .fold((0_u64, 0_u64), |sum, value| {
            (sum.0.saturating_add(value.0), sum.1.saturating_add(value.1))
        });
    MetricSummary {
        cpu_usage_percent: Some(report.system.cpu.usage_percent),
        memory_usage_percent: (report.system.memory.total_bytes > 0).then(|| {
            report.system.memory.used_bytes as f64 * 100.0 / report.system.memory.total_bytes as f64
        }),
        network_received_bytes_per_second: report
            .system
            .networks
            .iter()
            .map(|v| v.received_bytes_per_second)
            .reduce(f64::max),
        network_transmitted_bytes_per_second: report
            .system
            .networks
            .iter()
            .map(|v| v.transmitted_bytes_per_second)
            .reduce(f64::max),
        disk_read_bytes_per_second: report
            .system
            .disks
            .iter()
            .map(|v| v.read_bytes_per_second)
            .reduce(f64::max),
        disk_written_bytes_per_second: report
            .system
            .disks
            .iter()
            .map(|v| v.written_bytes_per_second)
            .reduce(f64::max),
        max_temperature_celsius: report
            .system
            .temperatures
            .iter()
            .filter_map(|v| v.celsius)
            .chain(
                report
                    .system
                    .gpus
                    .iter()
                    .filter_map(|v| v.temperature_celsius),
            )
            .reduce(f64::max),
        gpu_utilization_percent: report
            .system
            .gpus
            .iter()
            .filter_map(|v| v.utilization_percent)
            .reduce(f64::max),
        gpu_memory_usage_percent: (gpu_memory.1 > 0)
            .then(|| gpu_memory.0 as f64 * 100.0 / gpu_memory.1 as f64),
    }
}

pub fn host_status(last_seen: DateTime<Utc>, interval: Option<f64>) -> String {
    let age = (Utc::now() - last_seen).num_seconds().max(0) as f64;
    let interval = interval.unwrap_or(10.0).clamp(1.0, 3600.0);
    if age <= (interval * 3.0).max(30.0) {
        "online"
    } else if age <= (interval * 12.0).max(300.0) {
        "stale"
    } else {
        "offline"
    }
    .into()
}

#[cfg(test)]
mod client_release_tests {
    use super::*;
    #[test]
    fn client_release_is_diagnostic_and_not_a_compatibility_gate() {
        let mut host: HostIdentity = serde_json::from_value(serde_json::json!({
            "id": "018f1f4b-7a5d-7b5f-8d31-123456789abc", "os": "windows",
            "arch": "x86_64", "client_version": "0.9.7"
        }))
        .unwrap();
        for version in ["0.9.3", "0.9.23", "0.9.999", "development-build"] {
            host.client_version = version.into();
            assert!(validate_host(&host).is_ok());
        }
        host.client_version = "\n".into();
        assert!(validate_host(&host).is_err());
        host.client_version = "版本一".into();
        assert!(validate_host(&host).is_err());
    }

    #[test]
    fn pairing_compatibility_uses_protocol_not_release_version() {
        let host: HostIdentity = serde_json::from_value(serde_json::json!({
            "id": "018f1f4b-7a5d-7b5f-8d31-123456789abc", "os": "windows",
            "arch": "x86_64", "client_version": "0.9.999"
        }))
        .unwrap();
        let mut request = ClientPairingRequest {
            protocol_version: HOST_PAIRING_PROTOCOL_VERSION,
            mode: host_protocol::ClientPairingMode::Fresh,
            host,
            token_hash: "a".repeat(64),
            polling_secret_hash: "b".repeat(64),
        };
        assert!(validate_pairing(&request).is_ok());
        request.protocol_version = 2;
        assert!(matches!(
            validate_pairing(&request),
            Err(Error::UnsupportedClientProtocol {
                received: 2,
                supported: 1
            })
        ));
    }
}
