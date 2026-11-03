//! Libellés métier affichés sur les certificats.
use crate::disk::DiskKind;
use crate::i18n::{trf_in, Lang};

/// Libellé « type média » pour le certificat.
pub fn media_label(kind: DiskKind, is_usb: bool) -> String {
    if is_usb {
        return match kind {
            DiskKind::Hdd | DiskKind::HddUsb => "HDD-USB",
            DiskKind::SsdNvme => "NVME-USB",
            _ => "SSD-USB",
        }
        .to_string();
    }
    match kind {
        DiskKind::Hdd | DiskKind::HddUsb => "HDD",
        DiskKind::SsdSata | DiskKind::SsdUsb => "SSD SATA",
        DiskKind::SsdNvme => "SSD NVMe",
        DiskKind::Ssd => "SSD",
        DiskKind::Unknown => "INCONNU",
    }
    .to_string()
}

/// Libellé humain d'un code de méthode du moteur (`EraseMethod::label`),
/// en français (journaux, enregistrements de session).
pub fn method_label(code: &str) -> String {
    method_label_in(Lang::Fr, code)
}

/// [`method_label`] dans `lang` (certificats).
pub fn method_label_in(lang: Lang, code: &str) -> String {
    let passes = |n: u32, one: &str, many: &str| {
        trf_in(
            lang,
            if n > 1 { many } else { one },
            &[("n", &n.to_string())],
        )
    };
    if let Some(rest) = code.strip_prefix("HOST_OVERWRITE_") {
        let (count, zero) = match rest.strip_suffix("_ZERO") {
            Some(p) => (p, true),
            None => (rest, false),
        };
        let n: u32 = count.parse().unwrap_or(1);
        let base = passes(
            n,
            "Écriture hôte : {n} passe pseudo-aléatoire",
            "Écriture hôte : {n} passes pseudo-aléatoires",
        );
        return if zero {
            trf_in(lang, "{method} + zéros", &[("method", &base)])
        } else {
            base
        };
    }
    if let Some(rest) = code.strip_prefix("NVME_SANITIZE_OVERWRITE_") {
        let n: u32 = rest.parse().unwrap_or(1);
        return passes(
            n,
            "NVMe Sanitize Overwrite ({n} passe)",
            "NVMe Sanitize Overwrite ({n} passes)",
        );
    }
    match code {
        "NVME_SANITIZE_BLOCK_ERASE" => "NVMe Sanitize Block Erase",
        "NVME_SANITIZE_CRYPTO_ERASE" => "NVMe Sanitize Crypto Erase",
        "NVME_FORMAT_SES2_CRYPTO_ERASE" => "NVMe Format NVM crypto-erase (ses=2)",
        "NVME_FORMAT_SES1_USER_DATA_ERASE" => "NVMe Format NVM user-data erase (ses=1)",
        "ATA_SANITIZE_BLOCK_ERASE" => "ATA Sanitize Block Erase",
        "ATA_SANITIZE_CRYPTO_SCRAMBLE" => "ATA Sanitize Crypto Scramble",
        "ATA_SANITIZE_OVERWRITE" => "ATA Sanitize Overwrite",
        "ATA_SECURITY_ERASE_ENHANCED" => "ATA Security Erase (enhanced)",
        "ATA_SECURITY_ERASE" => "ATA Security Erase",
        other => other,
    }
    .to_string()
}

pub fn pdf_result_label(success: bool) -> &'static str {
    if success {
        "Succès"
    } else {
        "Échec"
    }
}

pub fn pdf_result_from_success_flag(success: bool) -> String {
    pdf_result_label(success).to_string()
}
