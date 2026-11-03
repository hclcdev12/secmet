//! Calcul de santé type QDiskInfo (Good / Caution / Bad / Unknown).

use crate::disk_info::smart_models::{
    AttrStatus, DiskProtocol, DiskSmartReport, HealthFlags, HealthPercent, HealthStatus,
    NvmeHealthLog, SmartAttributeRow,
};

/// Options alignées sur QDiskInfo (Ignore C4 activé par défaut).
#[derive(Debug, Clone)]
pub struct HealthOptions {
    /// Si true, l'attribut ATA 196 (C4 Reallocation Event Count) n'entraîne pas Caution.
    pub ignore_c4_reallocation_event_count: bool,
    /// Seuil NVMe `percentage_used` à partir duquel Caution (QDiskInfo : >= 90).
    pub nvme_percentage_used_caution: i64,
}

impl Default for HealthOptions {
    fn default() -> Self {
        Self {
            ignore_c4_reallocation_event_count: true,
            nvme_percentage_used_caution: 90,
        }
    }
}

/// Calcule l'état de santé global à partir du rapport déjà partiellement rempli.
pub fn compute_health(report: &DiskSmartReport, opts: &HealthOptions) -> HealthStatus {
    let flags = compute_health_flags(report, opts);
    compute_health_from_flags(report, &flags, opts)
}

pub fn compute_health_flags(report: &DiskSmartReport, opts: &HealthOptions) -> HealthFlags {
    let model_ok = !report.identity.model_name.is_empty();
    let status_known = report.smart_passed.is_some();
    let health_passed = report.smart_passed.unwrap_or(false);

    let (caution, bad) = match report.identity.protocol {
        DiskProtocol::Ata => ata_flags(&report.attributes, opts),
        DiskProtocol::Scsi => scsi_flags(report),
        DiskProtocol::Nvme => nvme_flags_from_entries(report, opts),
        DiskProtocol::Unknown => (false, false),
    };

    HealthFlags {
        health_passed,
        caution_flag: caution,
        bad_flag: bad,
        model_known: model_ok,
        smart_status_known: status_known,
    }
}

fn compute_health_from_flags(
    report: &DiskSmartReport,
    flags: &HealthFlags,
    opts: &HealthOptions,
) -> HealthStatus {
    if flags.health_passed && !flags.caution_flag && !flags.bad_flag {
        if is_excellent_candidate(report, opts) {
            HealthStatus::Excellent
        } else {
            HealthStatus::Good
        }
    } else if flags.health_passed && flags.caution_flag && !flags.bad_flag {
        HealthStatus::Caution
    } else if (flags.bad_flag || !flags.health_passed)
        && flags.model_known
        && flags.smart_status_known
    {
        HealthStatus::Bad
    } else {
        HealthStatus::Unknown
    }
}

fn is_excellent_candidate(report: &DiskSmartReport, opts: &HealthOptions) -> bool {
    match report.identity.protocol {
        DiskProtocol::Nvme => report
            .remaining_life_percent
            .map(|v| v >= 95)
            .unwrap_or(true),
        DiskProtocol::Ata => {
            let (caution, bad) = ata_flags(&report.attributes, opts);
            if caution || bad {
                return false;
            }
            if report.identity.is_ssd_like() {
                report
                    .remaining_life_percent
                    .map(|v| v >= 95)
                    .unwrap_or(true)
            } else {
                report.power_on_hours.unwrap_or(0) < 30_000
            }
        }
        DiskProtocol::Scsi => !report
            .scsi_errors
            .iter()
            .any(|e| e.total_uncorrected_errors != 0),
        DiskProtocol::Unknown => false,
    }
}

fn ata_flags(attrs: &[SmartAttributeRow], opts: &HealthOptions) -> (bool, bool) {
    let mut caution = false;
    let mut bad = false;

    for attr in attrs {
        if attr.threshold != 0 && attr.value < attr.threshold {
            bad = true;
        }
        let watch = matches!(attr.id, 5 | 197 | 198)
            || (attr.id == 196 && !opts.ignore_c4_reallocation_event_count);
        if watch && attr.raw_value > 0 {
            caution = true;
        }
    }
    (caution, bad)
}

fn scsi_flags(report: &DiskSmartReport) -> (bool, bool) {
    let mut caution = false;
    for err in &report.scsi_errors {
        if err.total_uncorrected_errors != 0 {
            caution = true;
        }
    }
    if report.scsi_grown_defect_list.unwrap_or(0) != 0 {
        caution = true;
    }
    (caution, false)
}

fn nvme_flags_from_entries(report: &DiskSmartReport, opts: &HealthOptions) -> (bool, bool) {
    let mut caution = false;
    let mut bad = false;

    let critical = report
        .nvme_entries
        .iter()
        .find(|e| e.key == "critical_warning")
        .map(|e| e.raw)
        .unwrap_or(0);
    if critical != 0 {
        bad = true;
    }

    let spare = report
        .nvme_entries
        .iter()
        .find(|e| e.key == "available_spare")
        .map(|e| e.raw);
    let spare_thresh = report
        .nvme_entries
        .iter()
        .find(|e| e.key == "available_spare_threshold")
        .map(|e| e.raw);

    if let (Some(raw), Some(thresh)) = (spare, spare_thresh) {
        if thresh > 100 {
            // workaround CrystalDiskInfo / QDiskInfo
        } else if raw == 0 && thresh == 0 {
            // ignore
        } else if raw < thresh {
            bad = true;
        } else if thresh != 100 && raw == thresh {
            caution = true;
        }
    }

    let pct_used = report
        .nvme_entries
        .iter()
        .find(|e| e.key == "percentage_used")
        .map(|e| e.raw)
        .unwrap_or(0);
    if pct_used >= opts.nvme_percentage_used_caution {
        caution = true;
    }

    (caution, bad)
}

/// Statut d'une ligne d'attribut ATA (pastille QDiskInfo).
pub fn ata_attr_status(
    attr_id: u32,
    value: i32,
    thresh: i32,
    raw: u64,
    opts: &HealthOptions,
) -> AttrStatus {
    if thresh != 0 && value < thresh {
        return AttrStatus::Bad;
    }
    let watch = matches!(attr_id, 5 | 197 | 198)
        || (attr_id == 196 && !opts.ignore_c4_reallocation_event_count);
    if watch && raw > 0 {
        return AttrStatus::Caution;
    }
    AttrStatus::Good
}

/// Statut d'une entrée NVMe nommée.
pub fn nvme_entry_status(
    key: &str,
    raw: i64,
    available_spare_threshold: Option<i64>,
    opts: &HealthOptions,
) -> AttrStatus {
    match key {
        "critical_warning" if raw != 0 => AttrStatus::Bad,
        "available_spare" => {
            let Some(thresh) = available_spare_threshold else {
                return AttrStatus::Good;
            };
            if thresh > 100 || (raw == 0 && thresh == 0) {
                AttrStatus::Good
            } else if raw < thresh {
                AttrStatus::Bad
            } else if thresh != 100 && raw == thresh {
                AttrStatus::Caution
            } else {
                AttrStatus::Good
            }
        }
        "percentage_used" if raw >= opts.nvme_percentage_used_caution => AttrStatus::Caution,
        _ => AttrStatus::Good,
    }
}

/// Vie restante SSD (%) d'après les attributs / log NVMe (logique QDiskInfo).
///
/// Sur HDD classique sans attribut wear : `None` (pas de % marketing inventé).
/// Matching par **nom** smartctl uniquement (comme QDiskInfo) — les IDs
/// vendor-specific « Unknown_Attribute » ne créent pas de faux %.
pub fn remaining_life_percent(
    protocol: DiskProtocol,
    attrs: &[SmartAttributeRow],
    nvme: Option<&NvmeHealthLog>,
    is_ssd_like: bool,
) -> Option<u32> {
    match protocol {
        DiskProtocol::Nvme => {
            let used = nvme?.percentage_used?;
            Some((100 - used).clamp(0, 100) as u32)
        }
        DiskProtocol::Ata => {
            // Comme QDiskInfo : le dernier attribut wear reconnu écrase les précédents.
            let mut percentage: Option<u32> = None;
            for attr in attrs {
                match attr.name.as_str() {
                    "Percent_Lifetime_Remain" if attr.id == 202 => {
                        percentage = Some((100i64 - attr.raw_value as i64).clamp(0, 100) as u32);
                    }
                    "Remaining_Lifetime_Perc" => {
                        percentage = Some(attr.raw_value.clamp(0, 100) as u32);
                    }
                    "Percent_Lifetime_Remain" | "Percent_Lifetime_Remaining" => {
                        percentage = Some(attr.value.clamp(0, 100) as u32);
                    }
                    "Media_Wearout_Indicator" | "SSD_Life_Left" if attr.worst <= 100 => {
                        percentage = Some(attr.value.clamp(0, 100) as u32);
                    }
                    _ => {}
                }
            }
            if percentage.is_none() && is_ssd_like {
                for attr in attrs {
                    if attr.name == "Wear_Leveling_Count" {
                        percentage = Some(attr.value.clamp(0, 100) as u32);
                        break;
                    }
                }
            }
            percentage
        }
        _ => None,
    }
}

/// Indice SecMet 0–100 dérivé des flags santé (pas d'usure constructeur).
///
/// Conservateur : `Unknown` → `None` (pas de faux positif).
/// Ne se base pas sur heures/cycles/température.
pub fn secmet_health_score(report: &DiskSmartReport) -> Option<u32> {
    match report.health {
        HealthStatus::Excellent => Some(100),
        HealthStatus::Good => Some(90),
        HealthStatus::Caution => Some(caution_score(report)),
        HealthStatus::Bad => Some(bad_score(report)),
        HealthStatus::Unknown => None,
    }
}

/// Wear QDiskInfo en priorité ; sinon indice SecMet si calculable.
pub fn resolve_health_percent(report: &DiskSmartReport) -> Option<HealthPercent> {
    if let Some(wear) = report.remaining_life_percent {
        return Some(HealthPercent::Wear(wear));
    }
    secmet_health_score(report).map(HealthPercent::SecMetScore)
}

fn attr_raw(attrs: &[SmartAttributeRow], id: u32) -> u64 {
    attrs
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.raw_value)
        .unwrap_or(0)
}

fn caution_score(report: &DiskSmartReport) -> u32 {
    // Preuves dures uniquement : pending/uncorrectable plus grave que realloc.
    let pending = attr_raw(&report.attributes, 197);
    let uncorrect = attr_raw(&report.attributes, 198);
    let realloc = attr_raw(&report.attributes, 5);

    if pending > 0 || uncorrect > 0 {
        return 60;
    }
    if report
        .scsi_errors
        .iter()
        .any(|e| e.total_uncorrected_errors != 0)
        || report.scsi_grown_defect_list.unwrap_or(0) != 0
    {
        return 60;
    }
    if realloc > 0 {
        return 70;
    }
    // NVMe spare == threshold / percentage_used caution, etc.
    65
}

fn bad_score(report: &DiskSmartReport) -> u32 {
    let mut score: u32 = 25;

    if report.smart_passed == Some(false) {
        score = score.min(20);
    }

    let pending = attr_raw(&report.attributes, 197);
    let uncorrect = attr_raw(&report.attributes, 198);
    if pending > 0 || uncorrect > 0 {
        score = score.min(15);
    }

    if report
        .attributes
        .iter()
        .any(|a| a.threshold != 0 && a.value < a.threshold)
    {
        score = score.min(25);
    }

    let critical = report
        .nvme_entries
        .iter()
        .find(|e| e.key == "critical_warning")
        .map(|e| e.raw)
        .unwrap_or(0);
    if critical != 0 {
        score = score.min(10);
    }

    let spare = report
        .nvme_entries
        .iter()
        .find(|e| e.key == "available_spare")
        .map(|e| e.raw);
    let spare_thresh = report
        .nvme_entries
        .iter()
        .find(|e| e.key == "available_spare_threshold")
        .map(|e| e.raw);
    if let (Some(raw), Some(thresh)) = (spare, spare_thresh) {
        if thresh <= 100 && !(raw == 0 && thresh == 0) && raw < thresh {
            score = score.min(15);
        }
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk_info::smart_models::{DiskIdentity, SmartAttributeRow};

    fn attr(id: u32, name: &str, value: i32, thresh: i32, raw: u64) -> SmartAttributeRow {
        SmartAttributeRow {
            id,
            name: name.to_string(),
            display_name: name.replace('_', " "),
            value,
            worst: value,
            threshold: thresh,
            raw_value: raw,
            raw_string: raw.to_string(),
            status: AttrStatus::Good,
        }
    }

    #[test]
    fn hdd_without_wear_has_no_life_percent() {
        let attrs = vec![
            attr(5, "Reallocated_Sector_Ct", 100, 10, 0),
            attr(9, "Power_On_Hours", 98, 0, 878),
            attr(12, "Power_Cycle_Count", 100, 0, 295),
        ];
        assert_eq!(
            remaining_life_percent(DiskProtocol::Ata, &attrs, None, false),
            None
        );
    }

    #[test]
    fn ssd_wear_leveling_gives_life_percent() {
        let attrs = vec![
            attr(5, "Reallocated_Sector_Ct", 100, 36, 54),
            attr(177, "Wear_Leveling_Count", 95, 0, 100),
        ];
        assert_eq!(
            remaining_life_percent(DiskProtocol::Ata, &attrs, None, true),
            Some(95)
        );
    }

    #[test]
    fn unknown_attr_id_alone_does_not_invent_life_percent() {
        // Même si l'ID 0xA9 est souvent « Remaining Lifetime », QDiskInfo ne
        // calcule le % que via le *nom* smartctl — pas via l'ID seul.
        let attrs = vec![attr(0xA9, "Unknown_Attribute", 91, 0, 106)];
        assert_eq!(
            remaining_life_percent(DiskProtocol::Ata, &attrs, None, true),
            None
        );
    }

    #[test]
    fn ata_excellent_when_passed_and_clean() {
        let report = DiskSmartReport {
            identity: DiskIdentity {
                model_name: "TEST".into(),
                protocol: DiskProtocol::Ata,
                ..Default::default()
            },
            smart_passed: Some(true),
            attributes: vec![attr(5, "Reallocated_Sector_Ct", 100, 10, 0)],
            ..Default::default()
        };
        assert_eq!(
            compute_health(&report, &HealthOptions::default()),
            HealthStatus::Excellent
        );
    }

    #[test]
    fn ata_caution_on_realloc_raw() {
        let report = DiskSmartReport {
            identity: DiskIdentity {
                model_name: "TEST".into(),
                protocol: DiskProtocol::Ata,
                ..Default::default()
            },
            smart_passed: Some(true),
            attributes: vec![attr(5, "Reallocated_Sector_Ct", 100, 10, 3)],
            ..Default::default()
        };
        assert_eq!(
            compute_health(&report, &HealthOptions::default()),
            HealthStatus::Caution
        );
    }

    #[test]
    fn ata_bad_when_value_below_thresh() {
        let report = DiskSmartReport {
            identity: DiskIdentity {
                model_name: "TEST".into(),
                protocol: DiskProtocol::Ata,
                ..Default::default()
            },
            smart_passed: Some(true),
            attributes: vec![attr(5, "Reallocated_Sector_Ct", 5, 10, 0)],
            ..Default::default()
        };
        assert_eq!(
            compute_health(&report, &HealthOptions::default()),
            HealthStatus::Bad
        );
    }

    #[test]
    fn ata_bad_when_smart_failed() {
        let report = DiskSmartReport {
            identity: DiskIdentity {
                model_name: "TEST".into(),
                protocol: DiskProtocol::Ata,
                ..Default::default()
            },
            smart_passed: Some(false),
            ..Default::default()
        };
        assert_eq!(
            compute_health(&report, &HealthOptions::default()),
            HealthStatus::Bad
        );
    }

    #[test]
    fn hdd_excellent_gets_secmet_indice_100() {
        let mut report = DiskSmartReport {
            identity: DiskIdentity {
                model_name: "HDD".into(),
                protocol: DiskProtocol::Ata,
                rotation_rate: Some(7200),
                ..Default::default()
            },
            smart_passed: Some(true),
            attributes: vec![attr(5, "Reallocated_Sector_Ct", 100, 10, 0)],
            remaining_life_percent: None,
            ..Default::default()
        };
        report.health = compute_health(&report, &HealthOptions::default());
        assert_eq!(report.health, HealthStatus::Excellent);
        assert_eq!(
            resolve_health_percent(&report),
            Some(HealthPercent::SecMetScore(100))
        );
    }

    #[test]
    fn wear_takes_priority_over_secmet_score() {
        let mut report = DiskSmartReport {
            identity: DiskIdentity {
                model_name: "SSD".into(),
                protocol: DiskProtocol::Ata,
                rotation_rate: Some(0),
                ..Default::default()
            },
            smart_passed: Some(true),
            remaining_life_percent: Some(95),
            health: HealthStatus::Excellent,
            ..Default::default()
        };
        report.health = HealthStatus::Excellent;
        assert_eq!(
            resolve_health_percent(&report),
            Some(HealthPercent::Wear(95))
        );
    }

    #[test]
    fn caution_realloc_scores_70() {
        let mut report = DiskSmartReport {
            identity: DiskIdentity {
                model_name: "HDD".into(),
                protocol: DiskProtocol::Ata,
                ..Default::default()
            },
            smart_passed: Some(true),
            attributes: vec![attr(5, "Reallocated_Sector_Ct", 100, 10, 3)],
            ..Default::default()
        };
        report.health = compute_health(&report, &HealthOptions::default());
        assert_eq!(report.health, HealthStatus::Caution);
        assert_eq!(secmet_health_score(&report), Some(70));
    }

    #[test]
    fn caution_pending_scores_60() {
        let mut report = DiskSmartReport {
            identity: DiskIdentity {
                model_name: "HDD".into(),
                protocol: DiskProtocol::Ata,
                ..Default::default()
            },
            smart_passed: Some(true),
            attributes: vec![attr(197, "Current_Pending_Sector", 100, 0, 2)],
            ..Default::default()
        };
        report.health = compute_health(&report, &HealthOptions::default());
        assert_eq!(report.health, HealthStatus::Caution);
        assert_eq!(secmet_health_score(&report), Some(60));
    }

    #[test]
    fn bad_failed_scores_at_most_20() {
        let mut report = DiskSmartReport {
            identity: DiskIdentity {
                model_name: "HDD".into(),
                protocol: DiskProtocol::Ata,
                ..Default::default()
            },
            smart_passed: Some(false),
            ..Default::default()
        };
        report.health = compute_health(&report, &HealthOptions::default());
        assert_eq!(report.health, HealthStatus::Bad);
        assert_eq!(secmet_health_score(&report), Some(20));
    }

    #[test]
    fn unknown_has_no_score() {
        let report = DiskSmartReport {
            health: HealthStatus::Unknown,
            ..Default::default()
        };
        assert_eq!(secmet_health_score(&report), None);
        assert_eq!(resolve_health_percent(&report), None);
    }
}
