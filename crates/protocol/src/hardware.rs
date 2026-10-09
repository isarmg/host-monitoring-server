//! Schema-v1 hardware inventory and telemetry. Unknown readings stay null.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MAX_HARDWARE_SENSORS: usize = 512;
pub const MAX_HARDWARE_DISKS: usize = 64;
pub const MAX_HARDWARE_NETWORKS: usize = 128;
pub const MAX_HARDWARE_TEXT: usize = 255;
pub const MAX_MEMORY_MODULES: usize = 64;
pub const MAX_HARDWARE_DEVICES: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareSnapshot {
    pub collected_at: DateTime<Utc>,
    pub cpu: CpuHardware,
    /// Interface attributes; these do not prove that an interface is a physical adapter.
    pub networks: Vec<NetworkHardware>,
    pub physical_networks: Vec<PhysicalNetworkAdapter>,
    pub sensors: Vec<HardwareSensor>,
    pub disk_health: Vec<DiskHealth>,
    /// Last completed scan; null while the first scan is pending.
    #[serde(deserialize_with = "deserialize_inventory_time")]
    pub inventory_collected_at: Option<DateTime<Utc>>,
    pub memory_modules: Vec<MemoryModule>,
    pub devices: Vec<HardwareDevice>,
}

fn deserialize_inventory_time<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DateTime<Utc>>, D::Error> {
    Option::deserialize(deserializer)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryModule {
    pub id: String,
    pub locator: Option<String>,
    /// Module part number, when supplied by firmware.
    pub model: Option<String>,
    pub vendor: Option<String>,
    /// DDR/LPDDR generation; this is not the SMBIOS specification version.
    pub memory_type: Option<String>,
    /// Module version or firmware revision as reported by the native provider.
    pub module_version: Option<String>,
    pub form_factor: Option<String>,
    #[serde(default, with = "crate::json_u64::option")]
    pub capacity_bytes: Option<u64>,
    /// Rated and configured transfer rates in MT/s, not clock frequencies in MHz.
    pub speed_mt_s: Option<f64>,
    pub configured_speed_mt_s: Option<f64>,
    /// Original platform rate label when its unit cannot be normalized safely.
    pub reported_speed: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareDeviceKind {
    Thunderbolt,
    Monitor,
    Bluetooth,
    UsbController,
    UsbDevice,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareDevice {
    pub id: String,
    pub kind: HardwareDeviceKind,
    pub name: String,
    pub model: Option<String>,
    pub vendor: Option<String>,
    pub vendor_id: Option<String>,
    pub product_id: Option<String>,
    pub revision: Option<String>,
    pub version: Option<String>,
    pub bus: Option<String>,
    pub driver: Option<String>,
    pub connection: Option<String>,
    /// Negotiated/exposed link speed, when available. No inferred marketing rates.
    pub speed_mbps: Option<f64>,
    pub source: String,
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

#[cfg(test)]
mod inventory_tests {
    use super::*;
    #[test]
    fn inventory_fields_are_required_and_unknown_fields_remain_strict() {
        let old = serde_json::json!({"collected_at":"2026-10-08T00:00:00Z", "cpu":CpuHardware::default(), "networks":[], "physical_networks":[], "sensors":[], "disk_health":[]});
        assert!(serde_json::from_value::<HardwareSnapshot>(old.clone()).is_err());
        let mut current = old;
        current["inventory_collected_at"] = serde_json::Value::Null;
        current["memory_modules"] = serde_json::json!([]);
        current["devices"] = serde_json::json!([]);
        let mut snapshot: HardwareSnapshot = serde_json::from_value(current.clone()).unwrap();
        assert!(snapshot.memory_modules.is_empty());
        assert!(snapshot.devices.is_empty());
        assert_eq!(serde_json::to_value(&snapshot).unwrap(), current);
        snapshot.memory_modules.push(MemoryModule {
            id: "dimm0".into(),
            source: "test".into(),
            capacity_bytes: Some(u64::MAX),
            ..Default::default()
        });
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(
            json["memory_modules"][0]["capacity_bytes"],
            u64::MAX.to_string()
        );
        assert_eq!(
            serde_json::from_value::<HardwareSnapshot>(json).unwrap(),
            snapshot
        );
        for key in ["inventory_collected_at", "memory_modules", "devices"] {
            let mut missing = current.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(serde_json::from_value::<HardwareSnapshot>(missing).is_err());
        }
        let mut unknown = current;
        unknown["unexpected"] = serde_json::json!([]);
        assert!(serde_json::from_value::<HardwareSnapshot>(unknown).is_err());
    }
}
