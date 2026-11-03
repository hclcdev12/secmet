//! Construction du rapport SMART d'affichage depuis un `ScanDevice` du helper
//! (IDENTIFY + SMART READ DATA/THRESHOLDS, ou page Health NVMe).

use crate::disk_info::ata_attr_names::{ata_attr_display_name, effective_ata_attr_name};
use crate::disk_info::disk_health::{
    ata_attr_status, compute_health, compute_health_flags, nvme_entry_status,
    remaining_life_percent, resolve_health_percent, HealthOptions,
};
use crate::disk_info::smart_models::{
    DiskIdentity, DiskProtocol, DiskSmartReport, HealthStatus, NvmeHealthLog, NvmeSmartEntry,
    SmartAttributeRow, TemperatureInfo,
};
use crate::helper_client::{AtaSmart, NvmeSmart, ScanResult};

fn attr_raw(attrs: &[SmartAttributeRow], id: u32) -> Option<u64> {
    attrs.iter().find(|a| a.id == id).map(|a| a.raw_value)
}

fn title_case_underscores(key: &str) -> String {
    key.split('_')
        .filter(|s| !s.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn ata_rows(ata: &AtaSmart, opts: &HealthOptions) -> Vec<SmartAttributeRow> {
    ata.attributes
        .iter()
        .map(|a| {
            let id = u32::from(a.id);
            let thresh = i32::from(a.threshold.unwrap_or(0));
            // Température : les 16 bits bas portent la valeur courante.
            let raw_value = if matches!(id, 190 | 194) {
                a.raw & 0xffff
            } else {
                a.raw
            };
            SmartAttributeRow {
                id,
                name: effective_ata_attr_name(id, ""),
                display_name: ata_attr_display_name(id, ""),
                value: i32::from(a.value),
                worst: i32::from(a.worst),
                threshold: thresh,
                raw_value,
                raw_string: raw_value.to_string(),
                status: ata_attr_status(id, i32::from(a.value), thresh, raw_value, opts),
            }
        })
        .collect()
}

fn nvme_log(n: &NvmeSmart) -> NvmeHealthLog {
    let i = |v: u64| Some(i64::try_from(v).unwrap_or(i64::MAX));
    NvmeHealthLog {
        critical_warning: Some(i64::from(n.critical_warning)),
        temperature: Some(i64::from(n.temperature_celsius)),
        available_spare: Some(i64::from(n.available_spare)),
        available_spare_threshold: Some(i64::from(n.available_spare_threshold)),
        percentage_used: Some(i64::from(n.percentage_used)),
        data_units_read: Some(n.data_units_read as f64),
        data_units_written: Some(n.data_units_written as f64),
        host_reads: i(n.host_reads),
        host_writes: i(n.host_writes),
        controller_busy_time: i(n.controller_busy_minutes),
        power_cycles: i(n.power_cycles),
        power_on_hours: i(n.power_on_hours),
        unsafe_shutdowns: i(n.unsafe_shutdowns),
        media_errors: i(n.media_errors),
        num_err_log_entries: i(n.error_log_entries),
        warning_temp_time: Some(i64::from(n.warning_temp_minutes)),
        critical_comp_time: Some(i64::from(n.critical_temp_minutes)),
    }
}

fn nvme_rows(log: &NvmeHealthLog, opts: &HealthOptions) -> Vec<NvmeSmartEntry> {
    let pairs: Vec<(&str, Option<i64>)> = vec![
        ("critical_warning", log.critical_warning),
        ("temperature", log.temperature),
        ("available_spare", log.available_spare),
        ("available_spare_threshold", log.available_spare_threshold),
        ("percentage_used", log.percentage_used),
        ("data_units_read", log.data_units_read.map(|v| v as i64)),
        (
            "data_units_written",
            log.data_units_written.map(|v| v as i64),
        ),
        ("host_reads", log.host_reads),
        ("host_writes", log.host_writes),
        ("controller_busy_time", log.controller_busy_time),
        ("power_cycles", log.power_cycles),
        ("power_on_hours", log.power_on_hours),
        ("unsafe_shutdowns", log.unsafe_shutdowns),
        ("media_errors", log.media_errors),
        ("num_err_log_entries", log.num_err_log_entries),
        ("warning_temp_time", log.warning_temp_time),
        ("critical_comp_time", log.critical_comp_time),
    ];
    pairs
        .into_iter()
        .filter_map(|(key, val)| {
            let raw = val?;
            Some(NvmeSmartEntry {
                key: key.to_string(),
                display_name: title_case_underscores(key),
                raw,
                status: nvme_entry_status(key, raw, log.available_spare_threshold, opts),
            })
        })
        .collect()
}

/// Rapport d'affichage depuis un scan du helper.
pub fn report_from_scan(scan: &ScanResult) -> Result<DiskSmartReport, String> {
    report_from_scan_with_options(scan, &HealthOptions::default())
}

pub fn report_from_scan_with_options(
    scan: &ScanResult,
    opts: &HealthOptions,
) -> Result<DiskSmartReport, String> {
    let d = &scan.disk;
    if d.inspection_failed {
        return Err(d
            .inspection_error
            .clone()
            .unwrap_or_else(|| "inspection firmware impossible".to_string()));
    }
    let protocol = if d.smart.nvme.is_some() || d.transport == "nvme" {
        DiskProtocol::Nvme
    } else if d.smart.ata.is_some() || d.ata.is_some() {
        DiskProtocol::Ata
    } else {
        DiskProtocol::Unknown
    };
    let identity = DiskIdentity {
        model_name: d.model.clone(),
        serial_number: d.serial.clone(),
        firmware: if d.firmware.is_empty() {
            "----".to_string()
        } else {
            d.firmware.clone()
        },
        device_node: format!("/dev/{}", d.name),
        device_type: d.transport.to_uppercase(),
        protocol,
        capacity_bytes: (d.size_bytes > 0).then_some(d.size_bytes),
        rotation_rate: d.rpm.map(i64::from),
        logical_block_size: (d.block_size > 0).then_some(d.block_size),
    };
    let lbs = identity.logical_block_size.unwrap_or(512);

    let attributes = d
        .smart
        .ata
        .as_ref()
        .map(|a| ata_rows(a, opts))
        .unwrap_or_default();
    let nvme = d.smart.nvme.as_ref().map(nvme_log);
    let nvme_entries = nvme
        .as_ref()
        .map(|l| nvme_rows(l, opts))
        .unwrap_or_default();

    let smart_passed = match (&d.smart.ata, &nvme) {
        (Some(ata), _) => ata.passed,
        (None, Some(log)) => log.critical_warning.map(|w| w == 0),
        _ => None,
    };
    let temperature = TemperatureInfo {
        current_celsius: match protocol {
            DiskProtocol::Nvme => nvme.as_ref().and_then(|l| l.temperature).map(|t| t as i32),
            _ => attr_raw(&attributes, 194)
                .or_else(|| attr_raw(&attributes, 190))
                .map(|t| t as i32),
        },
        ..TemperatureInfo::default()
    };
    let power_on_hours = match protocol {
        DiskProtocol::Nvme => nvme
            .as_ref()
            .and_then(|l| l.power_on_hours)
            .map(|h| h as u64),
        _ => attr_raw(&attributes, 9).map(|h| h & 0xffff),
    };
    let power_cycle_count = match protocol {
        DiskProtocol::Nvme => nvme.as_ref().and_then(|l| l.power_cycles).map(|c| c as u64),
        _ => attr_raw(&attributes, 12),
    };
    let (total_reads_mb, total_writes_mb) = match protocol {
        DiskProtocol::Nvme => (
            nvme.as_ref()
                .and_then(|l| l.data_units_read)
                .map(|u| ((u * lbs as f64) / 1000.0) as i64),
            nvme.as_ref()
                .and_then(|l| l.data_units_written)
                .map(|u| ((u * lbs as f64) / 1000.0) as i64),
        ),
        _ => (
            attr_raw(&attributes, 242).map(|lbas| (lbas.saturating_mul(lbs) / 1_000_000) as i64),
            attr_raw(&attributes, 241).map(|lbas| (lbas.saturating_mul(lbs) / 1_000_000) as i64),
        ),
    };
    let life = remaining_life_percent(protocol, &attributes, nvme.as_ref(), identity.is_ssd_like());

    let mut report = DiskSmartReport {
        identity,
        smart_passed,
        health_flags: Default::default(),
        temperature,
        health: HealthStatus::Unknown,
        remaining_life_percent: life,
        health_percent: None,
        power_on_hours,
        power_cycle_count,
        total_reads_mb,
        total_writes_mb,
        attributes,
        nvme_entries,
        scsi_errors: Vec::new(),
        self_tests: Vec::new(),
        scsi_grown_defect_list: None,
        smartctl_output_lines: Vec::new(),
    };
    report.health_flags = compute_health_flags(&report, opts);
    report.health = compute_health(&report, opts);
    report.health_percent = resolve_health_percent(&report);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(disk: serde_json::Value) -> ScanResult {
        serde_json::from_value(serde_json::json!({
            "token": "t",
            "identity": {"name": "sdz", "major": 8, "minor": 0, "serial": "S", "fingerprint": "f"},
            "disk": disk,
            "verdict": {"state": "ready"}
        }))
        .unwrap()
    }

    #[test]
    fn ata_scan_becomes_a_report_with_attributes_and_health() {
        let s = scan(serde_json::json!({
            "name": "sdz", "model": "ST1000DM003", "serial": "Z1", "firmware": "CC43",
            "transport": "sata", "media": "hdd", "size_bytes": 1_000_204_886_016u64, "block_size": 512,
            "rpm": 7200, "protection": {}, "health": {"nvme_media_errors": "0"},
            "ata": {"security_frozen": false},
            "smart": {"ata": {"passed": true, "attributes": [
                {"id": 5, "flags": 51, "value": 100, "worst": 100, "threshold": 10, "raw": 0},
                {"id": 9, "flags": 50, "value": 90, "worst": 90, "threshold": 0, "raw": 12345},
                {"id": 194, "flags": 34, "value": 32, "worst": 45, "threshold": 0, "raw": 0x0021001f0020u64},
                {"id": 197, "flags": 18, "value": 100, "worst": 100, "threshold": 0, "raw": 2},
                {"id": 241, "flags": 48, "value": 100, "worst": 100, "threshold": 0, "raw": 2_000_000}
            ]}, "nvme": null}
        }));
        let report = report_from_scan(&s).unwrap();
        assert_eq!(report.identity.protocol, DiskProtocol::Ata);
        assert_eq!(report.identity.model_name, "ST1000DM003");
        assert_eq!(report.identity.rotation_rate, Some(7200));
        assert!(!report.identity.is_ssd_like());
        assert_eq!(report.smart_passed, Some(true));
        assert_eq!(report.temperature.current_celsius, Some(32));
        assert_eq!(report.power_on_hours, Some(12345));
        assert_eq!(report.total_writes_mb, Some(1024));
        assert_eq!(report.attributes.len(), 5);
        let pending = report.attributes.iter().find(|a| a.id == 197).unwrap();
        assert_eq!(pending.status, crate::disk_info::AttrStatus::Caution);
        assert!(pending.display_name.to_lowercase().contains("pending"));
        assert_eq!(report.health, HealthStatus::Caution);
        assert_eq!(report.remaining_life_percent, None);
    }

    #[test]
    fn nvme_scan_becomes_a_report_with_entries_and_life() {
        let s = scan(serde_json::json!({
            "name": "nvme0n1", "model": "Samsung SSD 970", "serial": "N1", "firmware": "1B2Q",
            "transport": "nvme", "media": "ssd", "size_bytes": 500_107_862_016u64, "block_size": 512,
            "rpm": 0, "protection": {}, "health": {"nvme_media_errors": "0"},
            "nvme": {"format_supported": true},
            "smart": {"ata": null, "nvme": {
                "critical_warning": 0, "temperature_celsius": 41, "available_spare": 100,
                "available_spare_threshold": 10, "percentage_used": 2, "data_units_read": 1000,
                "data_units_written": 2000, "host_reads": 1, "host_writes": 1, "controller_busy_minutes": 5,
                "power_cycles": 30, "power_on_hours": 800, "unsafe_shutdowns": 2, "media_errors": 0,
                "error_log_entries": 0, "warning_temp_minutes": 0, "critical_temp_minutes": 0}}
        }));
        let report = report_from_scan(&s).unwrap();
        assert_eq!(report.identity.protocol, DiskProtocol::Nvme);
        assert!(report.identity.is_ssd_like());
        assert_eq!(report.smart_passed, Some(true));
        assert_eq!(report.temperature.current_celsius, Some(41));
        assert_eq!(report.remaining_life_percent, Some(98));
        assert_eq!(report.power_cycle_count, Some(30));
        assert_eq!(report.total_writes_mb, Some(1024));
        assert!(report
            .nvme_entries
            .iter()
            .any(|e| e.key == "percentage_used"));
        assert_eq!(report.health, HealthStatus::Excellent);
    }

    #[test]
    fn failed_inspection_is_an_error() {
        let s = scan(serde_json::json!({
            "name": "sdz", "inspection_failed": true, "inspection_error": "SG_IO refusé",
            "protection": {}, "health": {"nvme_media_errors": "0"}, "smart": {}
        }));
        assert_eq!(report_from_scan(&s).unwrap_err(), "SG_IO refusé");
    }
}
