//! Optional schema-v2 hardware inventory and telemetry. Unknown readings stay null.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MAX_HARDWARE_SENSORS: usize = 512;
pub const MAX_HARDWARE_DISKS: usize = 64;
pub const MAX_HARDWARE_NETWORKS: usize = 128;
pub const MAX_HARDWARE_TEXT: usize = 255;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareSnapshot {
    pub collected_at: DateTime<Utc>,
    pub cpu: CpuHardware,
    /// Interface attributes; these do not prove that an interface is a physical adapter.
    pub networks: Vec<NetworkHardware>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub physical_networks: Vec<PhysicalNetworkAdapter>,
    pub sensors: Vec<HardwareSensor>,
    pub disk_health: Vec<DiskHealth>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuHardware {
    pub model: Option<String>,
    pub vendor: Option<String>,
    pub frequency_mhz: Option<f64>,
    /// Maximum hardware frequency, not a promise of sustained boost performance.
    pub max_frequency_mhz: Option<f64>,
    pub per_core_frequency_mhz: Vec<Option<f64>>,
    /// Unix 1/5/15-minute load; absent on platforms without that concept.
    pub load_average: Option<[f64; 3]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkHardware {
    pub name: String,
    pub mac_address: Option<String>,
    pub ip_addresses: Vec<String>,
    #[serde(default, with = "crate::json_u64::option")]
    pub mtu: Option<u64>,
    pub link_speed_mbps: Option<f64>,
    pub operational_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalNetworkAdapter {
    pub id: String,
    pub name: String,
    pub interface_name: Option<String>,
    pub mac_address: Option<String>,
    pub link_speed_mbps: Option<f64>,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorKind {
    FanRpm,
    VoltageVolts,
    CurrentAmps,
    PowerWatts,
    EnergyJoules,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareSensor {
    pub id: String,
    pub label: String,
    pub kind: SensorKind,
    pub value: f64,
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskHealth {
    pub device: String,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub protocol: Option<String>,
    pub collected_at: DateTime<Utc>,
    pub healthy: Option<bool>,
    pub temperature_celsius: Option<f64>,
    /// NVMe allows values above 100 when rated endurance is exceeded.
    pub percentage_used: Option<f64>,
    pub available_spare_percent: Option<f64>,
    pub critical_warning: Option<u8>,
    #[serde(default, with = "crate::json_u64::option")]
    pub power_on_hours: Option<u64>,
    #[serde(default, with = "crate::json_u64::option")]
    pub power_cycles: Option<u64>,
    #[serde(default, with = "crate::json_u64::option")]
    pub unsafe_shutdowns: Option<u64>,
    #[serde(default, with = "crate::json_u64::option")]
    pub media_errors: Option<u64>,
    #[serde(default, with = "crate::json_u64::option")]
    pub bytes_read: Option<u64>,
    #[serde(default, with = "crate::json_u64::option")]
    pub bytes_written: Option<u64>,
    pub source: String,
}
