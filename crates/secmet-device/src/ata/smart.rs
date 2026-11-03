//! SMART ATA : RETURN STATUS (verdict seuil) et READ DATA (attributs).
//!
//! Remplace `smartctl -H` et la lecture des attributs 5, 9, 187, 194, 197,
//! 198 utilisés par SecMet pour le certificat et la santé.

use std::collections::BTreeMap;

use crate::{require, DecodeError};

/// Taille de SMART READ DATA.
pub const SMART_DATA_SIZE: usize = 512;

/// Verdict de SMART RETURN STATUS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartStatus {
    /// 4Fh / C2h : aucun seuil dépassé.
    Passed,
    /// F4h / 2Ch : un attribut a franchi son seuil (prédiction de panne).
    Failing,
}

/// Décode LBA Mid / LBA High de SMART RETURN STATUS.
pub fn status_from_registers(lba_mid: u8, lba_high: u8) -> Result<SmartStatus, DecodeError> {
    match (lba_mid, lba_high) {
        (0x4f, 0xc2) => Ok(SmartStatus::Passed),
        (0xf4, 0x2c) => Ok(SmartStatus::Failing),
        _ => Err(DecodeError::Invalid {
            structure: "ATA SMART RETURN STATUS",
            reason: "registres LBA inattendus",
        }),
    }
}

/// Valeurs brutes des attributs SMART utilisés par SecMet. `None` = non rapporté.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SmartAttributes {
    /// Attribut 5 : secteurs réalloués.
    pub reallocated_sectors: Option<u64>,
    /// Attribut 9 : heures de fonctionnement (16 bits bas).
    pub power_on_hours: Option<u64>,
    /// Attribut 187 : erreurs non corrigibles rapportées.
    pub reported_uncorrectable: Option<u64>,
    /// Attribut 194 : température, en degrés Celsius (16 bits bas de la valeur brute).
    pub temperature_celsius: Option<u16>,
    /// Attribut 197 : secteurs en attente de réallocation.
    pub pending_sectors: Option<u64>,
    /// Attribut 198 : secteurs non corrigibles hors ligne.
    pub offline_uncorrectable: Option<u64>,
}

/// Une entrée brute de la table SMART (12 octets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawAttribute {
    /// Identifiant (1..=255).
    pub id: u8,
    /// Drapeaux (bit 0 pre-fail, bit 1 online…).
    pub flags: u16,
    /// Valeur normalisée courante.
    pub value: u8,
    /// Pire valeur normalisée.
    pub worst: u8,
    /// Valeur brute sur 48 bits.
    pub raw: u64,
    /// Seuil (SMART READ THRESHOLDS), si connu.
    pub threshold: Option<u8>,
}

/// Taille de SMART READ THRESHOLDS.
pub const SMART_THRESHOLDS_SIZE: usize = 512;

fn checked_sector(data: &[u8], structure: &'static str) -> Result<(), DecodeError> {
    require(data, SMART_DATA_SIZE, structure).map_err(|e| match e {
        crate::DeviceError::Decode(d) => d,
        _ => DecodeError::Invalid {
            structure,
            reason: "tampon invalide",
        },
    })?;
    if data[..SMART_DATA_SIZE]
        .iter()
        .fold(0u8, |s, b| s.wrapping_add(*b))
        != 0
    {
        return Err(DecodeError::Checksum(structure));
    }
    Ok(())
}

/// Décode toutes les entrées présentes (identifiant non nul) de SMART READ DATA.
pub fn decode_attribute_table(data: &[u8]) -> Result<Vec<RawAttribute>, DecodeError> {
    checked_sector(data, "ATA SMART READ DATA")?;
    Ok(data[2..2 + 30 * 12]
        .chunks_exact(12)
        .filter(|entry| entry[0] != 0)
        .map(|entry| RawAttribute {
            id: entry[0],
            flags: u16::from_le_bytes([entry[1], entry[2]]),
            value: entry[3],
            worst: entry[4],
            raw: entry[5..11]
                .iter()
                .rev()
                .fold(0u64, |acc, b| (acc << 8) | u64::from(*b)),
            threshold: None,
        })
        .collect())
}

/// Décode SMART READ THRESHOLDS : identifiant → seuil.
pub fn decode_thresholds(data: &[u8]) -> Result<BTreeMap<u8, u8>, DecodeError> {
    checked_sector(data, "ATA SMART READ THRESHOLDS")?;
    Ok(data[2..2 + 30 * 12]
        .chunks_exact(12)
        .filter(|entry| entry[0] != 0)
        .map(|entry| (entry[0], entry[1]))
        .collect())
}

/// Décode SMART READ DATA : 30 entrées de 12 octets à partir de l'octet 2,
/// somme de contrôle sur 512 octets. Les valeurs brutes sont lues sur 48 bits.
pub fn decode_attributes(data: &[u8]) -> Result<SmartAttributes, DecodeError> {
    checked_sector(data, "ATA SMART READ DATA")?;
    let mut attributes = SmartAttributes::default();
    for entry in data[2..2 + 30 * 12].chunks_exact(12) {
        let raw = entry[5..11]
            .iter()
            .rev()
            .fold(0u64, |acc, b| (acc << 8) | u64::from(*b));
        match entry[0] {
            5 => attributes.reallocated_sectors = Some(raw),
            // Valeur brute 48 bits : certains disques dépassent 65 536 h.
            9 => attributes.power_on_hours = Some(raw),
            187 => attributes.reported_uncorrectable = Some(raw),
            194 => attributes.temperature_celsius = Some((raw & 0xffff) as u16),
            197 => attributes.pending_sectors = Some(raw),
            198 => attributes.offline_uncorrectable = Some(raw),
            _ => {}
        }
    }
    Ok(attributes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn smart_buffer(attributes: &[(u8, u64)]) -> Vec<u8> {
        let mut data = vec![0u8; SMART_DATA_SIZE];
        data[0] = 0x10;
        for (slot, (id, raw)) in attributes.iter().enumerate() {
            let entry = &mut data[2 + 12 * slot..2 + 12 * (slot + 1)];
            entry[0] = *id;
            entry[3] = 100;
            entry[4] = 100;
            entry[5..11].copy_from_slice(&raw.to_le_bytes()[..6]);
        }
        let sum = data.iter().fold(0u8, |s, b| s.wrapping_add(*b));
        data[511] = 0u8.wrapping_sub(sum);
        data
    }

    #[test]
    fn decodes_attributes() {
        let data = smart_buffer(&[
            (9, 12_000),
            (5, 12),
            (197, 1),
            (198, 0),
            (187, 3),
            (194, 0x0021_0012_0024),
        ]);
        let a = decode_attributes(&data).expect("valide");
        assert_eq!(a.reallocated_sectors, Some(12));
        assert_eq!(a.power_on_hours, Some(12_000));
        assert_eq!(a.reported_uncorrectable, Some(3));
        assert_eq!(a.temperature_celsius, Some(36));
        assert_eq!(a.pending_sectors, Some(1));
        assert_eq!(a.offline_uncorrectable, Some(0));
    }

    #[test]
    fn raw_values_are_48_bit_and_missing_stay_unknown() {
        let data = smart_buffer(&[(5, 0x0001_0000_0002)]);
        let a = decode_attributes(&data).expect("valide");
        assert_eq!(a.reallocated_sectors, Some(0x0001_0000_0002));
        assert_eq!(a.pending_sectors, None);
    }

    #[test]
    fn corrupted_data_is_rejected() {
        let mut data = smart_buffer(&[(5, 12)]);
        data[7] ^= 1;
        assert_eq!(
            decode_attributes(&data),
            Err(DecodeError::Checksum("ATA SMART READ DATA"))
        );
        assert!(decode_attributes(&[0; 10]).is_err());
    }

    #[test]
    fn decodes_full_table_and_thresholds() {
        let data = smart_buffer(&[(5, 12), (194, 36), (241, 7)]);
        let table = decode_attribute_table(&data).expect("valide");
        assert_eq!(table.len(), 3);
        assert_eq!((table[0].id, table[0].raw, table[0].value), (5, 12, 100));
        assert_eq!(table[2].id, 241);
        assert!(table.iter().all(|a| a.threshold.is_none()));
        let mut thresholds = vec![0u8; SMART_THRESHOLDS_SIZE];
        thresholds[2] = 5;
        thresholds[3] = 10;
        thresholds[14] = 194;
        thresholds[15] = 0;
        let sum = thresholds.iter().fold(0u8, |s, b| s.wrapping_add(*b));
        thresholds[511] = 0u8.wrapping_sub(sum);
        let map = decode_thresholds(&thresholds).expect("valide");
        assert_eq!(map.get(&5), Some(&10));
        assert_eq!(map.get(&194), Some(&0));
        assert_eq!(map.len(), 2);
        thresholds[3] = 11;
        assert!(decode_thresholds(&thresholds).is_err(), "somme de contrôle");
    }

    #[test]
    fn decodes_return_status() {
        assert_eq!(status_from_registers(0x4f, 0xc2), Ok(SmartStatus::Passed));
        assert_eq!(status_from_registers(0xf4, 0x2c), Ok(SmartStatus::Failing));
        assert!(status_from_registers(0, 0).is_err());
    }
}
