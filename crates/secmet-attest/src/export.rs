//! Exportation : signer, rendre le PDF, écrire les JSON ; relire et vérifier.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::lang::Lang;
use crate::model::Attestation;
use crate::pdf::{self, RenderedCertificate};
use crate::sign::{self, SignedAttestation, SigningIdentity, Verified};
use crate::{AttestError, Result};

/// Fichiers produits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// Le PDF (toutes les pages).
    pub pdf: PathBuf,
    /// Un JSON signé par certificat.
    pub json: Vec<PathBuf>,
    /// JSON du lot (tableau d'enveloppes), si un identifiant de lot est donné.
    pub lot_json: Option<PathBuf>,
    /// Les enveloppes signées, dans l'ordre.
    pub signed: Vec<SignedAttestation>,
}

/// Nom de fichier sûr dérivé d'un identifiant.
pub fn safe_filename(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Signe `attestations`, rend `dir/<pdf_file>` et écrit les JSON signés.
/// Chaque charge utile est validée avant signature ; la première invalide
/// fait échouer l'export sans rien écrire.
pub fn export(
    identity: &SigningIdentity,
    dir: &Path,
    pdf_file: &str,
    attestations: &[Attestation],
    lot_id: Option<&str>,
) -> Result<Exported> {
    export_with_logo(
        identity,
        dir,
        pdf_file,
        attestations,
        lot_id,
        None,
        Lang::Fr,
    )
}

/// Comme [`export`], avec un logo JPEG personnalisé pour l'en-tête des
/// pages (`None` ou JPEG illisible : logo SecMet) et la langue du PDF.
/// Les JSON signés ne dépendent pas de la langue de rendu.
pub fn export_with_logo(
    identity: &SigningIdentity,
    dir: &Path,
    pdf_file: &str,
    attestations: &[Attestation],
    lot_id: Option<&str>,
    logo_jpeg: Option<&[u8]>,
    lang: Lang,
) -> Result<Exported> {
    if attestations.is_empty() {
        return Err(AttestError::Invalid("aucun certificat à exporter".into()));
    }
    let mut rendered = Vec::with_capacity(attestations.len());
    for attestation in attestations {
        let bounded = attestation.clone().bounded();
        rendered.push(RenderedCertificate::new(identity.sign(&bounded)?)?);
    }
    let bytes =
        pdf::render_with_logo(&rendered, lot_id, logo_jpeg.unwrap_or(pdf::LOGO_JPEG), lang)?;
    fs::create_dir_all(dir).map_err(|e| AttestError::io(dir.display().to_string(), e))?;
    let pdf_path = dir.join(pdf_file);
    write_atomic(&pdf_path, &bytes)?;
    let mut json = Vec::with_capacity(rendered.len());
    for cert in &rendered {
        let path = dir.join(format!(
            "{}.json",
            safe_filename(&cert.signed.payload.certificate_id)
        ));
        write_atomic(
            &path,
            serde_json::to_string_pretty(&cert.signed)?.as_bytes(),
        )?;
        json.push(path);
    }
    let signed: Vec<SignedAttestation> = rendered.into_iter().map(|c| c.signed).collect();
    let lot_json = match lot_id {
        Some(lot) => {
            let path = dir.join(format!("{}.json", safe_filename(lot)));
            write_atomic(&path, serde_json::to_string_pretty(&signed)?.as_bytes())?;
            Some(path)
        }
        None => None,
    };
    Ok(Exported {
        pdf: pdf_path,
        json,
        lot_json,
        signed,
    })
}

/// Rend le PDF d'un lot à partir d'enveloppes **déjà signées** (clôture
/// d'un lot : les attestations ont été produites machine par machine, on
/// ne les re-signe pas). Écrit le PDF et le JSON du lot (tableau des
/// enveloppes) ; les JSON individuels existent déjà.
pub fn export_signed_lot(
    dir: &Path,
    pdf_file: &str,
    signed: &[SignedAttestation],
    lot_id: &str,
    logo_jpeg: Option<&[u8]>,
    lang: Lang,
) -> Result<Exported> {
    if signed.is_empty() {
        return Err(AttestError::Invalid("aucun certificat à exporter".into()));
    }
    let mut rendered = Vec::with_capacity(signed.len());
    for envelope in signed {
        sign::verify(envelope, &[])?;
        rendered.push(RenderedCertificate::new(envelope.clone())?);
    }
    let bytes = pdf::render_lot(&rendered, lot_id, logo_jpeg.unwrap_or(pdf::LOGO_JPEG), lang)?;
    fs::create_dir_all(dir).map_err(|e| AttestError::io(dir.display().to_string(), e))?;
    let pdf_path = dir.join(pdf_file);
    write_atomic(&pdf_path, &bytes)?;
    let lot_json = dir.join(format!("{}.json", safe_filename(lot_id)));
    write_atomic(&lot_json, serde_json::to_string_pretty(signed)?.as_bytes())?;
    Ok(Exported {
        pdf: pdf_path,
        json: Vec::new(),
        lot_json: Some(lot_json),
        signed: signed.to_vec(),
    })
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)
        .and_then(|()| fs::rename(&tmp, path))
        .map_err(|e| AttestError::io(path.display().to_string(), e))
}

/// Relit un JSON signé (une enveloppe ou un tableau) et vérifie chaque entrée.
pub fn verify_file(
    path: &Path,
    trusted_keys: &[String],
) -> Result<Vec<(SignedAttestation, Verified)>> {
    let text =
        fs::read_to_string(path).map_err(|e| AttestError::io(path.display().to_string(), e))?;
    verify_json(&text, trusted_keys)
}

/// Comme [`verify_file`], sur un texte JSON.
pub fn verify_json(
    text: &str,
    trusted_keys: &[String],
) -> Result<Vec<(SignedAttestation, Verified)>> {
    let value: Value = serde_json::from_str(text)?;
    let entries: Vec<SignedAttestation> = match value {
        Value::Array(_) => serde_json::from_value(value)?,
        other => vec![serde_json::from_value(other)?],
    };
    entries
        .into_iter()
        .map(|signed| sign::verify(&signed, trusted_keys).map(|v| (signed, v)))
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::model::tests::sample;
    use crate::qr;

    #[test]
    fn export_writes_pdf_and_signed_json_that_verify_and_detect_tampering() {
        let tmp = tempfile::tempdir().unwrap();
        let identity = SigningIdentity::from_seed(&[9u8; 32]);
        let mut second = sample();
        second.certificate_id = "SEC-20261008-AB12-sdc".into();
        second.disk.serial = "OTHER".into();
        let out = export(
            &identity,
            tmp.path(),
            "SEC-20261008-AB12-lot.pdf",
            &[sample(), second],
            Some("SEC-20261008-AB12-lot"),
        )
        .unwrap();
        assert!(out.pdf.exists());
        assert_eq!(pdf::page_count(&fs::read(&out.pdf).unwrap()), 3);
        assert_eq!(out.json.len(), 2);
        assert!(out.json[0].ends_with("SEC-20261008-AB12-sdb.json"));
        let lot = out.lot_json.clone().unwrap();
        assert!(lot.ends_with("SEC-20261008-AB12-lot.json"));
        assert!(!tmp.path().join("SEC-20261008-AB12-lot.tmp").exists());

        let trusted = vec![identity.public_key_hex()];
        let per_disk = verify_file(&out.json[0], &trusted).unwrap();
        assert_eq!(per_disk.len(), 1);
        assert!(per_disk[0].1.trusted);
        let whole_lot = verify_file(&lot, &[]).unwrap();
        assert_eq!(whole_lot.len(), 2);
        assert!(!whole_lot[0].1.trusted);

        // Le QR embarqué dans le PDF porte la même enveloppe.
        let qr_text = qr::encode_text(&out.signed[0]).unwrap();
        assert_eq!(qr::decode_text(&qr_text).unwrap(), out.signed[0]);

        // Un octet changé dans le JSON : la vérification échoue.
        let text = fs::read_to_string(&out.json[1]).unwrap();
        let tampered = text.replace("\"OTHER\"", "\"FORGED\"");
        assert_ne!(text, tampered);
        assert!(verify_json(&tampered, &[]).is_err());
        assert!(verify_json("[]", &[]).unwrap().is_empty());
        assert!(verify_json("{\"alg\":\"x\"}", &[]).is_err());
    }

    #[test]
    fn invalid_attestation_blocks_the_whole_export() {
        let tmp = tempfile::tempdir().unwrap();
        let identity = SigningIdentity::from_seed(&[9u8; 32]);
        let mut bad = sample();
        bad.certificate_id = String::new();
        assert!(export(&identity, tmp.path(), "x.pdf", &[sample(), bad], None).is_err());
        assert!(
            fs::read_dir(tmp.path()).unwrap().next().is_none(),
            "rien d'écrit"
        );
        assert!(export(&identity, tmp.path(), "x.pdf", &[], None).is_err());
        assert_eq!(safe_filename("SEC-1/2 é"), "SEC-1_2__");
    }
}
