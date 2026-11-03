//! Ré-identification d'un disque et construction des capacités du moteur.
//!
//! Le chemin reçu de l'UI ne sert qu'à ouvrir un nœud ; tout ce qui compte
//! est relu derrière le descripteur ouvert : numéro de périphérique
//! (`fstat`), IDENTIFY DEVICE ou Identify Controller/Namespace, protections
//! (`mountinfo`, `swaps`, `holders/`). Le résultat est une [`DiskIdentity`]
//! comparable au jeton de session et une
//! [`TargetCapabilities`](secmet_core::taxonomy::TargetCapabilities) pour le
//! moteur de décision.

use std::fs::File;
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;

use secmet_core::from_device;
use secmet_core::taxonomy::{HealthFacts, MediaType, ProtectionFlags, TargetCapabilities};
use secmet_device::ata::AtaDevice;
use secmet_device::block::{BlockDevice, OpenMode};
use secmet_device::nvme::NvmeDevice;
use secmet_device::protection::{self, DevNum};
use secmet_device::sysfs::{self, DiskEntry, SysfsPaths, Transport};
use secmet_device::DeviceError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{HelperError, IsolationError, Result};
use crate::session::DiskIdentity;

/// Une entrée de la table SMART ATA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtaSmartAttribute {
    /// Identifiant.
    pub id: u8,
    /// Drapeaux.
    pub flags: u16,
    /// Valeur normalisée.
    pub value: u8,
    /// Pire valeur.
    pub worst: u8,
    /// Seuil, si connu.
    pub threshold: Option<u8>,
    /// Valeur brute (48 bits).
    pub raw: u64,
}

/// SMART ATA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AtaSmart {
    /// SMART RETURN STATUS : `Some(false)` = seuil franchi.
    pub passed: Option<bool>,
    /// Table complète.
    pub attributes: Vec<AtaSmartAttribute>,
}

/// Page SMART / Health NVMe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NvmeSmart {
    /// Critical Warning.
    pub critical_warning: u8,
    /// Température composite, °C.
    pub temperature_celsius: i32,
    /// Réserve disponible, %.
    pub available_spare: u8,
    /// Seuil de réserve, %.
    pub available_spare_threshold: u8,
    /// Percentage Used.
    pub percentage_used: u8,
    /// Data Units Read (×512 000 octets).
    pub data_units_read: u64,
    /// Data Units Written.
    pub data_units_written: u64,
    /// Host Read Commands.
    pub host_reads: u64,
    /// Host Write Commands.
    pub host_writes: u64,
    /// Controller Busy Time, minutes.
    pub controller_busy_minutes: u64,
    /// Power Cycles.
    pub power_cycles: u64,
    /// Power On Hours.
    pub power_on_hours: u64,
    /// Unsafe Shutdowns.
    pub unsafe_shutdowns: u64,
    /// Media and Data Integrity Errors.
    pub media_errors: u64,
    /// Error Information Log Entries.
    pub error_log_entries: u64,
    /// Warning Composite Temperature Time, minutes.
    pub warning_temp_minutes: u32,
    /// Critical Composite Temperature Time, minutes.
    pub critical_temp_minutes: u32,
}

/// Instantané SMART d'un disque, pour le panneau d'information de l'UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SmartSnapshot {
    /// ATA (SATA, SAS-SATL, USB-SAT).
    pub ata: Option<AtaSmart>,
    /// NVMe.
    pub nvme: Option<NvmeSmart>,
}

fn clamp(v: u128) -> u64 {
    u64::try_from(v).unwrap_or(u64::MAX)
}

/// Résultat d'une inspection.
#[derive(Debug, Clone)]
pub struct Scan {
    /// Identité liée aux jetons.
    pub identity: DiskIdentity,
    /// Capacités pour le moteur.
    pub caps: TargetCapabilities,
    /// Entrée sysfs (transport, partitions, taille).
    pub entry: DiskEntry,
    /// Modèle (firmware si inspecté, sinon sysfs).
    pub model: String,
    /// Révision firmware.
    pub firmware: String,
    /// Pourquoi l'inspection firmware a échoué, le cas échéant.
    pub inspection_error: Option<String>,
    /// SMART, pour l'affichage.
    pub smart: SmartSnapshot,
    /// Vitesse de rotation (IDENTIFY mot 217), `Some(0)` pour un SSD.
    pub rpm: Option<u16>,
}

/// SHA-256 hexadécimal des champs d'identité, séparés par `\0`.
pub fn fingerprint(
    model: &str,
    serial: &str,
    firmware: &str,
    wwn: Option<&str>,
    size_bytes: u64,
    block_size: u64,
) -> String {
    let mut hasher = Sha256::new();
    for part in [model, serial, firmware, wwn.unwrap_or("")] {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    hasher.update(size_bytes.to_le_bytes());
    hasher.update(block_size.to_le_bytes());
    crate::polkit::hex(&hasher.finalize())
}

/// Numéro de périphérique d'un descripteur ouvert (`st_rdev`).
pub fn devnum_of(file: &File) -> io::Result<DevNum> {
    let meta = file.metadata()?;
    if !meta.file_type().is_block_device() {
        return Err(io::Error::other("pas un périphérique bloc"));
    }
    Ok(DevNum::from_rdev(meta.rdev()))
}

/// Numéro de périphérique d'un nœud, sans suivre de lien.
pub fn devnum_of_path(path: &Path) -> io::Result<DevNum> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.file_type().is_block_device() {
        return Err(io::Error::other("pas un périphérique bloc"));
    }
    Ok(DevNum::from_rdev(meta.rdev()))
}

/// Protections fraîches du disque `devnum`. Un montage intraçable vaut refus.
pub fn fresh_protection(paths: &SysfsPaths, devnum: DevNum) -> Result<ProtectionFlags> {
    match protection::protection_of(paths, devnum) {
        Ok(p) => Ok(ProtectionFlags::from(p)),
        Err(DeviceError::Untraceable(detail)) => Err(IsolationError::Busy(format!(
            "montage ou swap non traçable, tout est considéré protégé: {detail}"
        ))
        .into()),
        Err(e) => Err(e.into()),
    }
}

/// Inspecte le disque derrière `device` (déjà ouvert, quel que soit le mode).
pub fn inspect(paths: &SysfsPaths, device: &BlockDevice) -> Result<Scan> {
    let name = device.name().to_string();
    let entry = sysfs::read_disk(paths, &name)?;
    let devnum = devnum_of(device.file())
        .map_err(|e| HelperError::io(format!("fstat {}", device.path().display()), e))?;
    if (entry.major, entry.minor) != (devnum.major, devnum.minor) {
        // Le nœud ouvert n'est pas le disque sysfs du même nom (nœud recréé
        // ou renommé entre-temps) : rien de ce que dit sysfs ne lui correspond.
        return Err(IsolationError::IdentityMismatch {
            field: "devnum",
            expected: format!("{}:{}", entry.major, entry.minor),
            found: devnum.to_string(),
        }
        .into());
    }
    let protection = fresh_protection(paths, devnum)?;
    let block_size = device.block_size();
    let block_count = device.block_count();
    let size_bytes = device.size_bytes();

    let mut caps = TargetCapabilities {
        name: name.clone(),
        serial: String::new(),
        media: match entry.rotational {
            Some(true) => MediaType::Hdd,
            Some(false) => MediaType::Ssd,
            None => MediaType::Unknown,
        },
        transport: entry.transport.into(),
        block_size,
        block_count,
        nvme: None,
        ata: None,
        protection,
        health: HealthFacts::default(),
        inspection_failed: false,
    };
    let mut model = entry.model.clone().unwrap_or_default();
    let mut firmware = entry.firmware.clone().unwrap_or_default();
    let mut serial = String::new();
    let mut print = String::new();
    let mut inspection_error = None;
    let mut smart = SmartSnapshot::default();
    let mut rpm = None;

    let firmware_identity = if device.is_nvme() {
        inspect_nvme(device, &mut caps, &mut smart)
    } else {
        inspect_ata(device, entry.transport, &mut caps, &mut smart)
    };
    match firmware_identity {
        Ok(found) => {
            model = found.model;
            firmware = found.firmware;
            serial = found.serial;
            rpm = found.rpm;
            print = fingerprint(
                &model,
                &serial,
                &firmware,
                found.wwn.as_deref(),
                size_bytes,
                block_size,
            );
        }
        Err(error) => {
            caps.inspection_failed = true;
            caps.nvme = None;
            caps.ata = None;
            inspection_error = Some(error.to_string());
        }
    }
    if serial.is_empty() {
        // Sans firmware : S/N sysfs (VPD 80 ou attribut), sinon WWID, sinon rien.
        serial = entry
            .serial
            .clone()
            .or_else(|| entry.wwid.clone())
            .unwrap_or_default();
        print = fingerprint(
            &format!("sysfs:{model}"),
            &serial,
            &firmware,
            entry.wwid.as_deref(),
            size_bytes,
            block_size,
        );
    }
    caps.serial = serial.clone();
    Ok(Scan {
        identity: DiskIdentity {
            name,
            major: devnum.major,
            minor: devnum.minor,
            serial,
            fingerprint: print,
        },
        caps,
        entry,
        model,
        firmware,
        inspection_error,
        smart,
        rpm,
    })
}

struct FirmwareIdentity {
    model: String,
    serial: String,
    firmware: String,
    wwn: Option<String>,
    rpm: Option<u16>,
}

fn inspect_nvme(
    device: &BlockDevice,
    caps: &mut TargetCapabilities,
    smart: &mut SmartSnapshot,
) -> Result<FirmwareIdentity> {
    let nvme = NvmeDevice::new(device.file())?;
    let controller = nvme.identify_controller()?;
    let namespace = nvme.identify_namespace()?;
    // Les journaux sont optionnels : un contrôleur sans page sanitize n'est
    // pas un contrôleur inconnu, il n'a simplement pas de sanitize.
    let sanitize = nvme
        .sanitize_log()
        .unwrap_or(secmet_device::nvme::SanitizeLog {
            progress: 0,
            state: secmet_device::nvme::SanitizeState::NeverSanitized,
            overwrite_passes_done: 0,
            global_data_erased: false,
        });
    // La page SMART / Health est obligatoire (NVMe 1.0+).
    let health = nvme.health()?;
    caps.nvme = Some(from_device::nvme_caps(
        &controller,
        &namespace,
        &sanitize,
        &health,
    ));
    caps.health = from_device::nvme_health(&health);
    caps.media = MediaType::Ssd;
    smart.nvme = Some(NvmeSmart {
        critical_warning: health.critical_warning,
        temperature_celsius: health.composite_temperature_celsius(),
        available_spare: health.available_spare,
        available_spare_threshold: health.available_spare_threshold,
        percentage_used: health.percentage_used,
        data_units_read: clamp(health.data_units_read),
        data_units_written: clamp(health.data_units_written),
        host_reads: clamp(health.host_reads),
        host_writes: clamp(health.host_writes),
        controller_busy_minutes: clamp(health.controller_busy_minutes),
        power_cycles: clamp(health.power_cycles),
        power_on_hours: clamp(health.power_on_hours),
        unsafe_shutdowns: clamp(health.unsafe_shutdowns),
        media_errors: clamp(health.media_errors),
        error_log_entries: clamp(health.error_log_entries),
        warning_temp_minutes: health.warning_temp_minutes,
        critical_temp_minutes: health.critical_temp_minutes,
    });
    Ok(FirmwareIdentity {
        model: controller.model.clone(),
        serial: controller.serial.clone(),
        firmware: controller.firmware.clone(),
        wwn: namespace.nguid.clone(),
        rpm: Some(0),
    })
}

fn inspect_ata(
    device: &BlockDevice,
    transport: Transport,
    caps: &mut TargetCapabilities,
    smart: &mut SmartSnapshot,
) -> Result<FirmwareIdentity> {
    let ata = AtaDevice::for_transport(device.file(), transport);
    let identify = ata.identify()?;
    let sanitize = if identify.sanitize.supported {
        ata.sanitize_status().ok()
    } else {
        None
    };
    let smart_status = if identify.smart_enabled {
        ata.smart_status().ok()
    } else {
        None
    };
    let smart_attributes = if identify.smart_enabled {
        ata.smart_attributes().ok()
    } else {
        None
    };
    if identify.smart_enabled {
        smart.ata = Some(AtaSmart {
            passed: smart_status.map(|s| s == secmet_device::ata::SmartStatus::Passed),
            attributes: ata
                .smart_attribute_table()
                .unwrap_or_default()
                .into_iter()
                .map(|a| AtaSmartAttribute {
                    id: a.id,
                    flags: a.flags,
                    value: a.value,
                    worst: a.worst,
                    threshold: a.threshold,
                    raw: a.raw,
                })
                .collect(),
        });
    }
    caps.ata = Some(from_device::ata_caps(&identify, sanitize));
    caps.health = from_device::ata_health(smart_status, smart_attributes);
    // Mot 217 non rapporté (disques anciens) : garder la classification
    // sysfs (`rotational`) plutôt que « inconnu », qui appliquerait les règles
    // SSD (SECURITY ERASE normal classé Clear au lieu de Purge sur un HDD).
    let firmware_media: MediaType = identify.media.into();
    if firmware_media != MediaType::Unknown {
        caps.media = firmware_media;
    }
    Ok(FirmwareIdentity {
        model: identify.model.clone(),
        serial: identify.serial.clone(),
        firmware: identify.firmware.clone(),
        wwn: identify.wwn.clone(),
        rpm: match identify.media {
            secmet_device::ata::identify::Media::SolidState => Some(0),
            secmet_device::ata::identify::Media::Rotating(rpm) => Some(rpm),
            secmet_device::ata::identify::Media::Unknown => None,
        },
    })
}

/// Ouvre `spec` dans `mode` et l'inspecte.
pub fn scan(paths: &SysfsPaths, spec: &str, mode: OpenMode) -> Result<(BlockDevice, Scan)> {
    let device = BlockDevice::open(spec, mode)?;
    let scan = inspect(paths, &device)?;
    Ok((device, scan))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn fingerprint_is_stable_and_sensitive_to_every_field() {
        let base = fingerprint("M", "S", "F", Some("W"), 1000, 512);
        assert_eq!(base.len(), 64);
        assert_eq!(base, fingerprint("M", "S", "F", Some("W"), 1000, 512));
        assert_ne!(base, fingerprint("M2", "S", "F", Some("W"), 1000, 512));
        assert_ne!(base, fingerprint("M", "S2", "F", Some("W"), 1000, 512));
        assert_ne!(base, fingerprint("M", "S", "F2", Some("W"), 1000, 512));
        assert_ne!(base, fingerprint("M", "S", "F", None, 1000, 512));
        assert_ne!(base, fingerprint("M", "S", "F", Some("W"), 1001, 512));
        assert_ne!(base, fingerprint("M", "S", "F", Some("W"), 1000, 4096));
        // Le séparateur empêche les collisions par concaténation.
        assert_ne!(
            fingerprint("AB", "C", "", None, 0, 0),
            fingerprint("A", "BC", "", None, 0, 0)
        );
    }

    #[test]
    fn devnum_helpers_refuse_non_block_files() {
        let file = tempfile::NamedTempFile::new().unwrap();
        assert!(devnum_of(file.as_file()).is_err());
        assert!(devnum_of_path(file.path()).is_err());
        assert!(devnum_of_path(Path::new("/dev/null")).is_err());
    }

    #[test]
    fn scan_refuses_names_outside_the_allowlist() {
        let paths = SysfsPaths::default();
        assert!(matches!(
            scan(&paths, "/dev/loop0", OpenMode::ReadOnly),
            Err(HelperError::Device(DeviceError::RefusedName(_)))
        ));
        assert!(matches!(
            scan(&paths, "/etc/passwd", OpenMode::ReadOnly),
            Err(HelperError::Device(DeviceError::RefusedName(_)))
        ));
    }
}
