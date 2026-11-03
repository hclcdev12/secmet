//! Signature Ed25519 de l'attestation et vérification hors ligne.
//!
//! Message signé = `DOMAIN` ‖ JSON canonique de la charge utile. Le
//! séparateur de domaine empêche qu'une signature SecMet soit réinterprétée
//! dans un autre protocole utilisant la même clé.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::model::Attestation;
use crate::{canonical, hex, unhex, AttestError, Result};

/// Séparateur de domaine.
pub const DOMAIN: &[u8] = b"secmet-attestation-v1\n";
/// Algorithme, tel qu'écrit dans l'enveloppe.
pub const ALGORITHM: &str = "Ed25519";

/// Clé de signature du poste.
pub struct SigningIdentity {
    key: SigningKey,
}

impl std::fmt::Debug for SigningIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SigningIdentity({})", self.key_id())
    }
}

/// Identifiant court d'une clé publique : 16 hex de SHA-256(clé).
pub fn key_id_of(public_key: &[u8; 32]) -> String {
    hex(&Sha256::digest(public_key))[..16].to_string()
}

impl SigningIdentity {
    /// Depuis un seed de 32 octets.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(seed),
        }
    }

    /// Clé publique, 32 octets.
    pub fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    /// Clé publique en hexadécimal.
    pub fn public_key_hex(&self) -> String {
        hex(&self.public_key())
    }

    /// Identifiant court.
    pub fn key_id(&self) -> String {
        key_id_of(&self.public_key())
    }

    /// Signe `payload` (validée au préalable).
    pub fn sign(&self, payload: &Attestation) -> Result<SignedAttestation> {
        payload.validate()?;
        let value = serde_json::to_value(payload)?;
        let message = signing_message(&value);
        let signature: Signature = self.key.sign(&message);
        Ok(SignedAttestation {
            alg: ALGORITHM.to_string(),
            key_id: self.key_id(),
            public_key: self.public_key_hex(),
            payload: payload.clone(),
            signature: hex(&signature.to_bytes()),
        })
    }

    /// Signe `message` tel quel. Réservé aux autres protocoles de la crate
    /// ([`crate::release`]), dont le message porte déjà son propre
    /// séparateur de domaine.
    pub(crate) fn sign_raw(&self, message: &[u8]) -> [u8; 64] {
        self.key.sign(message).to_bytes()
    }
}

/// Octets signés pour une charge utile.
pub fn signing_message(payload: &Value) -> Vec<u8> {
    let mut message = DOMAIN.to_vec();
    message.extend_from_slice(canonical::to_string(payload).as_bytes());
    message
}

/// Enveloppe : la charge utile, la clé publique et la signature. C'est ce
/// qui est écrit en JSON à côté du PDF et encodé dans le QR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedAttestation {
    /// `Ed25519`.
    pub alg: String,
    /// Identifiant court de la clé.
    pub key_id: String,
    /// Clé publique, 64 hex.
    pub public_key: String,
    /// Charge utile.
    pub payload: Attestation,
    /// Signature, 128 hex.
    pub signature: String,
}

/// Résultat d'une vérification réussie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// Identifiant de la clé qui a signé.
    pub key_id: String,
    /// La clé est dans la liste de confiance fournie (ou aucune liste).
    pub trusted: bool,
}

/// Vérifie l'enveloppe : algorithme, cohérence `key_id`/clé, signature.
/// `trusted_keys` (hex) restreint les clés acceptées ; vide = toute clé
/// valide, mais `trusted` sera faux.
pub fn verify(signed: &SignedAttestation, trusted_keys: &[String]) -> Result<Verified> {
    let value = serde_json::to_value(&signed.payload)?;
    verify_raw(
        &signed.alg,
        &signed.key_id,
        &signed.public_key,
        &signed.signature,
        &signing_message(&value),
    )
    .map_err(|e| match e {
        AttestError::Signature(m) if m == BAD_SIGNATURE => {
            AttestError::Signature("signature invalide : contenu ou clé altérés".into())
        }
        other => other,
    })?;
    signed.payload.validate()?;
    Ok(Verified {
        key_id: signed.key_id.clone(),
        trusted: is_trusted(trusted_keys, &signed.key_id, &signed.public_key),
    })
}

const BAD_SIGNATURE: &str = "signature invalide";

/// Vérifie la signature hexadécimale `signature` de `message` : algorithme,
/// cohérence `key_id`/clé publique, signature stricte.
pub(crate) fn verify_raw(
    alg: &str,
    key_id: &str,
    public_key: &str,
    signature: &str,
    message: &[u8],
) -> Result<()> {
    if alg != ALGORITHM {
        return Err(AttestError::Signature(format!(
            "algorithme « {alg} » non supporté"
        )));
    }
    let public: [u8; 32] = unhex(public_key)
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| AttestError::Signature("clé publique malformée".into()))?;
    if key_id_of(&public) != key_id {
        return Err(AttestError::Signature(
            "key_id ne correspond pas à la clé publique".into(),
        ));
    }
    let signature: [u8; 64] = unhex(signature)
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| AttestError::Signature("signature malformée".into()))?;
    let verifying = VerifyingKey::from_bytes(&public)
        .map_err(|e| AttestError::Signature(format!("clé publique invalide: {e}")))?;
    verifying
        .verify_strict(message, &Signature::from_bytes(&signature))
        .map_err(|_| AttestError::Signature(BAD_SIGNATURE.into()))
}

/// Une clé de confiance se donne par sa clé publique (64 hex) ou par son
/// identifiant (16 hex) ; les deux désignent la même clé.
pub(crate) fn is_trusted(trusted_keys: &[String], key_id: &str, public_key: &str) -> bool {
    trusted_keys.iter().any(|k| {
        let k = k.trim();
        k.eq_ignore_ascii_case(public_key) || k.eq_ignore_ascii_case(key_id)
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::model::tests::sample;

    fn identity() -> SigningIdentity {
        SigningIdentity::from_seed(&[7u8; 32])
    }

    #[test]
    fn sign_then_verify_and_detect_any_tampering() {
        let id = identity();
        let signed = id.sign(&sample()).unwrap();
        assert_eq!(signed.signature.len(), 128);
        assert_eq!(signed.public_key.len(), 64);
        let v = verify(&signed, &[]).unwrap();
        assert_eq!(v.key_id, id.key_id());
        assert!(!v.trusted);
        assert!(
            verify(&signed, &[id.public_key_hex().to_uppercase()])
                .unwrap()
                .trusted
        );

        let mut tampered = signed.clone();
        tampered.payload.disk.serial = "AUTRE".into();
        assert!(verify(&tampered, &[]).is_err());
        let mut level = signed.clone();
        level.payload.erasure.nist_level = crate::model::NistLevel::Clear;
        assert!(verify(&level, &[]).is_err());
        let mut other_key = signed.clone();
        other_key.public_key = SigningIdentity::from_seed(&[8u8; 32]).public_key_hex();
        assert!(verify(&other_key, &[]).is_err(), "key_id incohérent");
        other_key.key_id = key_id_of(&SigningIdentity::from_seed(&[8u8; 32]).public_key());
        assert!(
            verify(&other_key, &[]).is_err(),
            "signature d'une autre clé"
        );
        let mut alg = signed.clone();
        alg.alg = "RSA".into();
        assert!(verify(&alg, &[]).is_err());
        let mut sig = signed;
        sig.signature = "00".repeat(64);
        assert!(verify(&sig, &[]).is_err());
    }

    #[test]
    fn signature_is_deterministic_and_independent_of_key_order() {
        let id = identity();
        let a = id.sign(&sample()).unwrap();
        let b = id.sign(&sample()).unwrap();
        assert_eq!(a.signature, b.signature, "Ed25519 est déterministe");
        // Une enveloppe relue depuis un JSON aux clés réordonnées reste valide.
        let text = serde_json::to_string(&a).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        let reordered = canonical::to_string(&value);
        let back: SignedAttestation = serde_json::from_str(&reordered).unwrap();
        verify(&back, &[]).unwrap();
    }

    #[test]
    fn invalid_payload_is_never_signed() {
        let mut bad = sample();
        bad.operator = String::new();
        assert!(identity().sign(&bad).is_err());
    }
}
