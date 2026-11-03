//! `secmet-helper` — bibliothèque du binaire privilégié.
//!
//! | Module        | Rôle                                                                     |
//! |---------------|--------------------------------------------------------------------------|
//! | [`ipc`]       | NDJSON borné à 1 Mio, types de messages stricts (`deny_unknown_fields`)  |
//! | [`polkit`]    | Jeton d'amorçage issu du prompt Polkit, `SO_PEERCRED`, droits du socket   |
//! | [`session`]   | Jetons de session à TTL, liés à `major:minor` + S/N + empreinte firmware  |
//! | [`identity`]  | Ré-identification d'un disque et construction des capacités du moteur     |
//! | [`isolation`] | Ouverture `O_EXCL | O_DIRECT`, protections fraîches, scan de `/proc/*/fd` |
//! | [`disk_target`] | Cible réelle : `O_DIRECT`, tampons alignés, commandes firmware de `secmet-device` |
//! | [`vault`]     | Mot de passe ATA Security enregistré durablement avant d'être posé        |
//! | [`hostinfo`]  | Inventaire du banc (DMI, CPU, mémoire) sans dmidecode ni lshw         |
//! | [`testdisk`]  | TestDisk en mode scripté sur le disque d'une session                    |
//! | [`runner`]    | Exécution d'un [`secmet_core::engine::ErasePlan`] avec canaris et progression |
//! | [`server`]    | Boucle d'acceptation, dispatch des commandes, jobs                        |
//!
//! Règles :
//! - le chemin `/dev/sdX` reçu de l'UI n'est jamais une preuve : avant toute
//!   action, le disque est ré-identifié (numéro de périphérique, S/N,
//!   empreinte firmware) et comparé au jeton de session ;
//! - aucune commande destructrice sans ouverture exclusive fraîche ;
//! - aucun `unwrap`/`expect`/`panic` en production (le profil release est en
//!   `panic = "abort"` : une panique tuerait un effacement en cours).

pub mod disk_target;
pub mod error;
pub mod hostinfo;
pub mod identity;
pub mod ipc;
pub mod isolation;
pub mod polkit;
pub mod recovery;
pub mod runner;
pub mod server;
pub mod session;
pub mod testdisk;
pub mod vault;

pub use error::{HelperError, IsolationError};
