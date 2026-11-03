//! `secmet-attest` — certificats d'effacement infalsifiables, sans Chromium.
//!
//! | Module        | Rôle                                                                 |
//! |---------------|----------------------------------------------------------------------|
//! | [`model`]     | Le contenu attesté : disque, méthode, catégorie NIST, validation     |
//! | [`canonical`] | JSON canonique (clés triées, compact) : ce qui est signé            |
//! | [`sign`]      | Clé Ed25519, enveloppe signée, vérification                         |
//! | [`base45`]    | Base45 (RFC 9285), l'alphabet du mode alphanumérique des QR codes   |
//! | [`qr`]        | `SM1:` + Base45(zlib(enveloppe)) et matrice de modules              |
//! | [`pdf`]       | Écrivain PDF minimal (polices standard) et mise en page du certificat |
//! | [`lang`]      | Langue de rédaction du PDF (français, anglais, allemand)             |
//! | [`keystore`]  | Clé de signature du poste, fichier `0600`, créée au premier usage     |
//! | [`export`]    | Du contenu au PDF + JSON signé sur disque, et vérification hors ligne |
//! | [`release`]   | Versions publiées : `SHA256SUMS` et `latest.json` signés (clé de version) |
//! | [`manifest`]  | Manifeste de lot signé : liste des certificats et empreintes (Annexe M) |
//!
//! Chaîne de confiance : la charge utile ([`model::Attestation`]) est
//! sérialisée en JSON canonique, préfixée d'un séparateur de domaine, puis
//! signée Ed25519 avec la clé du poste. L'enveloppe ([`sign::SignedAttestation`])
//! est écrite en JSON à côté du PDF et encodée dans le QR imprimé sur le PDF.
//! Un vérificateur hors ligne ([`sign::verify`], binaire `secmet-verify`)
//! n'a besoin que de la clé publique.

pub mod base45;
pub mod canonical;
pub mod export;
pub mod keystore;
pub mod lang;
pub mod manifest;
pub mod model;
pub mod pdf;
pub mod qr;
pub mod release;
pub mod sign;

/// Erreurs de la crate.
#[derive(Debug, thiserror::Error)]
pub enum AttestError {
    /// Sérialisation JSON.
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// Signature invalide ou clé illisible.
    #[error("signature: {0}")]
    Signature(String),
    /// Texte Base45 ou QR malformé.
    #[error("encodage: {0}")]
    Encoding(String),
    /// QR impossible (charge trop grande).
    #[error("QR code: {0}")]
    Qr(String),
    /// Contenu refusé avant signature.
    #[error("contenu invalide: {0}")]
    Invalid(String),
    /// E/S (clé, fichiers exportés).
    #[error("{context}: {source}")]
    Io {
        /// Ce qui était tenté.
        context: String,
        /// Erreur d'origine.
        #[source]
        source: std::io::Error,
    },
}

impl AttestError {
    /// Attache un contexte à une erreur d'E/S.
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

/// Résultat standard.
pub type Result<T> = std::result::Result<T, AttestError>;

/// Hexadécimal minuscule.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Décode un hexadécimal de longueur paire.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// SHA-256 hexadécimal.
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    hex(&sha2::Sha256::digest(data))
}
