//! `secmet-core` — moteur de décision NIST SP 800-88 et canaris de validation.
//!
//! Crate **pure** : aucune I/O, aucun appel système, aucun `unsafe`. Chaque
//! fonction est déterministe et testable en unitaire.
//!
//! | Module         | Rôle                                                                 |
//! |----------------|----------------------------------------------------------------------|
//! | [`taxonomy`]   | Niveaux NIST, méthodes d'effacement, capacités d'une cible           |
//! | [`engine`]     | Plan d'action : méthode retenue, replis, dégradation, critères d'échec |
//! | [`canary`]     | Échantillonnage, motifs ChaCha20 par LBA, validation post-effacement  |
//! | [`disposition`]| Verdict final : réemploi, destruction recommandée ou requise, relance |
//! | `from_device`  | (feature `device`) conversion depuis `secmet-device`                 |
//!
//! Règles invariantes :
//! 1. une écriture depuis l'hôte n'est jamais classée *Purge*, quel que soit
//!    le nombre de passes ;
//! 2. *Destroy* n'est jamais exécuté par le logiciel, seulement recommandé ;
//! 3. toute dégradation Purge → Clear est explicite dans le plan ;
//! 4. une cible protégée (système, montée, swap, volume) est refusée avant
//!    toute autre considération.

#![forbid(unsafe_code)]

pub mod canary;
pub mod disposition;
pub mod engine;
pub mod taxonomy;

#[cfg(feature = "device")]
pub mod from_device;

/// Erreurs du moteur et des canaris.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CoreError {
    /// La cible n'a aucun bloc.
    #[error("cible sans bloc adressable")]
    NoBlocks,
    /// Taille de bloc nulle ou non multiple de 4 (les motifs travaillent par mots).
    #[error("taille de bloc invalide: {0}")]
    InvalidBlockSize(u64),
    /// Nombre de passes hors de 1..=16.
    #[error("nombre de passes invalide: {0} (1 à 16)")]
    PassesOutOfRange(u8),
    /// Échantillonnage sous le minimum de la politique.
    #[error("{requested} échantillons demandés, {minimum} au minimum")]
    TooFewSamples {
        /// Demandé.
        requested: u32,
        /// Minimum de la politique.
        minimum: u32,
    },
    /// Un bloc fourni à la validation n'a pas la taille de bloc annoncée.
    #[error("bloc LBA {lba}: {actual} octets au lieu de {expected}")]
    BlockSizeMismatch {
        /// LBA du bloc.
        lba: u64,
        /// Taille attendue.
        expected: u64,
        /// Taille reçue.
        actual: usize,
    },
    /// Un LBA de validation n'appartient pas à l'échantillon prévu.
    #[error("LBA {0} hors de l'échantillon")]
    UnexpectedLba(u64),
}

/// Résultat standard.
pub type Result<T> = std::result::Result<T, CoreError>;
