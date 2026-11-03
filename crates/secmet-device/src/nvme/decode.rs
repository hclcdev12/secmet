//! Structures NVMe (NVM Express Base 2.0), offsets en octets, little-endian.

use crate::{ascii_trimmed, require, DecodeError};

/// Taille de toute structure Identify.
pub const IDENTIFY_SIZE: usize = 4096;
/// Taille de la page SMART / Health Information (02h).
pub const HEALTH_LOG_SIZE: usize = 512;
/// Taille lue de la page Sanitize Status (81h).
pub const SANITIZE_LOG_SIZE: usize = 20;

fn decode_err(e: crate::DeviceError, structure: &'static str) -> DecodeError {
    match e {
        crate::DeviceError::Decode(d) => d,
        _ => DecodeError::Invalid {
            structure,
            reason: "tampon invalide",
        },
    }
}

fn le_u16(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}

fn le_u32(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}

fn le_u64(d: &[u8], at: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[at..at + 8]);
    u64::from_le_bytes(b)
}

fn le_u128(d: &[u8], at: usize) -> u128 {
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[at..at + 16]);
    u128::from_le_bytes(b)
}

/// Identifiant hexadécimal, `None` s'il est entièrement nul.
fn hex_id(bytes: &[u8]) -> Option<String> {
    if bytes.iter().all(|&b| b == 0) {
        return None;
    }
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Capacités Sanitize (SANICAP, octets 328..332).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SanitizeCaps {
    /// Bit 0 : Crypto Erase.
    pub crypto_erase: bool,
    /// Bit 1 : Block Erase.
    pub block_erase: bool,
    /// Bit 2 : Overwrite.
    pub overwrite: bool,
    /// Bit 29 : No-Deallocate Inhibited.
    pub no_deallocate_inhibited: bool,
}

/// Capacités Format NVM (OACS bit 1, FNA octet 524).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FormatCaps {
    /// Format NVM supporté.
    pub supported: bool,
    /// FNA bit 0 : le format s'applique à tous les namespaces.
    pub format_all_namespaces: bool,
    /// FNA bit 1 : le secure erase s'applique à tous les namespaces.
    pub erase_all_namespaces: bool,
    /// FNA bit 2 : cryptographic erase supporté (SES = 2).
    pub crypto_erase: bool,
}

/// Identify Controller (CNS 01h), champs utiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifyController {
    /// SN, octets 4..24.
    pub serial: String,
    /// MN, octets 24..64.
    pub model: String,
    /// FR, octets 64..72.
    pub firmware: String,
    /// SANICAP.
    pub sanitize: SanitizeCaps,
    /// OACS / FNA.
    pub format: FormatCaps,
    /// NN, octets 516..520 : nombre de namespaces.
    pub namespace_count: u32,
}

/// Décode Identify Controller.
pub fn identify_controller(data: &[u8]) -> Result<IdentifyController, DecodeError> {
    require(data, IDENTIFY_SIZE, "NVMe Identify Controller")
        .map_err(|e| decode_err(e, "NVMe Identify Controller"))?;
    let oacs = le_u16(data, 256);
    let sanicap = le_u32(data, 328);
    let fna = data[524];
    Ok(IdentifyController {
        serial: ascii_trimmed(&data[4..24]),
        model: ascii_trimmed(&data[24..64]),
        firmware: ascii_trimmed(&data[64..72]),
        sanitize: SanitizeCaps {
            crypto_erase: sanicap & 1 != 0,
            block_erase: sanicap & (1 << 1) != 0,
            overwrite: sanicap & (1 << 2) != 0,
            no_deallocate_inhibited: sanicap & (1 << 29) != 0,
        },
        format: FormatCaps {
            supported: oacs & (1 << 1) != 0,
            format_all_namespaces: fna & 1 != 0,
            erase_all_namespaces: fna & (1 << 1) != 0,
            crypto_erase: fna & (1 << 2) != 0,
        },
        namespace_count: le_u32(data, 516),
    })
}

/// Identify Namespace (CNS 00h), champs utiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifyNamespace {
    /// NSZE, octets 0..8 : taille en blocs logiques.
    pub size_blocks: u64,
    /// Index du format LBA courant (FLBAS 3:0 et 6:5).
    pub lba_format_index: u8,
    /// Taille de bloc logique courante, en octets.
    pub block_bytes: u64,
    /// NGUID, octets 104..120 ; `None` si nul.
    pub nguid: Option<String>,
    /// EUI64, octets 120..128 ; `None` si nul.
    pub eui64: Option<String>,
}

impl IdentifyNamespace {
    /// Capacité en octets, `None` en cas de débordement.
    pub fn capacity_bytes(&self) -> Option<u64> {
        self.size_blocks.checked_mul(self.block_bytes)
    }
}

/// Décode Identify Namespace.
pub fn identify_namespace(data: &[u8]) -> Result<IdentifyNamespace, DecodeError> {
    require(data, IDENTIFY_SIZE, "NVMe Identify Namespace")
        .map_err(|e| decode_err(e, "NVMe Identify Namespace"))?;
    let flbas = data[26];
    let index = (flbas & 0x0f) | ((flbas >> 5) & 0x03) << 4;
    let lba_format = le_u32(data, 128 + 4 * usize::from(index));
    let lbads = (lba_format >> 16) & 0xff;
    if !(9..=63).contains(&lbads) {
        return Err(DecodeError::Invalid {
            structure: "NVMe Identify Namespace",
            reason: "taille de bloc logique hors plage",
        });
    }
    Ok(IdentifyNamespace {
        size_blocks: le_u64(data, 0),
        lba_format_index: index,
        block_bytes: 1u64 << lbads,
        nguid: hex_id(&data[104..120]),
        eui64: hex_id(&data[120..128]),
    })
}

/// SMART / Health Information log, champs utiles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HealthLog {
    /// Octet 0 : Critical Warning (bit 0 spare, 1 température, 2 fiabilité,
    /// 3 lecture seule, 4 sauvegarde volatile en panne).
    pub critical_warning: u8,
    /// Octets 1..3 : température composite, en kelvins.
    pub composite_temperature_kelvin: u16,
    /// Octet 3 : réserve disponible, en pourcentage.
    pub available_spare: u8,
    /// Octet 4 : seuil de réserve disponible.
    pub available_spare_threshold: u8,
    /// Octet 5 : Percentage Used (peut dépasser 100).
    pub percentage_used: u8,
    /// Octets 32..48 : Data Units Read (unités de 512 000 octets).
    pub data_units_read: u128,
    /// Octets 48..64 : Data Units Written.
    pub data_units_written: u128,
    /// Octets 64..80 : Host Read Commands.
    pub host_reads: u128,
    /// Octets 80..96 : Host Write Commands.
    pub host_writes: u128,
    /// Octets 96..112 : Controller Busy Time (minutes).
    pub controller_busy_minutes: u128,
    /// Octets 112..128 : Power Cycles.
    pub power_cycles: u128,
    /// Octets 128..144 : heures de fonctionnement.
    pub power_on_hours: u128,
    /// Octets 144..160 : Unsafe Shutdowns.
    pub unsafe_shutdowns: u128,
    /// Octets 160..176 : Media and Data Integrity Errors.
    pub media_errors: u128,
    /// Octets 176..192 : Number of Error Information Log Entries.
    pub error_log_entries: u128,
    /// Octets 192..196 : Warning Composite Temperature Time (minutes).
    pub warning_temp_minutes: u32,
    /// Octets 196..200 : Critical Composite Temperature Time (minutes).
    pub critical_temp_minutes: u32,
}

impl HealthLog {
    /// Bit 3 : média en lecture seule.
    pub fn read_only(&self) -> bool {
        self.critical_warning & (1 << 3) != 0
    }

    /// Bit 2 : fiabilité dégradée.
    pub fn reliability_degraded(&self) -> bool {
        self.critical_warning & (1 << 2) != 0
    }

    /// Température composite en degrés Celsius.
    pub fn composite_temperature_celsius(&self) -> i32 {
        i32::from(self.composite_temperature_kelvin) - 273
    }
}

/// Décode la page SMART / Health Information.
pub fn health(data: &[u8]) -> Result<HealthLog, DecodeError> {
    require(data, HEALTH_LOG_SIZE, "NVMe SMART / Health log")
        .map_err(|e| decode_err(e, "NVMe SMART / Health log"))?;
    Ok(HealthLog {
        critical_warning: data[0],
        composite_temperature_kelvin: le_u16(data, 1),
        available_spare: data[3],
        available_spare_threshold: data[4],
        percentage_used: data[5],
        data_units_read: le_u128(data, 32),
        data_units_written: le_u128(data, 48),
        host_reads: le_u128(data, 64),
        host_writes: le_u128(data, 80),
        controller_busy_minutes: le_u128(data, 96),
        power_cycles: le_u128(data, 112),
        power_on_hours: le_u128(data, 128),
        unsafe_shutdowns: le_u128(data, 144),
        media_errors: le_u128(data, 160),
        error_log_entries: le_u128(data, 176),
        warning_temp_minutes: le_u32(data, 192),
        critical_temp_minutes: le_u32(data, 196),
    })
}

/// État du sanitize (SSTAT bits 2:0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SanitizeState {
    /// 000b : jamais sanitisé.
    NeverSanitized,
    /// 001b / 100b : dernière opération terminée avec succès.
    Completed,
    /// 010b : opération en cours.
    InProgress,
    /// 011b : dernière opération échouée (mode Failure).
    Failed,
}

/// Page Sanitize Status (81h).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SanitizeLog {
    /// SPROG, octets 0..2 : progression sur 65 536.
    pub progress: u16,
    /// SSTAT bits 2:0.
    pub state: SanitizeState,
    /// SSTAT bits 7:3 : passes d'overwrite terminées.
    pub overwrite_passes_done: u8,
    /// SSTAT bit 8 : la zone GDL a été nettoyée.
    pub global_data_erased: bool,
}

impl SanitizeLog {
    /// Pourcentage approximatif.
    pub fn percent(&self) -> u8 {
        (u32::from(self.progress) * 100 / 65_536) as u8
    }
}

/// Décode la page Sanitize Status.
pub fn sanitize_log(data: &[u8]) -> Result<SanitizeLog, DecodeError> {
    require(data, SANITIZE_LOG_SIZE, "NVMe Sanitize Status log")
        .map_err(|e| decode_err(e, "NVMe Sanitize Status log"))?;
    let sstat = le_u16(data, 2);
    let state = match sstat & 0x07 {
        0 => SanitizeState::NeverSanitized,
        1 | 4 => SanitizeState::Completed,
        2 => SanitizeState::InProgress,
        3 => SanitizeState::Failed,
        _ => {
            return Err(DecodeError::Invalid {
                structure: "NVMe Sanitize Status log",
                reason: "SSTAT réservé (valeur 5 à 7)",
            })
        }
    };
    Ok(SanitizeLog {
        progress: le_u16(data, 0),
        state,
        overwrite_passes_done: ((sstat >> 3) & 0x1f) as u8,
        global_data_erased: sstat & (1 << 8) != 0,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn put(buffer: &mut [u8], at: usize, bytes: &[u8]) {
        buffer[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn padded(text: &str, width: usize) -> Vec<u8> {
        format!("{text:<width$}").into_bytes()
    }

    pub(crate) fn controller_buffer() -> Vec<u8> {
        let mut data = vec![0u8; IDENTIFY_SIZE];
        put(&mut data, 4, &padded("FICT0000A001", 20));
        put(&mut data, 24, &padded("Fictional NVMe SSD 1TB", 40));
        put(&mut data, 64, &padded("FICT01FW", 8));
        put(&mut data, 256, &0b10u16.to_le_bytes()); // OACS : Format NVM
        put(&mut data, 328, &0b011u32.to_le_bytes()); // SANICAP : CES + BES
        put(&mut data, 516, &1u32.to_le_bytes());
        data[524] = 0b110; // FNA : erase all namespaces, crypto erase
        data
    }

    #[test]
    fn decodes_identify_controller() {
        let c = identify_controller(&controller_buffer()).expect("valide");
        assert_eq!(c.serial, "FICT0000A001");
        assert_eq!(c.model, "Fictional NVMe SSD 1TB");
        assert_eq!(c.firmware, "FICT01FW");
        assert!(c.sanitize.crypto_erase && c.sanitize.block_erase && !c.sanitize.overwrite);
        assert!(c.format.supported && c.format.erase_all_namespaces && c.format.crypto_erase);
        assert!(!c.format.format_all_namespaces);
        assert_eq!(c.namespace_count, 1);
    }

    #[test]
    fn decodes_identify_namespace() {
        let mut data = vec![0u8; IDENTIFY_SIZE];
        put(&mut data, 0, &2_000_409_264u64.to_le_bytes());
        data[26] = 1;
        put(&mut data, 128, &(9u32 << 16).to_le_bytes());
        put(&mut data, 132, &(12u32 << 16).to_le_bytes());
        put(
            &mut data,
            120,
            &[0x00, 0xa0, 0x75, 0x01, 0xfe, 0x0c, 0x00, 0x01],
        );
        let ns = identify_namespace(&data).expect("valide");
        assert_eq!(ns.lba_format_index, 1);
        assert_eq!(ns.block_bytes, 4096);
        assert_eq!(ns.capacity_bytes(), Some(2_000_409_264 * 4096));
        assert_eq!(ns.eui64.as_deref(), Some("00a07501fe0c0001"));
        assert_eq!(ns.nguid, None);
        // Format 19 = 0b01_0011 : nibble bas 3, bits 6:5 = 01.
        data[26] = 0b0010_0011;
        put(&mut data, 128 + 4 * 19, &(9u32 << 16).to_le_bytes());
        assert_eq!(
            identify_namespace(&data).expect("valide").lba_format_index,
            19
        );
        assert!(
            identify_namespace(&vec![0u8; IDENTIFY_SIZE]).is_err(),
            "LBADS 0"
        );
    }

    #[test]
    fn decodes_health_log() {
        let mut data = vec![0u8; HEALTH_LOG_SIZE];
        data[0] = 0b100;
        put(&mut data, 1, &313u16.to_le_bytes());
        data[3] = 97;
        data[5] = 104;
        put(&mut data, 160, &7u128.to_le_bytes());
        let h = health(&data).expect("valide");
        assert!(h.reliability_degraded() && !h.read_only());
        assert_eq!(h.composite_temperature_celsius(), 40);
        assert_eq!(
            (h.available_spare, h.percentage_used, h.media_errors),
            (97, 104, 7)
        );
    }

    #[test]
    fn decodes_sanitize_log() {
        let log = |sstat: u16, sprog: u16| {
            let mut data = vec![0u8; SANITIZE_LOG_SIZE];
            put(&mut data, 0, &sprog.to_le_bytes());
            put(&mut data, 2, &sstat.to_le_bytes());
            sanitize_log(&data).expect("valide")
        };
        assert_eq!(log(0, 0).state, SanitizeState::NeverSanitized);
        assert_eq!(log(1, 0).state, SanitizeState::Completed);
        assert_eq!(log(4, 0).state, SanitizeState::Completed);
        assert_eq!(log(2, 0x8000).state, SanitizeState::InProgress);
        assert_eq!(log(2, 0x8000).percent(), 50);
        assert_eq!(log(3, 0).state, SanitizeState::Failed);
        assert_eq!(log(2 | 5 << 3 | 1 << 8, 0).overwrite_passes_done, 5);
        assert!(log(1 << 8, 0).global_data_erased);
        assert!(sanitize_log(&[0; 3]).is_err());
    }
}
