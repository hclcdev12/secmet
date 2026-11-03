//! Erreurs typées du helper.

use secmet_core::CoreError;
use secmet_device::DeviceError;
use serde::{Deserialize, Serialize};

/// Un processus tiers qui tient un descripteur sur la cible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opener {
    /// PID.
    pub pid: u32,
    /// `comm` du processus.
    pub comm: String,
    /// Numéro du descripteur.
    pub fd: u32,
    /// Cible du lien `/proc/<pid>/fd/<n>`.
    pub target: String,
}

fn describe_openers(openers: &[Opener]) -> String {
    openers
        .iter()
        .map(|o| format!("{} (pid {}, fd {} → {})", o.comm, o.pid, o.fd, o.target))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Refus d'isolation : la cible ou la session n'est pas dans l'état attendu.
/// Chaque variante est renvoyée telle quelle à l'UI (`kind` stable).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IsolationError {
    /// Montée, swap, membre d'un volume, ou `O_EXCL` refusé par le noyau.
    #[error("périphérique occupé: {0}")]
    Busy(String),
    /// Le disque derrière le nœud n'est plus celui de la session.
    #[error("identité du disque différente ({field}): attendu « {expected} », trouvé « {found} »")]
    IdentityMismatch {
        /// Champ en désaccord (`devnum`, `serial`, `fingerprint`).
        field: &'static str,
        /// Valeur liée au jeton.
        expected: String,
        /// Valeur relue sur le disque.
        found: String,
    },
    /// D'autres processus tiennent un descripteur sur la cible.
    #[error("descripteurs ouverts par d'autres processus: {}", describe_openers(.0))]
    OpenersFound(Vec<Opener>),
    /// Le jeton de session a dépassé sa durée de vie.
    #[error("jeton de session expiré")]
    TokenExpired,
    /// Jeton jamais émis (ou déjà purgé).
    #[error("jeton de session inconnu")]
    TokenUnknown,
    /// La connexion n'a pas présenté le jeton d'amorçage.
    #[error("connexion non authentifiée")]
    Unauthenticated,
    /// Jeton d'amorçage faux.
    #[error("jeton d'amorçage Polkit invalide")]
    BadBootstrap,
    /// Le pair du socket n'est ni root ni l'utilisateur qui a passé le prompt.
    #[error("pair non autorisé (uid {0})")]
    PeerNotAllowed(u32),
}

impl IsolationError {
    /// Identifiant stable pour l'UI.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Busy(_) => "busy",
            Self::IdentityMismatch { .. } => "identity_mismatch",
            Self::OpenersFound(_) => "openers_found",
            Self::TokenExpired => "token_expired",
            Self::TokenUnknown => "token_unknown",
            Self::Unauthenticated => "unauthenticated",
            Self::BadBootstrap => "bad_bootstrap",
            Self::PeerNotAllowed(_) => "peer_not_allowed",
        }
    }
}

/// Erreur du helper, toutes couches confondues.
#[derive(Debug, thiserror::Error)]
pub enum HelperError {
    /// Refus d'isolation ou de session.
    #[error(transparent)]
    Isolation(#[from] IsolationError),
    /// Accès matériel.
    #[error("matériel: {0}")]
    Device(#[from] DeviceError),
    /// Moteur de décision ou canaris.
    #[error("moteur: {0}")]
    Core(#[from] CoreError),
    /// Message mal formé, trop long, ou commande hors séquence.
    #[error("protocole: {0}")]
    Protocol(String),
    /// Le moteur refuse la cible (système, lecture seule…).
    #[error("cible refusée: {0}")]
    Refused(String),
    /// Le moteur attend une intervention (verrouillé, sanitize en cours…).
    #[error("cible bloquée: {0}")]
    Blocked(String),
    /// Un job existe déjà pour cette session.
    #[error("un effacement est déjà en cours pour cette session")]
    JobRunning,
    /// Aucun job pour cette session.
    #[error("aucun effacement en cours pour cette session")]
    NoJob,
    /// E/S locale (socket, fichiers d'état).
    #[error("{context}: {source}")]
    Io {
        /// Ce qui était tenté.
        context: String,
        /// Erreur d'origine.
        #[source]
        source: std::io::Error,
    },
    /// Incohérence interne (jamais attendue).
    #[error("interne: {0}")]
    Internal(String),
    /// Sérialisation JSON d'une réponse.
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
}

impl HelperError {
    /// Attache un contexte à une erreur d'E/S.
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    /// Identifiant stable pour l'UI.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Isolation(e) => e.kind(),
            Self::Device(DeviceError::Busy(_)) => "busy",
            Self::Device(_) => "device",
            Self::Core(_) => "core",
            Self::Protocol(_) => "protocol",
            Self::Refused(_) => "refused",
            Self::Blocked(_) => "blocked",
            Self::JobRunning => "job_running",
            Self::NoJob => "no_job",
            Self::Io { .. } => "io",
            Self::Internal(_) => "internal",
            Self::Json(_) => "internal",
        }
    }
}

/// Résultat standard du helper.
pub type Result<T> = std::result::Result<T, HelperError>;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn kinds_are_stable_and_messages_name_the_cause() {
        let mismatch = IsolationError::IdentityMismatch {
            field: "serial",
            expected: "A".into(),
            found: "B".into(),
        };
        assert_eq!(mismatch.kind(), "identity_mismatch");
        assert!(mismatch.to_string().contains("serial"));
        let openers = IsolationError::OpenersFound(vec![Opener {
            pid: 42,
            comm: "dd".into(),
            fd: 3,
            target: "/dev/sdz".into(),
        }]);
        assert_eq!(openers.kind(), "openers_found");
        assert!(openers.to_string().contains("pid 42"));
        assert_eq!(
            HelperError::from(IsolationError::TokenExpired).kind(),
            "token_expired"
        );
        assert_eq!(
            HelperError::Device(DeviceError::Busy("/dev/sdz".into())).kind(),
            "busy"
        );
        assert_eq!(HelperError::JobRunning.kind(), "job_running");
    }
}
