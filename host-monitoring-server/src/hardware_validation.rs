use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use host_protocol::*;

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
        || h.disk_health.len() > MAX_HARDWARE_DISKS
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
