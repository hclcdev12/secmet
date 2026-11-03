//! Décodage d'IDENTIFY DEVICE (ACS-4 §7.12) : 256 mots little-endian, les
//! chaînes ayant les deux octets de chaque mot inversés.

use crate::{ascii_trimmed, require, DecodeError};

/// Taille d'IDENTIFY DEVICE.
pub const IDENTIFY_SIZE: usize = 512;

/// État du Security feature set (mot 128).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SecurityState {
    /// Bit 0 : feature set supporté.
    pub supported: bool,
    /// Bit 1 : un mot de passe utilisateur est posé.
    pub enabled: bool,
    /// Bit 2 : verrouillé.
    pub locked: bool,
    /// Bit 3 : gelé (FREEZE LOCK) ; dégel par suspend-to-RAM.
    pub frozen: bool,
    /// Bit 4 : compteur de tentatives expiré.
    pub attempts_expired: bool,
    /// Bit 5 : enhanced erase supporté.
    pub enhanced_erase: bool,
    /// Bit 8 : niveau de sécurité Maximum (sinon High).
    pub level_maximum: bool,
}

/// Capacités du Sanitize feature set (mot 59).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SanitizeCaps {
    /// Bit 12 : feature set supporté.
    pub supported: bool,
    /// Bit 15 : BLOCK ERASE EXT.
    pub block_erase: bool,
    /// Bit 14 : OVERWRITE EXT.
    pub overwrite: bool,
    /// Bit 13 : CRYPTO SCRAMBLE EXT.
    pub crypto_scramble: bool,
    /// Bit 11 : ANTIFREEZE LOCK EXT (ACS-4).
    pub antifreeze: bool,
}

/// Type de média (mot 217).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Media {
    /// Non rotatif (SSD) : mot 217 = 1.
    SolidState,
    /// Rotatif, vitesse en tr/min.
    Rotating(u16),
    /// Non rapporté (mot 217 = 0 ou réservé).
    Unknown,
}

/// IDENTIFY DEVICE, champs utiles à SecMet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifyDevice {
    /// Mots 10..20.
    pub serial: String,
    /// Mots 23..27.
    pub firmware: String,
    /// Mots 27..47.
    pub model: String,
    /// Mots 108..112 en hexadécimal, si le mot 87 bit 8 l'annonce.
    pub wwn: Option<String>,
    /// Secteurs adressables (48 bits si supporté, sinon 28 bits).
    pub sectors: u64,
    /// Taille de secteur logique en octets (mots 106, 117..119).
    pub logical_sector_bytes: u32,
    /// Capacité en octets.
    pub capacity_bytes: u64,
    /// Mot 217.
    pub media: Media,
    /// Mot 83 bit 10 : adressage 48 bits.
    pub lba48: bool,
    /// Mot 169 bit 0 : DATA SET MANAGEMENT (TRIM) supporté.
    pub trim_supported: bool,
    /// Mot 82 bit 0 / mot 85 bit 0.
    pub smart_supported: bool,
    /// SMART activé.
    pub smart_enabled: bool,
    /// Mot 59.
    pub sanitize: SanitizeCaps,
    /// Mot 128.
    pub security: SecurityState,
    /// Mot 89 : durée estimée de SECURITY ERASE UNIT normal, en minutes.
    pub security_erase_minutes: Option<u32>,
    /// Mot 90 : durée estimée de l'enhanced erase, en minutes.
    pub enhanced_erase_minutes: Option<u32>,
}

impl IdentifyDevice {
    /// Le disque est-il un SSD (ou un média inconnu traité comme tel) ?
    pub fn is_solid_state(&self) -> bool {
        matches!(self.media, Media::SolidState | Media::Unknown)
    }
}

/// « Plus de 508 minutes » (255 en format court) : borne haute inconnue, on
/// retient 36 heures pour dimensionner les délais.
const BEYOND_SHORT_FORMAT_MINUTES: u32 = 36 * 60;

/// Durée d'erase des mots 89/90 (ACS-3 §7.12.7.30) : bit 15 posé → bits 14:0
/// × 2 minutes ; sinon bits 7:0 × 2 minutes, 0 = inconnu, 255 = plus de 508.
pub fn erase_minutes(word: u16) -> Option<u32> {
    if word & 0x8000 != 0 {
        let value = u32::from(word & 0x7fff);
        return (value != 0).then_some(value * 2);
    }
    match word & 0x00ff {
        0 => None,
        0xff => Some(BEYOND_SHORT_FORMAT_MINUTES),
        value => Some(u32::from(value) * 2),
    }
}

/// Décode IDENTIFY DEVICE en vérifiant le mot d'intégrité quand il est présent.
pub fn decode(data: &[u8]) -> Result<IdentifyDevice, DecodeError> {
    require(data, IDENTIFY_SIZE, "ATA IDENTIFY DEVICE").map_err(|e| match e {
        crate::DeviceError::Decode(d) => d,
        _ => DecodeError::Invalid {
            structure: "ATA IDENTIFY DEVICE",
            reason: "tampon invalide",
        },
    })?;
    let word = |i: usize| u16::from_le_bytes([data[2 * i], data[2 * i + 1]]);
    let bit = |i: usize, b: u32| word(i) & (1 << b) != 0;

    // Mot 0 bit 15 : 1 = périphérique ATAPI, pas un disque ATA.
    if bit(0, 15) {
        return Err(DecodeError::Invalid {
            structure: "ATA IDENTIFY DEVICE",
            reason: "périphérique ATAPI (mot 0 bit 15)",
        });
    }
    // Mot 255 : signature A5h en bits 7:0, somme sur 512 octets nulle.
    if data[510] == 0xa5
        && data[..IDENTIFY_SIZE]
            .iter()
            .fold(0u8, |s, b| s.wrapping_add(*b))
            != 0
    {
        return Err(DecodeError::Checksum("ATA IDENTIFY DEVICE"));
    }

    let lba48 = bit(83, 10) && bit(86, 10);
    let sectors = if lba48 {
        (0..4).fold(0u64, |acc, i| acc | (u64::from(word(100 + i)) << (16 * i)))
    } else {
        u64::from(word(60)) | (u64::from(word(61)) << 16)
    };
    // Mot 106 : bit 14 posé et bit 15 effacé = valide ; bit 12 = taille
    // logique dans les mots 117..118 (en mots).
    let logical_sector_bytes = if word(106) & 0xc000 == 0x4000 && bit(106, 12) {
        // En mots (16 bits) : ×2 sur 64 bits, puis plausibilité (ACS : au
        // moins 512 octets ; au-delà de 1 Mio ou hors puissance de deux, le
        // champ est corrompu). Un disque fantaisiste ne doit pas faire
        // déborder le calcul.
        let words = u64::from(word(117)) | (u64::from(word(118)) << 16);
        let bytes = words * 2;
        if !(512..=1 << 20).contains(&bytes) || !bytes.is_power_of_two() {
            return Err(DecodeError::Invalid {
                structure: "ATA IDENTIFY DEVICE",
                reason: "taille de secteur logique invalide (mots 117-118)",
            });
        }
        bytes as u32
    } else {
        512
    };
    let wwn = bit(87, 8)
        .then(|| {
            (108..112)
                .map(|i| format!("{:04x}", word(i)))
                .collect::<String>()
        })
        .filter(|text| text.bytes().any(|b| b != b'0'));
    let media = match word(217) {
        0 => Media::Unknown,
        1 => Media::SolidState,
        rpm @ 0x0401..=0xfffe => Media::Rotating(rpm),
        _ => Media::Unknown,
    };

    Ok(IdentifyDevice {
        serial: swapped_ascii(&data[20..40]),
        firmware: swapped_ascii(&data[46..54]),
        model: swapped_ascii(&data[54..94]),
        wwn,
        sectors,
        logical_sector_bytes,
        capacity_bytes: sectors.saturating_mul(u64::from(logical_sector_bytes)),
        media,
        lba48,
        trim_supported: bit(169, 0),
        smart_supported: bit(82, 0),
        smart_enabled: bit(85, 0),
        sanitize: SanitizeCaps {
            supported: bit(59, 12),
            block_erase: bit(59, 12) && bit(59, 15),
            overwrite: bit(59, 12) && bit(59, 14),
            crypto_scramble: bit(59, 12) && bit(59, 13),
            antifreeze: bit(59, 12) && bit(59, 11),
        },
        security: SecurityState {
            supported: bit(128, 0),
            enabled: bit(128, 1),
            locked: bit(128, 2),
            frozen: bit(128, 3),
            attempts_expired: bit(128, 4),
            enhanced_erase: bit(128, 5),
            level_maximum: bit(128, 8),
        },
        security_erase_minutes: erase_minutes(word(89)),
        enhanced_erase_minutes: erase_minutes(word(90)),
    })
}

/// Chaîne ATA : octets inversés dans chaque mot, puis bourrage retiré.
fn swapped_ascii(bytes: &[u8]) -> String {
    let swapped: Vec<u8> = bytes.chunks_exact(2).flat_map(|p| [p[1], p[0]]).collect();
    ascii_trimmed(&swapped)
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Construit un IDENTIFY DEVICE mot par mot.
    pub(crate) struct Identify(pub [u16; 256]);

    impl Identify {
        pub(crate) fn new() -> Self {
            Self([0; 256])
        }

        pub(crate) fn set(mut self, index: usize, value: u16) -> Self {
            self.0[index] = value;
            self
        }

        pub(crate) fn text(mut self, first: usize, words: usize, text: &str) -> Self {
            let bytes = format!("{text:<width$}", width = 2 * words).into_bytes();
            for (offset, pair) in bytes.chunks_exact(2).enumerate() {
                self.0[first + offset] = u16::from_be_bytes([pair[0], pair[1]]);
            }
            self
        }

        pub(crate) fn bytes(&self) -> Vec<u8> {
            self.0.iter().flat_map(|w| w.to_le_bytes()).collect()
        }

        /// Pose le mot d'intégrité pour une somme valide.
        pub(crate) fn signed(self) -> Vec<u8> {
            let mut data = self.set(255, 0x00a5).bytes();
            let sum = data.iter().fold(0u8, |s, b| s.wrapping_add(*b));
            data[511] = 0u8.wrapping_sub(sum);
            data
        }
    }

    pub(crate) fn ssd() -> Identify {
        Identify::new()
            .text(10, 10, "S3Z9NB0K123456A")
            .text(23, 4, "RVT04B6Q")
            .text(27, 20, "Samsung SSD 860 EVO 500GB")
            .set(59, 0b1011 << 12) // Sanitize, crypto, block erase
            .set(82, 1)
            .set(83, 1 << 10)
            .set(85, 1)
            .set(86, 1 << 10)
            .set(87, 1 << 8)
            .set(100, 0x6030)
            .set(101, 0x3a38) // 976 773 168 secteurs
            .set(108, 0x5002)
            .set(109, 0x538e)
            .set(110, 0x4012)
            .set(111, 0x3456)
            .set(128, 0b10_1001) // supporté, gelé, enhanced erase
            .set(169, 1)
            .set(217, 1)
    }

    #[test]
    fn decodes_identify_device() {
        let id = decode(&ssd().signed()).expect("valide");
        assert_eq!(id.serial, "S3Z9NB0K123456A");
        assert_eq!(id.firmware, "RVT04B6Q");
        assert_eq!(id.model, "Samsung SSD 860 EVO 500GB");
        assert_eq!(id.wwn.as_deref(), Some("5002538e40123456"));
        assert_eq!(id.sectors, 976_773_168);
        assert_eq!(id.capacity_bytes, 976_773_168 * 512);
        assert_eq!(id.media, Media::SolidState);
        assert!(id.is_solid_state() && id.lba48 && id.trim_supported && id.smart_enabled);
        assert!(id.sanitize.supported && id.sanitize.block_erase && id.sanitize.crypto_scramble);
        assert!(!id.sanitize.overwrite);
        assert!(id.security.supported && id.security.frozen && id.security.enhanced_erase);
        assert!(!id.security.enabled && !id.security.locked);
    }

    #[test]
    fn sanitize_bits_need_the_feature_set_bit() {
        let id = decode(&ssd().set(59, 0b1110 << 12).signed()).expect("valide");
        assert!(!id.sanitize.supported);
        assert_eq!(id.sanitize, SanitizeCaps::default());
    }

    #[test]
    fn reads_erase_time_estimates() {
        let id = decode(&ssd().set(89, 0x0019).set(90, 0x8000 | 300).signed()).expect("valide");
        assert_eq!(id.security_erase_minutes, Some(50));
        assert_eq!(id.enhanced_erase_minutes, Some(600));
        assert_eq!(
            decode(&ssd().signed())
                .expect("valide")
                .security_erase_minutes,
            None
        );
        assert_eq!(erase_minutes(0x00ff), Some(36 * 60));
    }

    #[test]
    fn absurd_logical_sector_size_is_rejected_without_overflow() {
        // Trouvé par fuzzing : mots 117-118 à 0xffff faisaient déborder
        // « 2 × taille » sur 32 bits.
        let huge = ssd()
            .set(106, 0x4000 | 1 << 12)
            .set(117, 0xffff)
            .set(118, 0xffff);
        assert!(matches!(
            decode(&huge.signed()),
            Err(DecodeError::Invalid { .. })
        ));
        // 1 536 octets : pas une puissance de deux, refusé.
        let odd = ssd().set(106, 0x4000 | 1 << 12).set(117, 768);
        assert!(decode(&odd.signed()).is_err());
        // 2 Mio : au-delà de la borne, refusé.
        let too_big = ssd().set(106, 0x4000 | 1 << 12).set(117, 0).set(118, 16);
        assert!(decode(&too_big.signed()).is_err());
    }

    #[test]
    fn rotation_rate_and_logical_sector_size() {
        let hdd = ssd()
            .set(217, 7200)
            .set(106, 0x4000 | 1 << 12)
            .set(117, 2048);
        let id = decode(&hdd.signed()).expect("valide");
        assert_eq!(id.media, Media::Rotating(7200));
        assert!(!id.is_solid_state());
        assert_eq!(id.logical_sector_bytes, 4096);
        assert_eq!(id.capacity_bytes, 976_773_168 * 4096);
        let unknown = decode(&ssd().set(217, 0).signed()).expect("valide");
        assert_eq!(unknown.media, Media::Unknown);
        assert!(
            unknown.is_solid_state(),
            "média inconnu traité comme SSD pour la méthode"
        );
    }

    #[test]
    fn lba28_fallback() {
        let old = ssd().set(83, 0).set(86, 0).set(60, 0x1234).set(61, 0x0001);
        let id = decode(&old.signed()).expect("valide");
        assert!(!id.lba48);
        assert_eq!(id.sectors, 0x0001_1234);
    }

    #[test]
    fn corrupted_or_atapi_identify_is_rejected() {
        let mut data = ssd().signed();
        data[200] ^= 0xff;
        assert_eq!(
            decode(&data),
            Err(DecodeError::Checksum("ATA IDENTIFY DEVICE"))
        );
        assert!(
            decode(&ssd().bytes()).is_ok(),
            "sans signature A5h, pas de somme"
        );
        assert!(decode(&ssd().set(0, 0x8000).signed()).is_err());
        assert!(decode(&[0; 100]).is_err());
    }
}
