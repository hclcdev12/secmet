//! Identité applicative centralisée (nom, chemins, variables d'environnement).

pub const APP_NAME: &str = "SecMet";
pub const APP_ID: &str = "com.secmet";
pub const PKG_NAME: &str = "secmet";
pub const LOGS_DIR: &str = ".secmet_logs";
pub const LEGACY_LOGS_DIR: &str = ".shred_graphique_logs";
pub const HELPER_ENV: &str = "SECMET_HELPER";
/// Dossier de données dédié (clé live, annexe M) : `config/` et `logs/` y
/// remplacent `~/.config/secmet` et `~/.secmet_logs`.
pub const DATA_DIR_ENV: &str = "SECMET_DATA_DIR";
pub const CERT_ID_PREFIX: &str = "SEC";
pub const DEFAULT_HELPER_PATH: &str = "/usr/bin/secmet-helper";
