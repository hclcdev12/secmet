//! Manifeste de lot signé (Annexe M, §M.5) : à la clôture d'un lot, la
//! liste des certificats et de leurs empreintes, signée avec la clé du
//! poste. Il comble une faiblesse du simple tableau d'attestations : on
//! peut en retirer une sans que cela se voie ; avec le manifeste, toute
//! soustraction ou substitution est détectable.
//!
//! Message signé = [`DOMAIN`] ‖ JSON canonique du manifeste, comme pour
//! les attestations ([`crate::sign`]) mais avec son propre domaine.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sign::{self, SignedAttestation, SigningIdentity, Verified, ALGORITHM};
use crate::{canonical, hex, sha256_hex, AttestError, Result};

/// Séparateur de domaine.
pub const DOMAIN: &[u8] = b"secmet-lot-manifest-v1\n";
/// Version du schéma.
pub const SCHEMA_VERSION: u32 = 1;

/// Un certificat du lot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    /// `certificate_id` de l'attestation.
    pub certificate_id: String,
    /// Numéro de série de la machine traitée (DMI), vide si inconnu.
    pub machine_serial: String,
    /// Empreinte SHA-256 du JSON canonique de l'enveloppe signée.
    pub sha256: String,
}

/// Charge utile du manifeste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LotManifest {
    /// Version du schéma.
    pub version: u32,
    /// Identifiant du lot.
    pub lot_id: String,
    /// Opérateur ayant ouvert le lot.
    pub operator: String,
    /// Référence client, libre.
    #[serde(default)]
    pub reference: String,
    /// Ouverture, `AAAA-MM-JJ HH:MM:SS`.
    pub opened_at: String,
    /// Clôture.
    pub closed_at: String,
    /// Logiciel (`secmet x.y.z`).
    pub software: String,
    /// Machines distinctes traitées.
    pub machines: u32,
    /// Certificats, dans l'ordre du lot.
    pub certificates: Vec<ManifestEntry>,
    /// Empreinte SHA-256 du journal chaîné du lot à la clôture.
    pub journal_sha256: String,
}

impl LotManifest {
    /// Refuse un manifeste vide ou incohérent.
    pub fn validate(&self) -> Result<()> {
        if self.version != SCHEMA_VERSION {
            return Err(AttestError::Invalid(format!(
                "manifeste : version {} non supportée",
                self.version
            )));
        }
        if self.lot_id.trim().is_empty() || self.operator.trim().is_empty() {
            return Err(AttestError::Invalid(
                "manifeste : lot_id et operator requis".into(),
            ));
        }
        if self.certificates.is_empty() {
            return Err(AttestError::Invalid("manifeste : aucun certificat".into()));
        }
        for c in &self.certificates {
            if c.sha256.len() != 64 || !c.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(AttestError::Invalid(format!(
                    "manifeste : empreinte malformée pour {}",
                    c.certificate_id
                )));
            }
        }
        Ok(())
    }
}

/// Empreinte d'une enveloppe signée, telle que le manifeste la liste :
/// SHA-256 du JSON canonique (indépendant de la mise en forme du fichier).
pub fn envelope_sha256(signed: &SignedAttestation) -> Result<String> {
    let value = serde_json::to_value(signed)?;
    Ok(sha256_hex(canonical::to_string(&value).as_bytes()))
}

/// Entrée de manifeste pour une enveloppe.
pub fn entry_for(signed: &SignedAttestation) -> Result<ManifestEntry> {
    Ok(ManifestEntry {
        certificate_id: signed.payload.certificate_id.clone(),
        machine_serial: signed.payload.bench.serial.clone(),
        sha256: envelope_sha256(signed)?,
    })
}

/// Enveloppe signée du manifeste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedManifest {
    /// `Ed25519`.
    pub alg: String,
    /// Identifiant court de la clé.
    pub key_id: String,
    /// Clé publique, 64 hex.
    pub public_key: String,
    /// Charge utile.
    pub payload: LotManifest,
    /// Signature, 128 hex.
    pub signature: String,
}

fn message(value: &Value) -> Vec<u8> {
    let mut m = DOMAIN.to_vec();
    m.extend_from_slice(canonical::to_string(value).as_bytes());
    m
}

/// Signe un manifeste (validé au préalable).
pub fn sign_manifest(identity: &SigningIdentity, payload: &LotManifest) -> Result<SignedManifest> {
    payload.validate()?;
    let value = serde_json::to_value(payload)?;
    let signature = identity.sign_raw(&message(&value));
    Ok(SignedManifest {
        alg: ALGORITHM.to_string(),
        key_id: identity.key_id(),
        public_key: identity.public_key_hex(),
        payload: payload.clone(),
        signature: hex(&signature),
    })
}

/// Vérifie la signature du manifeste ; `trusted_keys` comme pour
/// [`sign::verify`].
pub fn verify_manifest(signed: &SignedManifest, trusted_keys: &[String]) -> Result<Verified> {
    let value = serde_json::to_value(&signed.payload)?;
    sign::verify_raw(
        &signed.alg,
        &signed.key_id,
        &signed.public_key,
        &signed.signature,
        &message(&value),
    )?;
    signed.payload.validate()?;
    Ok(Verified {
        key_id: signed.key_id.clone(),
        trusted: sign::is_trusted(trusted_keys, &signed.key_id, &signed.public_key),
    })
}

/// Confronte un manifeste aux enveloppes effectivement présentes : chaque
/// certificat listé doit être là avec la même empreinte, et aucune
/// enveloppe en trop. Retourne les anomalies (vide = conforme).
pub fn reconcile(manifest: &LotManifest, present: &[SignedAttestation]) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for signed in present {
        let id = &signed.payload.certificate_id;
        seen.insert(id.clone());
        match manifest
            .certificates
            .iter()
            .find(|c| &c.certificate_id == id)
        {
            None => problems.push(format!("{id} : absent du manifeste")),
            Some(entry) => {
                if entry.sha256 != envelope_sha256(signed)? {
                    problems.push(format!("{id} : empreinte différente du manifeste"));
                }
            }
        }
    }
    for entry in &manifest.certificates {
        if !seen.contains(&entry.certificate_id) {
            problems.push(format!(
                "{} : listé au manifeste, fichier manquant",
                entry.certificate_id
            ));
        }
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::model::tests::sample;

    fn identity() -> SigningIdentity {
        SigningIdentity::from_seed(&[11u8; 32])
    }

    fn signed_pair() -> Vec<SignedAttestation> {
        let id = identity();
        let a = sample();
        let mut b = sample();
        b.certificate_id = "SEC-TEST-0001-sdb".into();
        b.disk.serial = "OTHER".into();
        vec![id.sign(&a).unwrap(), id.sign(&b).unwrap()]
    }

    fn manifest_for(signed: &[SignedAttestation]) -> LotManifest {
        LotManifest {
            version: SCHEMA_VERSION,
            lot_id: "L-20261030-AB12".into(),
            operator: "Jean".into(),
            reference: "client X".into(),
            opened_at: "2026-10-30 09:00:00".into(),
            closed_at: "2026-10-30 17:00:00".into(),
            software: "secmet 0.3.1".into(),
            machines: 2,
            certificates: signed.iter().map(|s| entry_for(s).unwrap()).collect(),
            journal_sha256: "0".repeat(64),
        }
    }

    #[test]
    fn sign_verify_and_detect_tampering() {
        let signed = signed_pair();
        let id = identity();
        let m = sign_manifest(&id, &manifest_for(&signed)).unwrap();
        let json = serde_json::to_string(&m).unwrap();
        let back: SignedManifest = serde_json::from_str(&json).unwrap();
        assert!(verify_manifest(&back, &[id.key_id()]).unwrap().trusted);
        assert!(!verify_manifest(&back, &[]).unwrap().trusted);

        let mut tampered = m.clone();
        tampered.payload.certificates.pop();
        assert!(verify_manifest(&tampered, &[]).is_err());
        // Une signature d'attestation ne vaut pas signature de manifeste.
        let mut cross = m;
        cross.signature = signed[0].signature.clone();
        assert!(verify_manifest(&cross, &[]).is_err());
    }

    #[test]
    fn reconcile_flags_missing_extra_and_changed() {
        let signed = signed_pair();
        let m = manifest_for(&signed);
        assert!(reconcile(&m, &signed).unwrap().is_empty());
        // Une attestation retirée.
        let problems = reconcile(&m, &signed[..1]).unwrap();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("fichier manquant"));
        // Une attestation re-signée avec un autre contenu.
        let mut altered = signed.clone();
        let mut payload = altered[1].payload.clone();
        payload.operator = "Autre".into();
        altered[1] = identity().sign(&payload).unwrap();
        let problems = reconcile(&m, &altered).unwrap();
        assert!(problems[0].contains("empreinte différente"));
        // Une attestation en trop.
        let mut extra = signed.clone();
        let mut p = sample();
        p.certificate_id = "SEC-TEST-0001-sdz".into();
        extra.push(identity().sign(&p).unwrap());
        assert!(reconcile(&m, &extra).unwrap()[0].contains("absent du manifeste"));
    }

    #[test]
    fn validation_rejects_empty_or_malformed() {
        let signed = signed_pair();
        let id = identity();
        let mut m = manifest_for(&signed);
        m.certificates.clear();
        assert!(sign_manifest(&id, &m).is_err());
        let mut m = manifest_for(&signed);
        m.certificates[0].sha256 = "zz".into();
        assert!(sign_manifest(&id, &m).is_err());
        let mut m = manifest_for(&signed);
        m.version = 2;
        assert!(sign_manifest(&id, &m).is_err());
    }
}
