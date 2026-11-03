//! Signature des versions publiées : `SHA256SUMS` et `latest.json` signés
//! avec la **clé de version**, distincte de la clé d'attestation des
//! certificats (Annexe N). Le signataire est le responsable des versions,
//! hors ligne ; le vérificateur est l'application (mise à jour) ou
//! `secmet-release verify`.
//!
//! Message signé = [`DOMAIN`] ‖ étiquette ‖ `\n` ‖ contenu : l'étiquette
//! (`sums`, `latest`) sépare les deux documents, le domaine sépare ce
//! protocole de celui des attestations même si une clé était réutilisée.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sign::{self, SigningIdentity, Verified, ALGORITHM};
use crate::{canonical, hex, sha256_hex, AttestError, Result};

/// Séparateur de domaine.
pub const DOMAIN: &[u8] = b"secmet-release-v1\n";
/// Fichier des empreintes, au format de `sha256sum`.
pub const SUMS_FILE: &str = "SHA256SUMS";
/// Signature détachée des empreintes (JSON, [`SumsSignature`]).
pub const SUMS_SIGNATURE_FILE: &str = "SHA256SUMS.sig";
/// Description signée de la dernière version ([`SignedLatest`]).
pub const LATEST_FILE: &str = "latest.json";

fn message(label: &str, content: &[u8]) -> Vec<u8> {
    let mut m = DOMAIN.to_vec();
    m.extend_from_slice(label.as_bytes());
    m.push(b'\n');
    m.extend_from_slice(content);
    m
}

/// Une ligne de `SHA256SUMS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SumEntry {
    /// Empreinte, 64 hex minuscules.
    pub sha256: String,
    /// Nom du fichier, sans répertoire.
    pub name: String,
}

/// Analyse le format de `sha256sum` : `<hex>  <nom>` (ou ` *<nom>`), une
/// entrée par ligne ; lignes vides et `#` ignorés.
pub fn parse_sums(text: &str) -> Result<Vec<SumEntry>> {
    let mut entries = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = || AttestError::Invalid(format!("{SUMS_FILE} ligne {} : format", n + 1));
        let (sum, rest) = line.split_once(' ').ok_or_else(bad)?;
        let name = rest.trim_start().trim_start_matches('*');
        if sum.len() != 64
            || !sum.bytes().all(|b| b.is_ascii_hexdigit())
            || name.is_empty()
            || name.contains('/')
        {
            return Err(bad());
        }
        entries.push(SumEntry {
            sha256: sum.to_ascii_lowercase(),
            name: name.to_string(),
        });
    }
    if entries.is_empty() {
        return Err(AttestError::Invalid(format!("{SUMS_FILE} vide")));
    }
    Ok(entries)
}

/// Écrit les entrées au format de `sha256sum`.
pub fn format_sums(entries: &[SumEntry]) -> String {
    entries
        .iter()
        .map(|e| format!("{}  {}\n", e.sha256, e.name))
        .collect()
}

/// Empreintes des fichiers `names` de `dir`.
pub fn sums_of(dir: &Path, names: &[String]) -> Result<Vec<SumEntry>> {
    names
        .iter()
        .map(|name| {
            let bytes = fs::read(dir.join(name)).map_err(|e| AttestError::io(name.clone(), e))?;
            Ok(SumEntry {
                sha256: sha256_hex(&bytes),
                name: name.clone(),
            })
        })
        .collect()
}

/// Vérifie que chaque fichier listé existe dans `dir` avec l'empreinte
/// annoncée.
pub fn check_sums(dir: &Path, entries: &[SumEntry]) -> Result<()> {
    for e in entries {
        let bytes =
            fs::read(dir.join(&e.name)).map_err(|err| AttestError::io(e.name.clone(), err))?;
        if sha256_hex(&bytes) != e.sha256 {
            return Err(AttestError::Invalid(format!(
                "{} : empreinte différente de {SUMS_FILE}",
                e.name
            )));
        }
    }
    Ok(())
}

/// Signature détachée de `SHA256SUMS`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SumsSignature {
    /// `Ed25519`.
    pub alg: String,
    /// Identifiant court de la clé de version.
    pub key_id: String,
    /// Clé publique de version, 64 hex.
    pub public_key: String,
    /// Empreinte du fichier `SHA256SUMS` signé (information).
    pub sha256: String,
    /// Signature, 128 hex.
    pub signature: String,
}

/// Signe le texte de `SHA256SUMS`.
pub fn sign_sums(identity: &SigningIdentity, sums_text: &str) -> SumsSignature {
    let signature = identity.sign_raw(&message("sums", sums_text.as_bytes()));
    SumsSignature {
        alg: ALGORITHM.to_string(),
        key_id: identity.key_id(),
        public_key: identity.public_key_hex(),
        sha256: sha256_hex(sums_text.as_bytes()),
        signature: hex(&signature),
    }
}

/// Vérifie la signature de `sums_text` ; `trusted_keys` comme pour
/// [`sign::verify`].
pub fn verify_sums(
    sig: &SumsSignature,
    sums_text: &str,
    trusted_keys: &[String],
) -> Result<Verified> {
    if sig.sha256 != sha256_hex(sums_text.as_bytes()) {
        return Err(AttestError::Signature(format!(
            "{SUMS_FILE} ne correspond pas à sa signature"
        )));
    }
    sign::verify_raw(
        &sig.alg,
        &sig.key_id,
        &sig.public_key,
        &sig.signature,
        &message("sums", sums_text.as_bytes()),
    )?;
    Ok(Verified {
        key_id: sig.key_id.clone(),
        trusted: sign::is_trusted(trusted_keys, &sig.key_id, &sig.public_key),
    })
}

/// `https://`, ou `http://` vers la boucle locale seulement (canal d'essai
/// servi sur le poste lui-même : pas de trajet réseau à protéger).
pub fn secure_url(url: &str) -> bool {
    url.starts_with("https://")
        || url.starts_with("http://127.0.0.1/")
        || url.starts_with("http://127.0.0.1:")
        || url.starts_with("http://localhost/")
        || url.starts_with("http://localhost:")
}

/// Dernière version publiée, telle que l'application la lit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LatestRelease {
    /// `MAJEUR.MINEUR.CORRECTIF`.
    pub version: String,
    /// Date de publication, `AAAA-MM-JJ`.
    pub published_at: String,
    /// Notes de version par langue (`fr`, `en`, `de`).
    #[serde(default)]
    pub notes: BTreeMap<String, String>,
    /// Adresse `https` du `SHA256SUMS` de cette version.
    pub sha256sums_url: String,
    /// Nouvelle adresse de `latest.json` si le canal a déménagé.
    #[serde(default)]
    pub moved_to: Option<String>,
}

impl LatestRelease {
    /// Refuse une version, une date ou une adresse malformées.
    pub fn validate(&self) -> Result<()> {
        parse_version(&self.version).ok_or_else(|| {
            AttestError::Invalid(format!(
                "version « {} » : attendu MAJEUR.MINEUR.CORRECTIF",
                self.version
            ))
        })?;
        let date_ok = self.published_at.len() == 10
            && self.published_at.chars().enumerate().all(|(i, c)| {
                if i == 4 || i == 7 {
                    c == '-'
                } else {
                    c.is_ascii_digit()
                }
            });
        if !date_ok {
            return Err(AttestError::Invalid(format!(
                "published_at « {} » : attendu AAAA-MM-JJ",
                self.published_at
            )));
        }
        if !secure_url(&self.sha256sums_url) {
            return Err(AttestError::Invalid(
                "sha256sums_url : https:// requis".into(),
            ));
        }
        if let Some(url) = &self.moved_to {
            if !url.starts_with("https://") {
                return Err(AttestError::Invalid("moved_to : https:// requis".into()));
            }
        }
        Ok(())
    }
}

/// `MAJEUR.MINEUR.CORRECTIF` (préfixe `v` toléré) en triplet comparable.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// `candidate` est-elle strictement plus récente que `installed` ? Faux si
/// l'une des deux est illisible : on ne propose jamais une mise à jour sur
/// une version qu'on ne sait pas comparer.
pub fn is_newer(candidate: &str, installed: &str) -> bool {
    match (parse_version(candidate), parse_version(installed)) {
        (Some(c), Some(i)) => c > i,
        _ => false,
    }
}

/// Enveloppe signée de [`LatestRelease`], contenu de `latest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedLatest {
    /// `Ed25519`.
    pub alg: String,
    /// Identifiant court de la clé de version.
    pub key_id: String,
    /// Clé publique de version, 64 hex.
    pub public_key: String,
    /// Charge utile.
    pub payload: LatestRelease,
    /// Signature, 128 hex.
    pub signature: String,
}

fn latest_message(value: &Value) -> Vec<u8> {
    message("latest", canonical::to_string(value).as_bytes())
}

/// Signe `payload` (validée au préalable).
pub fn sign_latest(identity: &SigningIdentity, payload: &LatestRelease) -> Result<SignedLatest> {
    payload.validate()?;
    let value = serde_json::to_value(payload)?;
    let signature = identity.sign_raw(&latest_message(&value));
    Ok(SignedLatest {
        alg: ALGORITHM.to_string(),
        key_id: identity.key_id(),
        public_key: identity.public_key_hex(),
        payload: payload.clone(),
        signature: hex(&signature),
    })
}

/// Vérifie `latest.json` ; `trusted_keys` comme pour [`sign::verify`].
pub fn verify_latest(signed: &SignedLatest, trusted_keys: &[String]) -> Result<Verified> {
    let value = serde_json::to_value(&signed.payload)?;
    sign::verify_raw(
        &signed.alg,
        &signed.key_id,
        &signed.public_key,
        &signed.signature,
        &latest_message(&value),
    )?;
    signed.payload.validate()?;
    Ok(Verified {
        key_id: signed.key_id.clone(),
        trusted: sign::is_trusted(trusted_keys, &signed.key_id, &signed.public_key),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn identity() -> SigningIdentity {
        SigningIdentity::from_seed(&[9u8; 32])
    }

    fn latest() -> LatestRelease {
        LatestRelease {
            version: "0.3.2".into(),
            published_at: "2026-11-15".into(),
            notes: BTreeMap::from([("fr".to_string(), "Correctifs.".to_string())]),
            sha256sums_url: "https://example.invalid/v0.3.2/SHA256SUMS".into(),
            moved_to: None,
        }
    }

    #[test]
    fn sums_round_trip_and_tampering() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.deb"), b"aaa").unwrap();
        fs::write(dir.path().join("b.rpm"), b"bbb").unwrap();
        let names = vec!["a.deb".to_string(), "b.rpm".to_string()];
        let entries = sums_of(dir.path(), &names).unwrap();
        let text = format_sums(&entries);
        assert_eq!(parse_sums(&text).unwrap(), entries);
        check_sums(dir.path(), &entries).unwrap();

        let id = identity();
        let sig = sign_sums(&id, &text);
        let v = verify_sums(&sig, &text, &[id.key_id()]).unwrap();
        assert!(v.trusted);
        assert!(!verify_sums(&sig, &text, &[]).unwrap().trusted);

        // Fichier modifié après signature : l'empreinte ne colle plus.
        fs::write(dir.path().join("a.deb"), b"AAA").unwrap();
        assert!(check_sums(dir.path(), &entries).is_err());
        // Texte des empreintes modifié : signature invalide.
        let altered = text.replace("a.deb", "c.deb");
        assert!(verify_sums(&sig, &altered, &[]).is_err());
        // Signature d'un autre domaine (attestation) refusée.
        let mut other = sig.clone();
        other.signature = hex(&id.sign_raw(&sign::signing_message(&Value::Null)));
        assert!(verify_sums(&other, &text, &[]).is_err());
    }

    #[test]
    fn parse_sums_rejects_malformed_lines() {
        assert!(parse_sums("").is_err());
        assert!(parse_sums("abc  x\n").is_err());
        let hex64 = "0".repeat(64);
        assert!(parse_sums(&format!("{hex64}  ../x\n")).is_err());
        let ok = parse_sums(&format!("# commentaire\n{hex64} *x.AppImage\n")).unwrap();
        assert_eq!(ok[0].name, "x.AppImage");
    }

    #[test]
    fn loopback_http_is_accepted_for_tests_only() {
        assert!(secure_url("https://example.org/x/SHA256SUMS"));
        assert!(secure_url("http://127.0.0.1:8765/SHA256SUMS"));
        assert!(secure_url("http://localhost/SHA256SUMS"));
        assert!(!secure_url("http://192.168.1.10/SHA256SUMS"));
        assert!(!secure_url("http://127.0.0.1.evil.example/SHA256SUMS"));
        let mut l = latest();
        l.sha256sums_url = "http://127.0.0.1:8765/SHA256SUMS".into();
        assert!(l.validate().is_ok());
        l.sha256sums_url = "http://example.org/SHA256SUMS".into();
        assert!(l.validate().is_err());
    }

    #[test]
    fn latest_round_trip_validation_and_tampering() {
        let id = identity();
        let signed = sign_latest(&id, &latest()).unwrap();
        let json = serde_json::to_string(&signed).unwrap();
        let back: SignedLatest = serde_json::from_str(&json).unwrap();
        assert!(
            verify_latest(&back, &[id.public_key_hex()])
                .unwrap()
                .trusted
        );

        let mut tampered = signed.clone();
        tampered.payload.version = "9.9.9".into();
        assert!(verify_latest(&tampered, &[]).is_err());

        let mut bad = latest();
        bad.version = "0.3".into();
        assert!(sign_latest(&id, &bad).is_err());
        bad = latest();
        bad.sha256sums_url = "http://example.invalid/SHA256SUMS".into();
        assert!(sign_latest(&id, &bad).is_err());
        bad = latest();
        bad.published_at = "15/11/2026".into();
        assert!(sign_latest(&id, &bad).is_err());
    }

    #[test]
    fn version_ordering() {
        assert!(is_newer("0.3.2", "0.3.1"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("0.3.1", "0.3.1"));
        assert!(!is_newer("0.3.0", "0.3.1"));
        assert!(!is_newer("0.3.1-rc1", "0.3.0"));
        assert_eq!(parse_version("0.10.2"), Some((0, 10, 2)));
    }
}
