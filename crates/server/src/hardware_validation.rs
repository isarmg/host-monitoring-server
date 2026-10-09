use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use xsos_protocol::*;

pub(crate) fn validate(h: &HardwareSnapshot, report_time: DateTime<Utc>) -> Result<()> {
    let invalid = || Error::BadRequest("invalid hardware telemetry".into());
    let text = |s: &str| {
        !s.trim().is_empty() && s.len() <= MAX_HARDWARE_TEXT && !s.chars().any(char::is_control)
    };
    let optional_text = |s: &Option<String>| s.as_deref().is_none_or(text);
    let number = |v: Option<f64>| v.is_none_or(|v| v.is_finite() && v >= 0.0);
    if h.collected_at > report_time
        || h.sensors.len() > MAX_HARDWARE_SENSORS
        || h.networks.len() > MAX_HARDWARE_NETWORKS
        || h.physical_networks.len() > MAX_HARDWARE_NETWORKS
        || h.disk_health.len() > MAX_HARDWARE_DISKS
        || h.memory_modules.len() > MAX_MEMORY_MODULES
        || h.devices.len() > MAX_HARDWARE_DEVICES
        || h.inventory_collected_at
            .is_some_and(|time| time > report_time)
        || h.cpu.per_core_frequency_mhz.len() > CLIENT_REPORT_MAX_CPU_CORES
        || !optional_text(&h.cpu.model)
        || !optional_text(&h.cpu.vendor)
        || !number(h.cpu.frequency_mhz)
        || !number(h.cpu.max_frequency_mhz)
        || !h.cpu.per_core_frequency_mhz.iter().all(|v| number(*v))
        || !h
            .cpu
            .load_average
            .is_none_or(|v| v.into_iter().all(|v| number(Some(v))))
    {
        return Err(invalid());
    }
    let rate = |v: Option<f64>, max: f64| v.is_none_or(|v| v.is_finite() && v > 0.0 && v <= max);
    let mut module_ids = std::collections::HashSet::new();
    for m in &h.memory_modules {
        if !text(&m.id)
            || !text(&m.source)
            || ![
                &m.locator,
                &m.model,
                &m.vendor,
                &m.memory_type,
                &m.module_version,
                &m.form_factor,
                &m.reported_speed,
            ]
            .into_iter()
            .all(optional_text)
            || m.capacity_bytes == Some(0)
            || !rate(m.speed_mt_s, 10_000_000.0)
            || !rate(m.configured_speed_mt_s, 10_000_000.0)
            || !module_ids.insert((&m.source, &m.id))
        {
            return Err(invalid());
        }
    }
    let mut device_ids = std::collections::HashSet::new();
    for d in &h.devices {
        if !text(&d.id)
            || !text(&d.name)
            || !text(&d.source)
            || ![
                &d.model,
                &d.vendor,
                &d.vendor_id,
                &d.product_id,
                &d.revision,
                &d.version,
                &d.bus,
                &d.driver,
                &d.connection,
            ]
            .into_iter()
            .all(optional_text)
            || !rate(d.speed_mbps, 1_000_000_000.0)
            || !device_ids.insert((&d.source, &d.id, d.kind))
        {
            return Err(invalid());
        }
    }
    let mut sensor_ids = std::collections::HashSet::new();
    for s in &h.sensors {
        if !text(&s.id)
            || !text(&s.label)
            || !text(&s.source)
            || !s.value.is_finite()
            || (s.value < 0.0
                && !matches!(s.kind, SensorKind::VoltageVolts | SensorKind::CurrentAmps))
            || !sensor_ids.insert((&s.source, &s.id))
        {
            return Err(invalid());
        }
    }
    for n in &h.networks {
        if !text(&n.name)
            || !optional_text(&n.mac_address)
            || !optional_text(&n.operational_state)
            || !number(n.link_speed_mbps)
            || n.ip_addresses.len() > 64
            || !n.ip_addresses.iter().all(|s| text(s) && valid_address(s))
        {
            return Err(invalid());
        }
    }
    let mut adapter_ids = std::collections::HashSet::new();
    for n in &h.physical_networks {
        if !text(&n.id)
            || !text(&n.name)
            || !optional_text(&n.interface_name)
            || !optional_text(&n.mac_address)
            || !text(&n.source)
            || !number(n.link_speed_mbps)
            || !adapter_ids.insert((&n.source, &n.id))
        {
            return Err(invalid());
        }
    }
    for d in &h.disk_health {
        if !text(&d.device)
            || !text(&d.source)
            || !optional_text(&d.model)
            || !optional_text(&d.serial_number)
            || !optional_text(&d.protocol)
            || d.collected_at > report_time
            || !d
                .temperature_celsius
                .is_none_or(|v| v.is_finite() && (-273.15..=1000.0).contains(&v))
            || !d
                .percentage_used
                .is_none_or(|v| v.is_finite() && (0.0..=255.0).contains(&v))
            || !d
                .available_spare_percent
                .is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn valid_address(value: &str) -> bool {
    let Some((ip, prefix)) = value.split_once('/') else {
        return false;
    };
    match (ip.parse::<std::net::IpAddr>(), prefix.parse::<u8>()) {
        (Ok(ip), Ok(prefix)) => prefix <= if ip.is_ipv4() { 32 } else { 128 },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> HardwareSnapshot {
        HardwareSnapshot {
            collected_at: Utc::now(),
            cpu: CpuHardware::default(),
            networks: vec![],
            physical_networks: vec![],
            sensors: vec![],
            disk_health: vec![],
            inventory_collected_at: None,
            memory_modules: vec![MemoryModule {
                id: "dimm0".into(),
                vendor: Some("Kingston".into()),
                memory_type: Some("DDR5".into()),
                speed_mt_s: Some(5600.0),
                configured_speed_mt_s: Some(4800.0),
                source: "test".into(),
                ..Default::default()
            }],
            devices: vec![HardwareDevice {
                id: "audio0".into(),
                kind: HardwareDeviceKind::Audio,
                name: "Realtek ALC295".into(),
                model: None,
                vendor: None,
                vendor_id: None,
                product_id: None,
                revision: None,
                version: None,
                bus: None,
                driver: None,
                connection: None,
                speed_mbps: None,
                source: "test".into(),
            }],
        }
    }
    #[test]
    fn inventory_validates_limits_duplicates_text_units_and_time() {
        let h = snapshot();
        assert!(validate(&h, h.collected_at).is_ok());
        let mut value = h.clone();
        value.memory_modules.push(value.memory_modules[0].clone());
        assert!(validate(&value, value.collected_at).is_err());
        let mut value = h.clone();
        value.devices.push(value.devices[0].clone());
        assert!(validate(&value, value.collected_at).is_err());
        for speed in [-1.0, 0.0, f64::INFINITY, f64::NAN, 10_000_001.0] {
            let mut value = h.clone();
            value.memory_modules[0].speed_mt_s = Some(speed);
            assert!(validate(&value, value.collected_at).is_err());
        }
        let mut value = h.clone();
        value.devices[0].model = Some("bad\nmodel".into());
        assert!(validate(&value, value.collected_at).is_err());
        let mut value = h.clone();
        value.devices[0].vendor = Some("a".repeat(MAX_HARDWARE_TEXT + 1));
        assert!(validate(&value, value.collected_at).is_err());
        let mut value = h.clone();
        value.inventory_collected_at = Some(value.collected_at + chrono::Duration::seconds(1));
        assert!(validate(&value, value.collected_at).is_err());
        let mut value = h.clone();
        value.memory_modules = vec![value.memory_modules[0].clone(); MAX_MEMORY_MODULES + 1];
        assert!(validate(&value, value.collected_at).is_err());
        let mut value = h;
        value.devices = vec![value.devices[0].clone(); MAX_HARDWARE_DEVICES + 1];
        assert!(validate(&value, value.collected_at).is_err());
    }
}
