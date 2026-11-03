//! Taxonomie : niveaux NIST SP 800-88, méthodes d'effacement, capacités et
//! état d'une cible.
//!
//! Les niveaux attribués aux méthodes suivent l'annexe A de la révision 1
//! (2014) par défaut, avec une politique alternative plus conservatrice
//! inspirée de la révision 2 pour les cas que celle-ci a rétrogradés
//! (Format NVM user-data erase, ATA Security Erase sur SSD).

use std::fmt;

/// Niveau de sanitisation NIST SP 800-88.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NistLevel {
    /// Techniques logiques contre une récupération simple (lecture directe).
    Clear,
    /// Techniques contre une récupération en laboratoire.
    Purge,
    /// Destruction physique : jamais exécutée par le logiciel, seulement recommandée.
    Destroy,
}

impl fmt::Display for NistLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Clear => "Clear",
            Self::Purge => "Purge",
            Self::Destroy => "Destroy",
        })
    }
}

/// Méthode d'effacement exécutable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EraseMethod {
    /// NVMe Sanitize, action Block Erase.
    NvmeSanitizeBlockErase,
    /// NVMe Sanitize, action Crypto Erase.
    NvmeSanitizeCryptoErase,
    /// NVMe Sanitize, action Overwrite (`passes` passes).
    NvmeSanitizeOverwrite {
        /// Passes (1 à 16).
        passes: u8,
    },
    /// NVMe Format NVM, SES = 2 (cryptographic erase).
    NvmeFormatCryptoErase,
    /// NVMe Format NVM, SES = 1 (user data erase).
    NvmeFormatUserDataErase,
    /// ATA SANITIZE BLOCK ERASE EXT.
    AtaSanitizeBlockErase,
    /// ATA SANITIZE CRYPTO SCRAMBLE EXT.
    AtaSanitizeCryptoScramble,
    /// ATA SANITIZE OVERWRITE EXT (une passe de zéros).
    AtaSanitizeOverwrite,
    /// ATA SECURITY ERASE UNIT, mode enhanced.
    AtaSecurityEraseEnhanced,
    /// ATA SECURITY ERASE UNIT, mode normal.
    AtaSecurityEraseNormal,
    /// Écriture depuis l'hôte (`O_DIRECT`) : `passes` passes pseudo-aléatoires
    /// recomputables, puis une passe de zéros si `final_zero`.
    HostOverwrite {
        /// Passes pseudo-aléatoires (1 à 16).
        passes: u8,
        /// Passe finale de zéros.
        final_zero: bool,
    },
}

impl EraseMethod {
    /// Commande firmware (vraie sanitisation par le disque) ?
    pub const fn is_firmware(self) -> bool {
        !matches!(self, Self::HostOverwrite { .. })
    }

    /// Utilise le Sanitize feature set ATA ?
    pub const fn is_ata_sanitize(self) -> bool {
        matches!(
            self,
            Self::AtaSanitizeBlockErase
                | Self::AtaSanitizeCryptoScramble
                | Self::AtaSanitizeOverwrite
        )
    }

    /// Utilise le Security feature set ATA ?
    pub const fn is_ata_security_erase(self) -> bool {
        matches!(
            self,
            Self::AtaSecurityEraseEnhanced | Self::AtaSecurityEraseNormal
        )
    }

    /// Commande NVMe ?
    pub const fn is_nvme(self) -> bool {
        matches!(
            self,
            Self::NvmeSanitizeBlockErase
                | Self::NvmeSanitizeCryptoErase
                | Self::NvmeSanitizeOverwrite { .. }
                | Self::NvmeFormatCryptoErase
                | Self::NvmeFormatUserDataErase
        )
    }

    /// Un effacement cryptographique (la donnée reste, la clé disparaît).
    pub const fn is_cryptographic(self) -> bool {
        matches!(
            self,
            Self::NvmeSanitizeCryptoErase
                | Self::NvmeFormatCryptoErase
                | Self::AtaSanitizeCryptoScramble
        )
    }

    /// Libellé stable pour les rapports et certificats.
    pub fn label(self) -> String {
        match self {
            Self::NvmeSanitizeBlockErase => "NVME_SANITIZE_BLOCK_ERASE".into(),
            Self::NvmeSanitizeCryptoErase => "NVME_SANITIZE_CRYPTO_ERASE".into(),
            Self::NvmeSanitizeOverwrite { passes } => format!("NVME_SANITIZE_OVERWRITE_{passes}"),
            Self::NvmeFormatCryptoErase => "NVME_FORMAT_SES2_CRYPTO_ERASE".into(),
            Self::NvmeFormatUserDataErase => "NVME_FORMAT_SES1_USER_DATA_ERASE".into(),
            Self::AtaSanitizeBlockErase => "ATA_SANITIZE_BLOCK_ERASE".into(),
            Self::AtaSanitizeCryptoScramble => "ATA_SANITIZE_CRYPTO_SCRAMBLE".into(),
            Self::AtaSanitizeOverwrite => "ATA_SANITIZE_OVERWRITE".into(),
            Self::AtaSecurityEraseEnhanced => "ATA_SECURITY_ERASE_ENHANCED".into(),
            Self::AtaSecurityEraseNormal => "ATA_SECURITY_ERASE".into(),
            Self::HostOverwrite { passes, final_zero } => {
                format!(
                    "HOST_OVERWRITE_{passes}{}",
                    if final_zero { "_ZERO" } else { "" }
                )
            }
        }
    }
}

/// Type de média.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaType {
    /// Disque rotatif.
    Hdd,
    /// Mémoire flash (SATA, NVMe).
    Ssd,
    /// Non rapporté par le disque : traité comme un SSD pour le choix de la
    /// méthode (jamais d'overwrite firmware certifié Purge sans preuve).
    Unknown,
}

impl MediaType {
    /// Règles SSD applicables (Unknown compris).
    pub const fn solid_state_rules(self) -> bool {
        !matches!(self, Self::Hdd)
    }
}

/// Transport de la cible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transport {
    /// SATA natif.
    Sata,
    /// HBA SAS (SATL pilote).
    Sas,
    /// Pont USB (SATL du pont : commandes firmware non fiables).
    Usb,
    /// NVMe PCIe.
    Nvme,
    /// virtio-blk (aucune commande firmware).
    Virtio,
    /// Indéterminé.
    Other,
}

/// État du moteur de sanitize.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SanitizeState {
    /// Aucune opération en cours, dernière réussie ou aucune.
    #[default]
    Idle,
    /// Opération en cours.
    InProgress,
    /// Dernière opération échouée (mode Failure).
    Failed,
}

/// Capacités et état NVMe.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NvmeCaps {
    /// SANICAP.BES.
    pub sanitize_block_erase: bool,
    /// SANICAP.CES.
    pub sanitize_crypto_erase: bool,
    /// SANICAP.OWS.
    pub sanitize_overwrite: bool,
    /// OACS bit 1.
    pub format_supported: bool,
    /// FNA bit 2.
    pub format_crypto_erase: bool,
    /// FNA bit 1 : le secure erase du Format porte sur tous les namespaces.
    pub format_erases_all_namespaces: bool,
    /// Le contrôleur expose plus d'un namespace.
    pub multiple_namespaces: bool,
    /// Sanitize Status log.
    pub sanitize_state: SanitizeState,
    /// Critical Warning bit 3 : média en lecture seule.
    pub read_only: bool,
}

/// Capacités et état ATA.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AtaCaps {
    /// Mot 59 bit 12.
    pub sanitize_supported: bool,
    /// BLOCK ERASE EXT.
    pub sanitize_block_erase: bool,
    /// CRYPTO SCRAMBLE EXT.
    pub sanitize_crypto_scramble: bool,
    /// OVERWRITE EXT.
    pub sanitize_overwrite: bool,
    /// Sanitize Freeze Lock actif.
    pub sanitize_frozen: bool,
    /// SANITIZE STATUS EXT.
    pub sanitize_state: SanitizeState,
    /// Mot 128 bit 0.
    pub security_supported: bool,
    /// Mot 128 bit 1 : un mot de passe est posé.
    pub security_enabled: bool,
    /// Mot 128 bit 2.
    pub security_locked: bool,
    /// Mot 128 bit 3 : FREEZE LOCK (dégel par suspend-to-RAM).
    pub security_frozen: bool,
    /// Mot 128 bit 4.
    pub security_attempts_expired: bool,
    /// Mot 128 bit 5.
    pub enhanced_erase_supported: bool,
    /// Estimation du disque pour l'erase normal, en minutes.
    pub security_erase_minutes: Option<u32>,
    /// Estimation pour l'enhanced erase, en minutes.
    pub enhanced_erase_minutes: Option<u32>,
}

/// Protections système : si l'une est vraie, la cible est refusée.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ProtectionFlags {
    /// Héberge `/`, `/boot` ou l'ESP.
    pub hosts_system: bool,
    /// Une partition est montée.
    pub mounted: bool,
    /// Swap actif.
    pub swap: bool,
    /// Membre LVM / md / dm.
    pub held: bool,
}

impl ProtectionFlags {
    /// Au moins une protection.
    pub const fn any(self) -> bool {
        self.hosts_system || self.mounted || self.swap || self.held
    }
}

/// Indicateurs de santé pour la recommandation de destruction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct HealthFacts {
    /// SMART RETURN STATUS : seuil dépassé (ATA).
    pub smart_failing: bool,
    /// Attribut 5.
    pub reallocated_sectors: Option<u64>,
    /// Attribut 197.
    pub pending_sectors: Option<u64>,
    /// Attribut 198.
    pub offline_uncorrectable: Option<u64>,
    /// NVMe Critical Warning bit 2.
    pub nvme_reliability_degraded: bool,
    /// NVMe Critical Warning bit 0.
    pub nvme_spare_below_threshold: bool,
    /// NVMe Media and Data Integrity Errors.
    pub nvme_media_errors: u128,
    /// NVMe Percentage Used.
    pub nvme_percentage_used: Option<u8>,
}

/// Tout ce que le moteur sait d'une cible. Construit par le helper à partir
/// de `secmet-device` (voir `from_device` avec la feature `device`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TargetCapabilities {
    /// Nom pour les rapports (`sdb`, `nvme0n1`).
    pub name: String,
    /// Numéro de série, pour confirmation et certificat.
    pub serial: String,
    /// Média.
    pub media: MediaType,
    /// Transport.
    pub transport: Transport,
    /// Taille de bloc logique.
    pub block_size: u64,
    /// Nombre de blocs logiques.
    pub block_count: u64,
    /// Capacités NVMe (`Some` pour un namespace NVMe).
    pub nvme: Option<NvmeCaps>,
    /// Capacités ATA (`Some` pour un disque ATA/SATA, y compris derrière USB/SAS).
    pub ata: Option<AtaCaps>,
    /// Protections système.
    pub protection: ProtectionFlags,
    /// Santé.
    pub health: HealthFacts,
    /// L'inspection firmware a échoué : aucune commande firmware possible.
    pub inspection_failed: bool,
}

impl TargetCapabilities {
    /// Capacité en octets.
    pub fn capacity_bytes(&self) -> u64 {
        self.block_size.saturating_mul(self.block_count)
    }
}

/// Politique de classification : à quel niveau NIST chaque méthode est
/// certifiée. Versionnée, écrite dans chaque rapport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Policy {
    /// Identifiant de la politique (`nist-800-88r1`, `nist-800-88r2`).
    pub id: &'static str,
    /// Format NVM, SES = 1.
    pub nvme_format_user_data_erase: NistLevel,
    /// Format NVM, SES = 2.
    pub nvme_format_crypto_erase: NistLevel,
    /// Security Erase enhanced sur SSD.
    pub ata_security_erase_enhanced_ssd: NistLevel,
    /// Security Erase normal sur SSD.
    pub ata_security_erase_normal_ssd: NistLevel,
    /// Security Erase (normal ou enhanced) sur HDD.
    pub ata_security_erase_hdd: NistLevel,
    /// Attribut 5 au-delà duquel la destruction est recommandée.
    pub max_reallocated_sectors: u64,
    /// Blocs aléatoires lus à la validation.
    pub validation_samples: u32,
    /// Minimum accepté pour `validation_samples`.
    pub min_validation_samples: u32,
}

impl Policy {
    /// NIST SP 800-88 Rev. 1 (2014), annexe A : Format NVM SES 1 ou 2 et
    /// Security Erase sont Purge.
    pub const fn nist_800_88_rev1() -> Self {
        Self {
            id: "nist-800-88r1",
            nvme_format_user_data_erase: NistLevel::Purge,
            nvme_format_crypto_erase: NistLevel::Purge,
            ata_security_erase_enhanced_ssd: NistLevel::Purge,
            ata_security_erase_normal_ssd: NistLevel::Clear,
            ata_security_erase_hdd: NistLevel::Purge,
            max_reallocated_sectors: 50,
            validation_samples: 1000,
            min_validation_samples: 100,
        }
    }

    /// Lecture conservatrice (Rev. 2 / IEEE 2883) : Format user-data erase
    /// et Security Erase sur SSD retombent en Clear.
    pub const fn nist_800_88_rev2() -> Self {
        Self {
            id: "nist-800-88r2",
            nvme_format_user_data_erase: NistLevel::Clear,
            nvme_format_crypto_erase: NistLevel::Purge,
            ata_security_erase_enhanced_ssd: NistLevel::Clear,
            ata_security_erase_normal_ssd: NistLevel::Clear,
            ata_security_erase_hdd: NistLevel::Purge,
            ..Self::nist_800_88_rev1()
        }
    }

    /// Niveau certifié d'une méthode sur un média donné.
    pub fn level_of(&self, method: EraseMethod, media: MediaType) -> NistLevel {
        match method {
            EraseMethod::NvmeSanitizeBlockErase
            | EraseMethod::NvmeSanitizeCryptoErase
            | EraseMethod::NvmeSanitizeOverwrite { .. }
            | EraseMethod::AtaSanitizeBlockErase
            | EraseMethod::AtaSanitizeCryptoScramble
            | EraseMethod::AtaSanitizeOverwrite => NistLevel::Purge,
            EraseMethod::NvmeFormatCryptoErase => self.nvme_format_crypto_erase,
            EraseMethod::NvmeFormatUserDataErase => self.nvme_format_user_data_erase,
            EraseMethod::AtaSecurityEraseEnhanced if media.solid_state_rules() => {
                self.ata_security_erase_enhanced_ssd
            }
            EraseMethod::AtaSecurityEraseNormal if media.solid_state_rules() => {
                self.ata_security_erase_normal_ssd
            }
            EraseMethod::AtaSecurityEraseEnhanced | EraseMethod::AtaSecurityEraseNormal => {
                self.ata_security_erase_hdd
            }
            // Invariant 1 : une écriture hôte n'est jamais Purge.
            EraseMethod::HostOverwrite { .. } => NistLevel::Clear,
        }
    }
}

impl Default for Policy {
    fn default() -> Self {
        Self::nist_800_88_rev1()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn host_overwrite_is_never_purge_whatever_the_passes() {
        for policy in [Policy::nist_800_88_rev1(), Policy::nist_800_88_rev2()] {
            for passes in [1u8, 3, 7, 16] {
                for final_zero in [false, true] {
                    let method = EraseMethod::HostOverwrite { passes, final_zero };
                    for media in [MediaType::Hdd, MediaType::Ssd, MediaType::Unknown] {
                        assert_eq!(policy.level_of(method, media), NistLevel::Clear);
                    }
                }
            }
        }
    }

    #[test]
    fn sanitize_commands_are_purge_under_every_policy() {
        for policy in [Policy::nist_800_88_rev1(), Policy::nist_800_88_rev2()] {
            for method in [
                EraseMethod::NvmeSanitizeBlockErase,
                EraseMethod::NvmeSanitizeCryptoErase,
                EraseMethod::NvmeSanitizeOverwrite { passes: 1 },
                EraseMethod::AtaSanitizeBlockErase,
                EraseMethod::AtaSanitizeCryptoScramble,
                EraseMethod::AtaSanitizeOverwrite,
                EraseMethod::NvmeFormatCryptoErase,
            ] {
                assert_eq!(
                    policy.level_of(method, MediaType::Ssd),
                    NistLevel::Purge,
                    "{method:?}"
                );
            }
        }
    }

    #[test]
    fn rev1_and_rev2_differ_where_expected() {
        let r1 = Policy::nist_800_88_rev1();
        let r2 = Policy::nist_800_88_rev2();
        assert_eq!(
            r1.level_of(EraseMethod::NvmeFormatUserDataErase, MediaType::Ssd),
            NistLevel::Purge
        );
        assert_eq!(
            r2.level_of(EraseMethod::NvmeFormatUserDataErase, MediaType::Ssd),
            NistLevel::Clear
        );
        assert_eq!(
            r1.level_of(EraseMethod::AtaSecurityEraseEnhanced, MediaType::Ssd),
            NistLevel::Purge
        );
        assert_eq!(
            r2.level_of(EraseMethod::AtaSecurityEraseEnhanced, MediaType::Ssd),
            NistLevel::Clear
        );
        assert_eq!(
            r1.level_of(EraseMethod::AtaSecurityEraseNormal, MediaType::Ssd),
            NistLevel::Clear
        );
        // HDD : Security Erase reste Purge dans les deux lectures.
        for policy in [r1, r2] {
            assert_eq!(
                policy.level_of(EraseMethod::AtaSecurityEraseNormal, MediaType::Hdd),
                NistLevel::Purge
            );
            assert_eq!(
                policy.level_of(EraseMethod::AtaSecurityEraseEnhanced, MediaType::Hdd),
                NistLevel::Purge
            );
        }
        // Média inconnu : règles SSD.
        assert_eq!(
            r2.level_of(EraseMethod::AtaSecurityEraseEnhanced, MediaType::Unknown),
            NistLevel::Clear
        );
    }

    #[test]
    fn labels_and_predicates() {
        assert_eq!(
            EraseMethod::HostOverwrite {
                passes: 3,
                final_zero: true
            }
            .label(),
            "HOST_OVERWRITE_3_ZERO"
        );
        assert_eq!(
            EraseMethod::NvmeSanitizeOverwrite { passes: 2 }.label(),
            "NVME_SANITIZE_OVERWRITE_2"
        );
        assert!(!EraseMethod::HostOverwrite {
            passes: 1,
            final_zero: false
        }
        .is_firmware());
        assert!(EraseMethod::AtaSanitizeBlockErase.is_ata_sanitize());
        assert!(EraseMethod::AtaSecurityEraseNormal.is_ata_security_erase());
        assert!(
            EraseMethod::NvmeFormatCryptoErase.is_nvme()
                && EraseMethod::NvmeFormatCryptoErase.is_cryptographic()
        );
        assert!(NistLevel::Clear < NistLevel::Purge && NistLevel::Purge < NistLevel::Destroy);
        assert_eq!(NistLevel::Purge.to_string(), "Purge");
        assert!(ProtectionFlags {
            swap: true,
            ..ProtectionFlags::default()
        }
        .any());
    }
}
