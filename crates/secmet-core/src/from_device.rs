//! Conversion des structures matérielles de `secmet-device` vers la
//! taxonomie du moteur (feature `device`). Ce module ne fait aucune I/O :
//! il reçoit des valeurs déjà lues par le helper.

use secmet_device::ata::{
    identify::Media, IdentifyDevice, SanitizeStatus as AtaSanitizeStatus, SmartAttributes,
    SmartStatus,
};
use secmet_device::nvme::{
    HealthLog, IdentifyController, IdentifyNamespace, SanitizeLog,
    SanitizeState as NvmeSanitizeState,
};
use secmet_device::protection::Protection;
use secmet_device::sysfs::Transport as DeviceTransport;

use crate::taxonomy::{
    AtaCaps, HealthFacts, MediaType, NvmeCaps, ProtectionFlags, SanitizeState, Transport,
};

impl From<DeviceTransport> for Transport {
    fn from(transport: DeviceTransport) -> Self {
        match transport {
            DeviceTransport::Sata => Self::Sata,
            DeviceTransport::Sas => Self::Sas,
            DeviceTransport::Usb => Self::Usb,
            DeviceTransport::Nvme => Self::Nvme,
            DeviceTransport::Virtio => Self::Virtio,
            DeviceTransport::Other => Self::Other,
        }
    }
}

impl From<Protection> for ProtectionFlags {
    fn from(p: Protection) -> Self {
        Self {
            hosts_system: p.hosts_system,
            mounted: p.mounted,
            swap: p.swap,
            held: p.held,
        }
    }
}

impl From<Media> for MediaType {
    fn from(media: Media) -> Self {
        match media {
            Media::SolidState => Self::Ssd,
            Media::Rotating(_) => Self::Hdd,
            Media::Unknown => Self::Unknown,
        }
    }
}

impl From<secmet_device::ata::SanitizeState> for SanitizeState {
    fn from(state: secmet_device::ata::SanitizeState) -> Self {
        match state {
            secmet_device::ata::SanitizeState::Idle
            | secmet_device::ata::SanitizeState::Completed => Self::Idle,
            secmet_device::ata::SanitizeState::InProgress => Self::InProgress,
            secmet_device::ata::SanitizeState::Failed => Self::Failed,
        }
    }
}

impl From<NvmeSanitizeState> for SanitizeState {
    fn from(state: NvmeSanitizeState) -> Self {
        match state {
            NvmeSanitizeState::NeverSanitized | NvmeSanitizeState::Completed => Self::Idle,
            NvmeSanitizeState::InProgress => Self::InProgress,
            NvmeSanitizeState::Failed => Self::Failed,
        }
    }
}

/// Capacités ATA depuis IDENTIFY et, si le Sanitize feature set est
/// supporté, SANITIZE STATUS EXT.
pub fn ata_caps(identify: &IdentifyDevice, sanitize: Option<AtaSanitizeStatus>) -> AtaCaps {
    AtaCaps {
        sanitize_supported: identify.sanitize.supported,
        sanitize_block_erase: identify.sanitize.block_erase,
        sanitize_crypto_scramble: identify.sanitize.crypto_scramble,
        sanitize_overwrite: identify.sanitize.overwrite,
        sanitize_frozen: sanitize.is_some_and(|s| s.frozen),
        sanitize_state: sanitize.map(|s| s.state.into()).unwrap_or_default(),
        security_supported: identify.security.supported,
        security_enabled: identify.security.enabled,
        security_locked: identify.security.locked,
        security_frozen: identify.security.frozen,
        security_attempts_expired: identify.security.attempts_expired,
        enhanced_erase_supported: identify.security.enhanced_erase,
        security_erase_minutes: identify.security_erase_minutes,
        enhanced_erase_minutes: identify.enhanced_erase_minutes,
    }
}

/// Capacités NVMe depuis Identify Controller, Identify Namespace, le journal
/// sanitize et la page de santé.
pub fn nvme_caps(
    controller: &IdentifyController,
    namespace: &IdentifyNamespace,
    sanitize: &SanitizeLog,
    health: &HealthLog,
) -> NvmeCaps {
    let _ = namespace;
    NvmeCaps {
        sanitize_block_erase: controller.sanitize.block_erase,
        sanitize_crypto_erase: controller.sanitize.crypto_erase,
        sanitize_overwrite: controller.sanitize.overwrite,
        format_supported: controller.format.supported,
        format_crypto_erase: controller.format.crypto_erase,
        format_erases_all_namespaces: controller.format.erase_all_namespaces,
        multiple_namespaces: controller.namespace_count > 1,
        sanitize_state: sanitize.state.into(),
        read_only: health.read_only(),
    }
}

/// Santé ATA depuis SMART.
pub fn ata_health(status: Option<SmartStatus>, attributes: Option<SmartAttributes>) -> HealthFacts {
    let a = attributes.unwrap_or_default();
    HealthFacts {
        smart_failing: status == Some(SmartStatus::Failing),
        reallocated_sectors: a.reallocated_sectors,
        pending_sectors: a.pending_sectors,
        offline_uncorrectable: a.offline_uncorrectable,
        ..HealthFacts::default()
    }
}

/// Santé NVMe depuis la page SMART / Health.
pub fn nvme_health(health: &HealthLog) -> HealthFacts {
    HealthFacts {
        nvme_reliability_degraded: health.reliability_degraded(),
        nvme_spare_below_threshold: health.critical_warning & 1 != 0,
        nvme_media_errors: health.media_errors,
        nvme_percentage_used: Some(health.percentage_used),
        ..HealthFacts::default()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn transports_protections_and_states_map_one_to_one() {
        assert_eq!(Transport::from(DeviceTransport::Usb), Transport::Usb);
        assert_eq!(Transport::from(DeviceTransport::Other), Transport::Other);
        let p = Protection {
            hosts_system: true,
            mounted: false,
            swap: true,
            held: false,
        };
        let flags = ProtectionFlags::from(p);
        assert!(flags.hosts_system && flags.swap && !flags.mounted && !flags.held);
        assert_eq!(MediaType::from(Media::Rotating(7200)), MediaType::Hdd);
        assert_eq!(MediaType::from(Media::Unknown), MediaType::Unknown);
        assert_eq!(
            SanitizeState::from(NvmeSanitizeState::Completed),
            SanitizeState::Idle
        );
        assert_eq!(
            SanitizeState::from(NvmeSanitizeState::Failed),
            SanitizeState::Failed
        );
        assert_eq!(
            SanitizeState::from(secmet_device::ata::SanitizeState::InProgress),
            SanitizeState::InProgress
        );
    }

    #[test]
    fn ata_health_from_smart() {
        let h = ata_health(
            Some(SmartStatus::Failing),
            Some(SmartAttributes {
                reallocated_sectors: Some(3),
                ..SmartAttributes::default()
            }),
        );
        assert!(h.smart_failing);
        assert_eq!(h.reallocated_sectors, Some(3));
        assert!(!ata_health(None, None).smart_failing);
    }
}
