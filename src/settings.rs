//! Préférences persistantes de l'application (`settings.json` dans le dossier
//! de configuration, voir [`crate::paths::layout`]).
//! Passes d'écriture hôte, langue de l'interface, logo des certificats PDF,
//! nom de l'opérateur par défaut, vérification des mises à jour.
//! Structure extensible sans migration (champs inconnus ignorés, défauts à la manquante).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// Passes aléatoires shred pour les HDD (défaut 1 = NIST Clear).
    #[serde(default = "default_hdd_passes")]
    pub hdd_passes: u8,
    /// Langue de l'interface (`fr`, `en`, `de`).
    #[serde(default = "default_language")]
    pub language: String,
    /// Logo JPEG des certificats PDF (converti par l'application) ; `None` :
    /// logo SecMet.
    #[serde(default)]
    pub pdf_logo: Option<PathBuf>,
    /// Nom de l'opérateur pré-rempli à la génération des certificats ;
    /// vide : saisie à chaque fois.
    #[serde(default)]
    pub operator_name: String,
    /// Mode lot sur un poste installé (portée « disques ») ; sur la clé
    /// live (dossier de données dédié) les lots sont toujours actifs.
    #[serde(default)]
    pub lots_enabled: bool,
    /// Vérification automatique des mises à jour (une requête HTTPS par
    /// jour vers le canal de publication, voir [`crate::update`]).
    #[serde(default = "default_true")]
    pub check_updates: bool,
    /// Adresse de `latest.json` remplaçant le canal par défaut (cas
    /// particuliers : miroir interne) ; `None` : canal par défaut.
    #[serde(default)]
    pub update_url: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_language() -> String {
    "fr".to_string()
}

/// Passes pseudo-aléatoires par défaut pour l'écriture hôte (NIST Clear).
pub const DEFAULT_HDD_PASSES: u8 = 1;

fn default_hdd_passes() -> u8 {
    DEFAULT_HDD_PASSES
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            hdd_passes: default_hdd_passes(),
            language: default_language(),
            pdf_logo: None,
            operator_name: String::new(),
            lots_enabled: false,
            check_updates: true,
            update_url: None,
        }
    }
}

pub fn settings_path() -> Option<PathBuf> {
    Some(crate::paths::config_file("settings.json"))
}

/// Charge les réglages ; toute erreur (fichier absent, JSON invalide, champ
/// aberrant) retombe sur les défauts — l'app ne doit jamais refuser de
/// démarrer à cause des préférences.
pub fn load() -> Settings {
    let Some(path) = settings_path() else {
        return Settings::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Sauvegarde silencieuse : un échec d'écriture ne bloque pas l'opération.
pub fn save(settings: &Settings) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(path, json);
    }
}

/// Bornes acceptées pour les passes shred (1 = recommandé, 3 = historique).
pub fn clamp_hdd_passes(passes: u8) -> u8 {
    if passes == 0 {
        default_hdd_passes()
    } else {
        passes.min(3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_nist_and_parse_tolerant() {
        let s = Settings::default();
        assert_eq!(s.hdd_passes, 1);

        // Champ absent → défaut ; champ inconnu → ignoré.
        let parsed: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed.hdd_passes, 1);
        let parsed: Settings = serde_json::from_str(r#"{"hdd_passes":3,"future":"x"}"#).unwrap();
        assert_eq!(parsed.hdd_passes, 3);
        assert_eq!(parsed.language, "fr");
        assert_eq!(parsed.pdf_logo, None);
        assert_eq!(parsed.operator_name, "");
        let parsed: Settings =
            serde_json::from_str(r#"{"language":"de","pdf_logo":"/tmp/logo.jpg"}"#).unwrap();
        assert_eq!(parsed.language, "de");
        assert_eq!(
            parsed.pdf_logo.as_deref(),
            Some(std::path::Path::new("/tmp/logo.jpg"))
        );
        let parsed: Settings = serde_json::from_str(r#"{"operator_name":"Jean Dupont"}"#).unwrap();
        assert_eq!(parsed.operator_name, "Jean Dupont");
        assert!(parsed.check_updates);
        assert_eq!(parsed.update_url, None);
        let parsed: Settings =
            serde_json::from_str(r#"{"check_updates":false,"update_url":"https://x/latest.json"}"#)
                .unwrap();
        assert!(!parsed.check_updates);
        assert_eq!(parsed.update_url.as_deref(), Some("https://x/latest.json"));
    }

    #[test]
    fn clamp_bounds() {
        assert_eq!(clamp_hdd_passes(0), 1);
        assert_eq!(clamp_hdd_passes(1), 1);
        assert_eq!(clamp_hdd_passes(3), 3);
        assert_eq!(clamp_hdd_passes(7), 3);
    }
}
