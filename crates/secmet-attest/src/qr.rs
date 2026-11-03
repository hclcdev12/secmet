//! Attestation dans un QR code : `SM1:` + Base45(zlib(JSON canonique de
//! l'enveloppe)). Le préfixe identifie le format ; Base45 permet le mode
//! alphanumérique du QR ; zlib ramène l'enveloppe sous 1 Kio.

use std::io::{Read, Write};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use qrcode::{Color, EcLevel, QrCode};

use crate::sign::SignedAttestation;
use crate::{base45, canonical, AttestError, Result};

/// Préfixe du texte QR.
pub const PREFIX: &str = "SM1:";
/// Côté maximal (version 24 : 117 modules). Au-delà, les modules tombent
/// sous 1 pt sur la carte imprimée et le QR n'est plus lisible.
pub const MAX_SIDE: usize = 117;
/// Taille décompressée maximale acceptée (anti-bombe zip).
const MAX_DECODED: u64 = 1 << 20;

/// Texte à encoder dans le QR.
pub fn encode_text(signed: &SignedAttestation) -> Result<String> {
    let value = serde_json::to_value(signed)?;
    let json = canonical::to_string(&value);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder
        .write_all(json.as_bytes())
        .and_then(|()| encoder.finish())
        .map(|compressed| format!("{PREFIX}{}", base45::encode(&compressed)))
        .map_err(|e| AttestError::Encoding(format!("zlib: {e}")))
}

/// Enveloppe depuis le texte d'un QR (non vérifiée : voir [`crate::sign::verify`]).
pub fn decode_text(text: &str) -> Result<SignedAttestation> {
    let body = text
        .trim()
        .strip_prefix(PREFIX)
        .ok_or_else(|| AttestError::Encoding(format!("préfixe {PREFIX} absent")))?;
    let compressed = base45::decode(body)?;
    let mut json = Vec::new();
    ZlibDecoder::new(&compressed[..])
        .take(MAX_DECODED)
        .read_to_end(&mut json)
        .map_err(|e| AttestError::Encoding(format!("zlib: {e}")))?;
    Ok(serde_json::from_slice(&json)?)
}

/// Matrice de modules d'un QR, sans zone de silence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrMatrix {
    /// Modules par côté.
    pub size: usize,
    /// Ligne par ligne, `true` = module sombre.
    pub modules: Vec<bool>,
}

impl QrMatrix {
    /// Module en (`x`, `y`).
    pub fn dark(&self, x: usize, y: usize) -> bool {
        self.modules
            .get(y * self.size + x)
            .copied()
            .unwrap_or(false)
    }
}

/// QR code pour `text`. Correction L : imprimé sur papier blanc sans
/// logo superposé, la densité prime sur la redondance. Refusé au-delà de
/// [`MAX_SIDE`] : une attestation bornée ([`crate::model::Attestation::bounded`])
/// tient toujours, l'erreur signale une attestation non bornée.
pub fn matrix(text: &str) -> Result<QrMatrix> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::L)
        .map_err(|e| AttestError::Qr(e.to_string()))?;
    if code.width() > MAX_SIDE {
        return Err(AttestError::Qr(format!(
            "QR trop dense pour l'impression ({} modules, maximum {MAX_SIDE}) : \
             attestation trop longue ({} caractères)",
            code.width(),
            text.len()
        )));
    }
    Ok(QrMatrix {
        size: code.width(),
        modules: code.to_colors().iter().map(|c| *c == Color::Dark).collect(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::model::tests::sample;
    use crate::sign::{verify, SigningIdentity};

    #[test]
    fn qr_text_round_trips_and_stays_alphanumeric() {
        let signed = SigningIdentity::from_seed(&[1u8; 32])
            .sign(&sample())
            .unwrap();
        let text = encode_text(&signed).unwrap();
        assert!(text.starts_with(PREFIX));
        assert!(text.len() < 1200, "{} caractères", text.len());
        assert!(text[PREFIX.len()..]
            .bytes()
            .all(|c| b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:".contains(&c)));
        let back = decode_text(&text).unwrap();
        assert_eq!(back, signed);
        verify(&back, &[]).unwrap();
        let m = matrix(&text).unwrap();
        assert!(m.size >= 21 && m.size <= 177 && m.size % 4 == 1);
        assert_eq!(m.modules.len(), m.size * m.size);
        assert!(m.dark(0, 0) && m.dark(6, 6), "motif de repérage");
        assert!(
            m.size <= MAX_SIDE,
            "version ≤ 24 pour rester lisible imprimé : {} modules, {} caractères",
            m.size,
            text.len()
        );
    }

    /// Pire cas d'une attestation bornée : listes pleines, champs longs.
    fn worst_case() -> crate::model::Attestation {
        let mut a = sample();
        a.certificate_id = "SEC-20261231-ZZZZ-nvme0n1".into();
        a.operator = "Opérateur avec un nom particulièrement long pour le test".into();
        a.software = "secmet 10.20.30 (build 2026-12-31 abcdef0123456789)".into();
        a.disk.model = "WDC WD40EFRX-68WT0N0 Red NAS Hard Drive Edition 4TB".into();
        a.disk.serial = "WD-WCC4E0123456789".into();
        a.disk.firmware = "82.00A82".into();
        a.disk.media = "HDD 5400 tr/min".into();
        a.disk.capacity = "4 000 787 030 016 octets".into();
        a.erasure.method_label =
            "Écriture hôte 3 passes (pseudo-aléatoire, complément, zéros)".into();
        a.erasure.caveats = (0..20)
            .map(|i| format!("réserve numéro {i} : {}", "détail ".repeat(40)))
            .collect();
        a.validation.residual_signatures = (0..20)
            .map(|i| format!("signature {i} : {}", "zone ".repeat(60)))
            .collect();
        a.validation.verify_status = "Vérification TestDisk : ".to_string() + &"x".repeat(100);
        a.bench.manufacturer = "Constructeur Générique International".into();
        a.bench.model = "Station de travail modèle très long 9000".into();
        a.bench.serial = "SN-0123456789-ABCDEFGHIJ".into();
        a.bounded()
    }

    #[test]
    fn a_bounded_worst_case_attestation_stays_printable() {
        let worst = worst_case();
        assert_eq!(worst.erasure.caveats.len(), crate::model::MAX_LIST_ITEMS);
        assert!(worst.erasure.caveats[7].contains("+13 autres réserves"));
        assert!(worst
            .erasure
            .caveats
            .iter()
            .all(|c| c.chars().count() <= crate::model::MAX_ITEM_CHARS));
        assert_eq!(
            worst.validation.residual_signatures.len(),
            crate::model::MAX_LIST_ITEMS
        );
        let signed = SigningIdentity::from_seed(&[2u8; 32]).sign(&worst).unwrap();
        let text = encode_text(&signed).unwrap();
        let m = matrix(&text).expect("le pire cas borné doit tenir");
        assert!(m.size <= MAX_SIDE, "{} modules", m.size);
    }

    #[test]
    fn an_unbounded_attestation_is_refused_rather_than_unreadable() {
        let mut huge = sample();
        huge.erasure.caveats = (0..200)
            .map(|i| format!("réserve {i} {}", "x".repeat(200)))
            .collect();
        let signed = SigningIdentity::from_seed(&[3u8; 32]).sign(&huge).unwrap();
        let text = encode_text(&signed).unwrap();
        let err = matrix(&text).unwrap_err().to_string();
        assert!(
            err.contains("trop dense") || err.contains("too long") || err.contains("data"),
            "{err}"
        );
    }

    #[test]
    fn malformed_qr_texts_are_rejected() {
        assert!(decode_text("XX:AAA").is_err());
        assert!(decode_text("SM1:abc").is_err());
        assert!(decode_text("SM1:BB8").is_err(), "zlib invalide");
        let signed = SigningIdentity::from_seed(&[1u8; 32])
            .sign(&sample())
            .unwrap();
        let text = encode_text(&signed).unwrap();
        let mut corrupted = text.clone().into_bytes();
        let i = corrupted.len() / 2;
        corrupted[i] = if corrupted[i] == b'A' { b'B' } else { b'A' };
        let corrupted = String::from_utf8(corrupted).unwrap();
        assert!(
            decode_text(&corrupted).is_err()
                || verify(&decode_text(&corrupted).unwrap(), &[]).is_err()
        );
    }
}
