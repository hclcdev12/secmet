//! `secmet-device` — accès direct au matériel de stockage, sans outil externe.
//!
//! Cette crate remplace `hdparm`, `nvme-cli`, `smartctl`, `lsblk` et `blkid`
//! par des appels système Rust :
//!
//! | Besoin SecMet                     | Avant (CLI)              | Maintenant                         |
//! |-----------------------------------|--------------------------|------------------------------------|
//! | IDENTIFY, SMART, SANITIZE, Security | `hdparm`, `smartctl`   | [`ata`] : ATA PASS-THROUGH via `SG_IO` |
//! | Ponts USB-SATA                    | `smartctl -d sat`        | [`ata`] : repli automatique CDB 16 → 12 |
//! | Identify / Format / Sanitize NVMe | `nvme-cli`               | [`nvme`] : `NVME_IOCTL_ADMIN_CMD`  |
//! | Inventaire disques, transport     | `lsblk`                  | [`sysfs`]                          |
//! | Disque système, montages, swap    | `/proc/mounts` + `lsblk -s` | [`protection`] (mountinfo + `slaves/`) |
//! | Signatures résiduelles            | `blkid`                  | [`signatures`]                     |
//! | Dé-frozen ATA                     | `echo mem > /sys/power/state` | [`defrozen`]                   |
//!
//! Règles de la crate :
//! - tout `unsafe` vit dans [`sys`] (les ioctl eux-mêmes) et [`aligned`]
//!   (allocation alignée), chaque bloc porte un commentaire `SAFETY:` et les
//!   layouts sont épinglés par des tests de taille et d'offset ;
//! - aucun `unwrap()` ni `expect()` en production : toute défaillance remonte
//!   en [`DeviceError`] ;
//! - rien ici ne décide d'effacer : la crate expose des primitives, la
//!   politique (refus du disque système, confirmation…) reste dans le helper.
//!
//! Plateforme : Linux x86_64 uniquement (layouts UAPI 64 bits).

#![cfg_attr(
    not(all(target_os = "linux", target_pointer_width = "64")),
    allow(unused)
)]

#[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
compile_error!(
    "secmet-device cible Linux 64 bits uniquement (layouts sg_io_hdr / nvme_passthru_cmd)"
);

pub mod aligned;
pub mod ata;
pub mod block;
pub mod defrozen;
pub mod nvme;
pub mod protection;
pub mod signatures;
pub mod sys;
pub mod sysfs;

use std::path::PathBuf;

/// Erreur d'accès au matériel. Chaque variante nomme la couche fautive.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    /// Appel système ou lecture de fichier en échec, avec le contexte.
    #[error("{context}: {source}")]
    Io {
        /// Ce qui était tenté (chemin, ioctl…).
        context: String,
        /// Erreur d'origine.
        #[source]
        source: std::io::Error,
    },
    /// Le chemin ne désigne pas un périphérique bloc.
    #[error("{0} n'est pas un périphérique bloc")]
    NotBlockDevice(PathBuf),
    /// Nom de périphérique hors de l'allowlist (`sd*`, `nvme*n*`, `vd*`, `xvd*`).
    #[error("nom de périphérique refusé: {0}")]
    RefusedName(String),
    /// Le noyau refuse l'ouverture exclusive : monté, swap, membre dm/md…
    #[error("périphérique occupé (O_EXCL): {0}")]
    Busy(PathBuf),
    /// Échec au niveau SCSI (transport, hôte ou pilote), sans registres ATA.
    #[error("SG_IO: statut SCSI 0x{status:02x}, hôte 0x{host:04x}, pilote 0x{driver:04x}{detail}")]
    Scsi {
        /// Statut SCSI (0x02 = CHECK CONDITION).
        status: u8,
        /// `host_status` du pilote SCSI.
        host: u16,
        /// `driver_status`.
        driver: u16,
        /// Clé de sense, ASC et ASCQ si le sense en porte.
        sense: Option<ata::SenseCode>,
        /// Les mêmes, formatés pour l'affichage.
        detail: String,
    },
    /// Le disque a abandonné la commande ATA (bit ERR du registre Status).
    #[error(
        "commande ATA 0x{command:02x} abandonnée (error 0x{error:02x}, status 0x{status:02x})"
    )]
    AtaAborted {
        /// Code de commande ATA.
        command: u8,
        /// Registre Error.
        error: u8,
        /// Registre Status.
        status: u8,
        /// Registre LBA renvoyé avec l'abandon (motif pour SANITIZE DEVICE).
        lba: u64,
    },
    /// Le disque a abandonné une commande SANITIZE DEVICE en donnant son motif.
    #[error("commande SANITIZE refusée : {0}")]
    SanitizeRefused(ata::SanitizeRefusal),
    /// Le transfert de données ATA n'est pas complet.
    #[error("transfert ATA incomplet ({0} octets non transférés)")]
    IncompleteTransfer(i32),
    /// Commande 48 bits ou champs trop larges pour ATA PASS-THROUGH (12).
    #[error("commande ATA 0x{0:02x} non exprimable en ATA PASS-THROUGH (12)")]
    NotRepresentable12(u8),
    /// Le contrôleur NVMe a rendu un statut d'erreur.
    #[error("statut NVMe 0x{code:03x} ({meaning})")]
    NvmeStatus {
        /// Statut (SCT << 8 | SC).
        code: u16,
        /// Signification, pour les codes connus.
        meaning: &'static str,
    },
    /// Structure firmware illisible.
    #[error(transparent)]
    Decode(#[from] DecodeError),
    /// Fonction annoncée non supportée par ce disque.
    #[error("non supporté par le disque: {0}")]
    Unsupported(&'static str),
    /// Paramètre refusé avant tout appel système.
    #[error("argument invalide: {0}")]
    InvalidArgument(String),
    /// Inventaire sysfs incohérent.
    #[error("inventaire sysfs: {0}")]
    Inventory(String),
    /// Un montage, un swap ou un volume n'a pas pu être rattaché à un disque.
    #[error("montage non traçable (par prudence, tout est considéré protégé): {0}")]
    Untraceable(String),
    /// Mise en veille impossible.
    #[error("mise en veille: {0}")]
    Power(String),
}

/// Pourquoi une structure firmware n'a pas pu être décodée.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// Tampon plus court que la structure.
    #[error("{structure}: {actual} octets reçus, {expected} attendus")]
    TooShort {
        /// Nom de la structure.
        structure: &'static str,
        /// Taille attendue.
        expected: usize,
        /// Taille reçue.
        actual: usize,
    },
    /// Somme de contrôle fausse.
    #[error("{0}: somme de contrôle invalide")]
    Checksum(&'static str),
    /// Valeur hors spécification.
    #[error("{structure}: {reason}")]
    Invalid {
        /// Nom de la structure.
        structure: &'static str,
        /// Détail.
        reason: &'static str,
    },
}

impl DeviceError {
    /// Attache un contexte à une erreur d'E/S.
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

/// Résultat standard de la crate.
pub type Result<T> = std::result::Result<T, DeviceError>;

/// Vérifie qu'un tampon atteint la taille d'une structure.
pub(crate) fn require(data: &[u8], expected: usize, structure: &'static str) -> Result<()> {
    if data.len() < expected {
        return Err(DecodeError::TooShort {
            structure,
            expected,
            actual: data.len(),
        }
        .into());
    }
    Ok(())
}

/// Chaîne ASCII imprimable, espaces de bourrage retirés.
pub(crate) fn ascii_trimmed(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                ' '
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}
