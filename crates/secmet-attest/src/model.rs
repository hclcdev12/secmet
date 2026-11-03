//! Le contenu attesté. Tout champ ici est signé ; rien d'autre ne l'est.

use serde::{Deserialize, Serialize};

use crate::{AttestError, Result};

/// Version du schéma de charge utile.
pub const SCHEMA_VERSION: u32 = 1;

/// Catégorie NIST SP 800-88 atteinte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NistLevel {
    /// Résistant à une récupération en laboratoire (commande firmware vérifiée).
    Purge,
    /// Résistant à une récupération logicielle (écriture hôte, Security Erase SSD…).
    Clear,
    /// Méthode hors annexe A (TRIM seul, effacement partiel) ou échec : aucune catégorie.
    NotClassified,
}

impl NistLevel {
    /// Libellé pour le PDF.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Purge => "PURGE",
            Self::Clear => "CLEAR",
            Self::NotClassified => "NON CLASSÉ",
        }
    }

    /// Explication courte pour le PDF.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Purge => {
                "NIST SP 800-88 Purge : commande de sanitisation du disque, vérifiée par relecture."
            }
            Self::Clear => {
                "NIST SP 800-88 Clear : protection contre une récupération logicielle simple."
            }
            Self::NotClassified => {
                "Aucune catégorie NIST ne peut être revendiquée pour cette opération."
            }
        }
    }
}

/// Issue de l'opération.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Terminé et validé.
    Success,
    /// Échec ou validation négative.
    Failed,
    /// Interrompu.
    Cancelled,
}

impl Outcome {
    /// Libellé pour le PDF.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Success => "EFFACEMENT RÉUSSI",
            Self::Failed => "EFFACEMENT ÉCHOUÉ",
            Self::Cancelled => "EFFACEMENT INTERROMPU",
        }
    }
}

/// Sort du support à l'issue de l'opération (`secmet-core`, module
/// `disposition`) : ce que l'atelier doit faire du disque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    /// Effacé et vérifié : réemploi possible.
    Reusable,
    /// Effacé et vérifié ; destruction physique recommandée.
    DestroyRecommended,
    /// Non effacé, aucune voie logicielle restante ; destruction physique requise.
    DestroyRequired,
    /// Non effacé, cause étrangère au support ; à relancer.
    Retry,
}

impl Disposition {
    /// Code stable, identique à `secmet_core::disposition::Disposition::code`.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Reusable => "reusable",
            Self::DestroyRecommended => "destroy_recommended",
            Self::DestroyRequired => "destroy_required",
            Self::Retry => "retry",
        }
    }

    /// Lecture d'un code.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "reusable" => Some(Self::Reusable),
            "destroy_recommended" => Some(Self::DestroyRecommended),
            "destroy_required" => Some(Self::DestroyRequired),
            "retry" => Some(Self::Retry),
            _ => None,
        }
    }

    /// Le support doit quitter le circuit de réemploi.
    pub const fn destroy(self) -> bool {
        matches!(self, Self::DestroyRecommended | Self::DestroyRequired)
    }
}

/// Le disque.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DiskFacts {
    /// Nom noyau (`sdb`).
    pub name: String,
    /// Modèle.
    pub model: String,
    /// Numéro de série.
    pub serial: String,
    /// Révision firmware.
    #[serde(default)]
    pub firmware: String,
    /// Média (`HDD`, `SSD SATA`, `SSD NVMe`, `HDD-USB`…).
    pub media: String,
    /// Transport (`sata`, `nvme`, `usb`…).
    #[serde(default)]
    pub transport: String,
    /// Capacité lisible (`500 GB`).
    pub capacity: String,
    /// Taille exacte en octets, si connue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// Taille de bloc logique, si connue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_size: Option<u64>,
}

/// L'opération.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErasureFacts {
    /// Code stable de la méthode (`NVME_SANITIZE_BLOCK_ERASE`, `HOST_OVERWRITE_1_ZERO`…).
    pub method_code: String,
    /// Libellé humain.
    pub method_label: String,
    /// Catégorie NIST revendiquée.
    pub nist_level: NistLevel,
    /// Politique de classification (`nist-800-88r1`).
    pub policy: String,
    /// Issue.
    pub outcome: Outcome,
    /// Début (`YYYY-MM-DD HH:MM:SS`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// Fin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// Durée en secondes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u64>,
    /// Réserves (pont USB, repli, dégradation Purge → Clear…).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<String>,
    /// Sort du support ; absent des attestations antérieures à la 0.3.9.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition: Option<Disposition>,
    /// Raisons du sort, codes stables (`SmartFailing`, `UnreadableSectors`…),
    /// libellés par [`crate::lang::reason_label`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disposition_reasons: Vec<String>,
}

/// Éléments maximaux d'une liste (réserves, signatures résiduelles) dans
/// l'attestation signée : au-delà, le QR dépasse la version 24 et devient
/// illisible imprimé. Le rapport de session garde la liste complète.
pub const MAX_LIST_ITEMS: usize = 8;
/// Longueur maximale d'un élément de liste dans l'attestation signée.
pub const MAX_ITEM_CHARS: usize = 160;

/// `text` tronqué à `max` caractères, avec une ellipse.
pub fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Borne une liste à [`MAX_LIST_ITEMS`] éléments de [`MAX_ITEM_CHARS`]
/// caractères ; le dernier élément résume ce qui a été omis.
fn bounded_items(items: &[String], what: &str) -> Vec<String> {
    if items.len() <= MAX_LIST_ITEMS {
        return items
            .iter()
            .map(|c| truncate_chars(c, MAX_ITEM_CHARS))
            .collect();
    }
    let kept = MAX_LIST_ITEMS - 1;
    let mut out: Vec<String> = items
        .iter()
        .take(kept)
        .map(|c| truncate_chars(c, MAX_ITEM_CHARS))
        .collect();
    out.push(format!(
        "… (+{} {what}, voir le rapport de session)",
        items.len() - kept
    ));
    out
}

/// Les preuves de validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ValidationFacts {
    /// `canaries` (seed + échantillon), `read-probes` (sondes SHA-256 historiques), `none`.
    pub kind: String,
    /// Seed de session hexadécimal (canaris), pour recalculer tout motif.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<String>,
    /// Blocs ou zones examinés.
    pub samples: u32,
    /// Anomalies (canari survivant, motif absent, bloc illisible…).
    pub anomalies: u32,
    /// SHA-256 des preuves (canonique) : l'« empreinte des canaris ».
    pub evidence_sha256: String,
    /// Signatures résiduelles (table de partitions, FS) après l'opération.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub residual_signatures: Vec<String>,
    /// Statut de la vérification indépendante (testdisk…), texte libre.
    #[serde(default)]
    pub verify_status: String,
}

/// Le banc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BenchFacts {
    /// Constructeur de la machine.
    pub manufacturer: String,
    /// Modèle.
    pub model: String,
    /// Numéro de série.
    pub serial: String,
}

/// Charge utile signée d'un certificat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attestation {
    /// Version du schéma.
    pub version: u32,
    /// Identifiant du certificat (`SEC-20261008-AB12-sdb`).
    pub certificate_id: String,
    /// Identifiant de la session.
    pub session_id: String,
    /// Émission (`YYYY-MM-DD HH:MM:SS`, heure locale du banc).
    pub issued_at: String,
    /// Opérateur.
    pub operator: String,
    /// Logiciel (`secmet 0.2.0`).
    pub software: String,
    /// Disque.
    pub disk: DiskFacts,
    /// Opération.
    pub erasure: ErasureFacts,
    /// Validation.
    pub validation: ValidationFacts,
    /// Banc.
    pub bench: BenchFacts,
    /// SHA-256 du rapport technique détaillé joint, s'il existe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_sha256: Option<String>,
}

impl Attestation {
    /// L'attestation avec ses listes bornées (voir [`MAX_LIST_ITEMS`]) :
    /// à appliquer avant signature, pour que le QR reste lisible imprimé.
    pub fn bounded(mut self) -> Self {
        self.erasure.caveats = bounded_items(&self.erasure.caveats, "autres réserves");
        self.erasure.disposition_reasons =
            bounded_items(&self.erasure.disposition_reasons, "autres raisons");
        self.validation.residual_signatures =
            bounded_items(&self.validation.residual_signatures, "autres signatures");
        self
    }

    /// Refuse un contenu incohérent avant signature : identifiants vides,
    /// succès sans catégorie, Purge revendiqué sans validation.
    pub fn validate(&self) -> Result<()> {
        if self.version != SCHEMA_VERSION {
            return Err(AttestError::Invalid(format!(
                "version de schéma {} (attendu {SCHEMA_VERSION})",
                self.version
            )));
        }
        for (name, value) in [
            ("certificate_id", &self.certificate_id),
            ("operator", &self.operator),
            ("disk.serial", &self.disk.serial),
            ("erasure.method_code", &self.erasure.method_code),
        ] {
            if value.trim().is_empty() {
                return Err(AttestError::Invalid(format!("{name} vide")));
            }
        }
        if self.erasure.outcome != Outcome::Success
            && self.erasure.nist_level != NistLevel::NotClassified
        {
            return Err(AttestError::Invalid(
                "une opération non réussie ne peut revendiquer aucune catégorie NIST".into(),
            ));
        }
        let unvalidated =
            matches!(self.validation.kind.trim(), "" | "none") || self.validation.samples == 0;
        if self.erasure.nist_level == NistLevel::Purge
            && (unvalidated || self.validation.anomalies > 0)
        {
            return Err(AttestError::Invalid(
                "Purge exige une validation par relecture sans anomalie".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Attestation de référence pour les tests de la crate.
    pub fn sample() -> Attestation {
        Attestation {
            version: SCHEMA_VERSION,
            certificate_id: "SEC-20261008-AB12-sdb".into(),
            session_id: "SEC-20261008-AB12".into(),
            issued_at: "2026-10-08 11:30:00".into(),
            operator: "Jean Dupont".into(),
            software: "secmet 0.2.0".into(),
            disk: DiskFacts {
                name: "sdb".into(),
                model: "Samsung SSD 870 EVO 500GB".into(),
                serial: "S6PYNX0T305592W".into(),
                firmware: "SVT02B6Q".into(),
                media: "SSD SATA".into(),
                transport: "sata".into(),
                capacity: "500 GB".into(),
                size_bytes: Some(500_107_862_016),
                block_size: Some(512),
            },
            erasure: ErasureFacts {
                method_code: "ATA_SANITIZE_BLOCK_ERASE".into(),
                method_label: "ATA SANITIZE BLOCK ERASE EXT".into(),
                nist_level: NistLevel::Purge,
                policy: "nist-800-88r1".into(),
                outcome: Outcome::Success,
                started_at: Some("2026-10-08 11:00:00".into()),
                finished_at: Some("2026-10-08 11:04:12".into()),
                duration_seconds: Some(252),
                caveats: vec![],
                disposition: Some(Disposition::Reusable),
                disposition_reasons: Vec::new(),
            },
            validation: ValidationFacts {
                kind: "canaries".into(),
                seed: Some("ab".repeat(32)),
                samples: 1003,
                anomalies: 0,
                evidence_sha256: "0".repeat(64),
                residual_signatures: vec![],
                verify_status: "OK".into(),
            },
            bench: BenchFacts {
                manufacturer: "Dell Inc.".into(),
                model: "OptiPlex 7090".into(),
                serial: "ABC1234".into(),
            },
            report_sha256: None,
        }
    }

    #[test]
    fn validation_rules() {
        let ok = sample();
        ok.validate().unwrap();
        let mut no_serial = sample();
        no_serial.disk.serial = " ".into();
        assert!(no_serial.validate().is_err());
        let mut failed_purge = sample();
        failed_purge.erasure.outcome = Outcome::Failed;
        assert!(failed_purge.validate().is_err());
        failed_purge.erasure.nist_level = NistLevel::NotClassified;
        failed_purge.validate().unwrap();
        let mut purge_without_proof = sample();
        purge_without_proof.validation.kind = "none".into();
        assert!(purge_without_proof.validate().is_err());
        let mut purge_with_anomaly = sample();
        purge_with_anomaly.validation.anomalies = 1;
        assert!(purge_with_anomaly.validate().is_err());
        let mut bad_version = sample();
        bad_version.version = 2;
        assert!(bad_version.validate().is_err());
    }

    #[test]
    fn serde_round_trip_refuses_unknown_fields() {
        let text = serde_json::to_string(&sample()).unwrap();
        let back: Attestation = serde_json::from_str(&text).unwrap();
        assert_eq!(back, sample());
        let tampered = text.replace("\"version\":1", "\"version\":1,\"extra\":1");
        assert!(serde_json::from_str::<Attestation>(&tampered).is_err());
        assert_eq!(NistLevel::Purge.label(), "PURGE");
        assert_eq!(Outcome::Cancelled.label(), "EFFACEMENT INTERROMPU");
    }
}
