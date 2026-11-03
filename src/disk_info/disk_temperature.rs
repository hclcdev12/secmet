//! Seuils et couleurs de température (logique / seuils QDiskInfo).

use crate::disk_info::smart_models::{DiskProtocol, HealthStatus, TemperatureInfo};

/// Seuils d'alerte température (°C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemperatureThresholds {
    pub warning_celsius: i32,
    pub critical_celsius: i32,
}

impl TemperatureThresholds {
    /// Défauts QDiskInfo : ATA 55/60, NVMe 65/70.
    pub fn for_protocol(protocol: DiskProtocol) -> Self {
        match protocol {
            DiskProtocol::Nvme => Self {
                warning_celsius: 65,
                critical_celsius: 70,
            },
            _ => Self {
                warning_celsius: 55,
                critical_celsius: 60,
            },
        }
    }
}

/// Résout les seuils en tenant compte du JSON / sortie texte smartctl.
pub fn resolve_thresholds(
    protocol: DiskProtocol,
    temperature: &TemperatureInfo,
    smartctl_output_lines: &[String],
) -> TemperatureThresholds {
    let mut t = TemperatureThresholds::for_protocol(protocol);

    if protocol == DiskProtocol::Nvme {
        for line in smartctl_output_lines {
            if let Some(v) = parse_temp_threshold_line(line, "Warning  Comp. Temp. Threshold") {
                if v > 0 {
                    t.warning_celsius = v;
                }
            } else if let Some(v) =
                parse_temp_threshold_line(line, "Critical Comp. Temp. Threshold")
            {
                if v > 0 {
                    t.critical_celsius = v;
                }
            }
        }
    } else if protocol == DiskProtocol::Scsi {
        if let Some(trip) = temperature.drive_trip_celsius {
            if trip > 0 {
                t.critical_celsius = trip;
            }
        }
    }

    t
}

fn parse_temp_threshold_line(line: &str, prefix: &str) -> Option<i32> {
    let trimmed = line.trim();
    if !trimmed.starts_with(prefix) {
        return None;
    }
    let after = trimmed.split_once(':')?.1.trim();
    let first = after.split_whitespace().next()?;
    first.parse().ok()
}

/// Couleur / statut pour la température actuelle.
///
/// QDiskInfo a deux seuils (warning / critical) mais une branche « caution »
/// inatteignable dans le C++ (bug : premier `if` utilise déjà `warning`).
/// On applique l'intention CrystalDiskInfo/QDiskInfo :
/// - absente → Unknown
/// - ≤ warning → Good
/// - ≤ critical → Caution
/// - > critical → Bad
pub fn temperature_status(
    current: Option<i32>,
    thresholds: &TemperatureThresholds,
) -> HealthStatus {
    match current {
        None | Some(0) => HealthStatus::Unknown,
        Some(t) if t > thresholds.critical_celsius => HealthStatus::Bad,
        Some(t) if t > thresholds.warning_celsius => HealthStatus::Caution,
        Some(_) => HealthStatus::Good,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ata_defaults() {
        let t = TemperatureThresholds::for_protocol(DiskProtocol::Ata);
        assert_eq!(t.warning_celsius, 55);
        assert_eq!(t.critical_celsius, 60);
    }

    #[test]
    fn status_bands() {
        let t = TemperatureThresholds {
            warning_celsius: 55,
            critical_celsius: 60,
        };
        assert_eq!(temperature_status(Some(40), &t), HealthStatus::Good);
        assert_eq!(temperature_status(Some(56), &t), HealthStatus::Caution);
        assert_eq!(temperature_status(Some(61), &t), HealthStatus::Bad);
        assert_eq!(temperature_status(Some(0), &t), HealthStatus::Unknown);
    }

    #[test]
    fn parse_nvme_thresholds_from_output() {
        let lines = vec![
            "Warning  Comp. Temp. Threshold:    75 Celsius".to_string(),
            "Critical Comp. Temp. Threshold:   80 Celsius".to_string(),
        ];
        let resolved = resolve_thresholds(DiskProtocol::Nvme, &TemperatureInfo::default(), &lines);
        assert_eq!(resolved.warning_celsius, 75);
        assert_eq!(resolved.critical_celsius, 80);
    }
}
