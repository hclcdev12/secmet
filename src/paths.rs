//! Chemins configurables pour l'environnement d'exécution : helper
//! privilégié, policy PolicyKit, et dossier de données de l'application.
//!
//! Les données (réglages, logo, clé d'attestation, état des mises à jour,
//! journaux, certificats) vivent dans le compte utilisateur
//! (`~/.config/secmet`, `~/.secmet_logs`) ou, si `SECMET_DATA_DIR` est
//! défini (clé live, annexe M), dans `<dossier>/config` et `<dossier>/logs`.
//! Tout chemin de données passe par [`layout`].
use crate::app_identity::{DATA_DIR_ENV, DEFAULT_HELPER_PATH, HELPER_ENV, LOGS_DIR, PKG_NAME};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const POLICY_FILENAME: &str = "secmet.policy";
const POLICY_EXEC_PATH_KEY: &str = "org.freedesktop.policykit.exec.path";

pub fn helper_path() -> String {
    std::env::var(HELPER_ENV).unwrap_or_else(|_| DEFAULT_HELPER_PATH.to_string())
}

/// Emplacements des données de l'application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataLayout {
    /// Réglages, logo, clé d'attestation, état des mises à jour.
    pub config: PathBuf,
    /// Journaux de session, rapports, certificats.
    pub logs: PathBuf,
    /// Lots ouverts (`lots/<LOT-ID>/`, annexe M).
    pub lots: PathBuf,
    /// Lots clôturés.
    pub archive: PathBuf,
    /// Dossier dédié (`SECMET_DATA_DIR`) plutôt que le compte utilisateur.
    pub dedicated: bool,
}

/// Résolution pure : un dossier dédié absolu prime ; sinon
/// `<config_base>/secmet` et `<home>/.secmet_logs`, avec `~/.config` puis
/// `.` en repli.
pub fn resolve_layout(
    data_dir: Option<&Path>,
    config_base: Option<&Path>,
    home: Option<&Path>,
) -> DataLayout {
    if let Some(dir) = data_dir.filter(|d| d.is_absolute()) {
        return DataLayout {
            config: dir.join("config"),
            logs: dir.join("logs"),
            lots: dir.join("lots"),
            archive: dir.join("archive"),
            dedicated: true,
        };
    }
    let home = home
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let config_base = config_base
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".config"));
    let logs = home.join(LOGS_DIR);
    DataLayout {
        config: config_base.join(PKG_NAME),
        lots: logs.join("lots"),
        archive: logs.join("archive"),
        logs,
        dedicated: false,
    }
}

/// Emplacements effectifs, calculés une fois : la variable d'environnement
/// ne change pas en cours d'exécution. Un `SECMET_DATA_DIR` relatif est
/// ignoré, avec un avertissement.
pub fn layout() -> &'static DataLayout {
    static LAYOUT: OnceLock<DataLayout> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        let data = std::env::var_os(DATA_DIR_ENV)
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty());
        if let Some(p) = data.as_ref().filter(|p| !p.is_absolute()) {
            eprintln!(
                "{DATA_DIR_ENV}={} ignoré : chemin absolu attendu",
                p.display()
            );
        }
        resolve_layout(
            data.as_deref(),
            dirs::config_dir().as_deref(),
            dirs::home_dir().as_deref(),
        )
    })
}

/// Fichier de configuration `name` (`settings.json`, `pdf-logo.jpg`, …).
pub fn config_file(name: &str) -> PathBuf {
    layout().config.join(name)
}

/// Racine des journaux, rapports et certificats.
pub fn logs_dir() -> PathBuf {
    layout().logs.clone()
}

/// Lots ouverts.
pub fn lots_dir() -> PathBuf {
    layout().lots.clone()
}

/// Lots clôturés.
pub fn archive_dir() -> PathBuf {
    layout().archive.clone()
}

/// Les données vivent dans un dossier dédié (`SECMET_DATA_DIR`).
pub fn dedicated_data_dir() -> bool {
    layout().dedicated
}

/// Dossier `app/` de la partition de données (clé live) : paquets de mise
/// à jour vérifiés, installés au démarrage suivant. `None` hors clé.
pub fn app_store_dir() -> Option<PathBuf> {
    app_store_for(layout())
}

/// Résolution pure de [`app_store_dir`].
pub fn app_store_for(layout: &DataLayout) -> Option<PathBuf> {
    if !layout.dedicated {
        return None;
    }
    layout.config.parent().map(|root| root.join("app"))
}

/// Chemins candidats pour `secmet.policy`, dérivés du helper et des emplacements standards.
pub fn polkit_policy_candidates(helper: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(parent) = Path::new(helper).parent().and_then(|p| p.parent()) {
        candidates.push(parent.join("share/polkit-1/actions").join(POLICY_FILENAME));
    }
    candidates.push(PathBuf::from("/usr/share/polkit-1/actions").join(POLICY_FILENAME));
    candidates.push(PathBuf::from("/usr/local/share/polkit-1/actions").join(POLICY_FILENAME));
    candidates.dedup();
    candidates
}

/// Extrait `org.freedesktop.policykit.exec.path` depuis le XML de la policy.
pub fn parse_policy_exec_path(policy_xml: &str) -> Option<String> {
    for line in policy_xml.lines() {
        let trimmed = line.trim();
        if !trimmed.contains(POLICY_EXEC_PATH_KEY) {
            continue;
        }
        if let Some(start) = trimmed.find('>') {
            if let Some(end) = trimmed.rfind('<') {
                if end > start + 1 {
                    return Some(trimmed[start + 1..end].trim().to_string());
                }
            }
        }
    }
    None
}

/// Vérifie que le helper existe et que la policy Polkit pointe vers le même chemin.
pub fn verify_polkit_policy() -> Result<(), String> {
    let helper = helper_path();
    let helper_path = Path::new(&helper);

    if !helper_path.is_file() {
        return Err(format!(
            "Helper introuvable: {helper}\n\
             Installez-le avec: make dev"
        ));
    }

    let meta =
        std::fs::metadata(helper_path).map_err(|e| format!("Impossible de lire {helper}: {e}"))?;
    #[cfg(unix)]
    if meta.permissions().mode() & 0o111 == 0 {
        return Err(format!("Helper non exécutable: {helper}"));
    }

    let candidates = polkit_policy_candidates(&helper);
    let policy_path = candidates.iter().find(|p| p.is_file()).ok_or_else(|| {
        format!(
            "Policy Polkit introuvable ({POLICY_FILENAME}).\n\
                 Chemins testés:\n{}\n\
                 Installez-la avec: make dev",
            candidates
                .iter()
                .map(|p| format!("  - {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n")
        )
    })?;

    let policy_xml = std::fs::read_to_string(policy_path)
        .map_err(|e| format!("Impossible de lire {}: {e}", policy_path.display()))?;

    let exec_path = parse_policy_exec_path(&policy_xml).ok_or_else(|| {
        format!(
            "Policy {} sans annotation {POLICY_EXEC_PATH_KEY}",
            policy_path.display()
        )
    })?;

    if exec_path != helper {
        return Err(format!(
            "Policy Polkit incohérente:\n\
             helper={helper}\n\
             policy ({}) → exec.path={exec_path}\n\
             Relancez: make dev",
            policy_path.display()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_policy_exec_path_extracts_helper_path() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<policyconfig>
  <annotate key="org.freedesktop.policykit.exec.path">/usr/local/bin/secmet-helper</annotate>
</policyconfig>"#;
        assert_eq!(
            parse_policy_exec_path(xml).as_deref(),
            Some("/usr/local/bin/secmet-helper")
        );
    }

    #[test]
    fn parse_policy_exec_path_missing_returns_none() {
        assert!(parse_policy_exec_path("<policyconfig></policyconfig>").is_none());
    }

    #[test]
    fn layout_prefers_dedicated_absolute_dir() {
        let l = resolve_layout(
            Some(Path::new("/media/SECMET-DATA")),
            Some(Path::new("/home/u/.config")),
            Some(Path::new("/home/u")),
        );
        assert!(l.dedicated);
        assert_eq!(l.config, PathBuf::from("/media/SECMET-DATA/config"));
        assert_eq!(l.logs, PathBuf::from("/media/SECMET-DATA/logs"));
        assert_eq!(
            app_store_for(&l),
            Some(PathBuf::from("/media/SECMET-DATA/app"))
        );
        assert_eq!(l.lots, PathBuf::from("/media/SECMET-DATA/lots"));
        assert_eq!(l.archive, PathBuf::from("/media/SECMET-DATA/archive"));
        // Relatif : ignoré, comportement historique.
        let l = resolve_layout(
            Some(Path::new("data")),
            Some(Path::new("/home/u/.config")),
            Some(Path::new("/home/u")),
        );
        assert!(!l.dedicated);
        assert_eq!(l.config, PathBuf::from("/home/u/.config/secmet"));
        assert_eq!(l.logs, PathBuf::from("/home/u/.secmet_logs"));
        assert_eq!(l.lots, PathBuf::from("/home/u/.secmet_logs/lots"));
    }

    #[test]
    fn layout_falls_back_without_home() {
        let l = resolve_layout(None, None, Some(Path::new("/home/u")));
        assert_eq!(l.config, PathBuf::from("/home/u/.config/secmet"));
        let l = resolve_layout(None, None, None);
        assert_eq!(l.config, PathBuf::from("./.config/secmet"));
        assert_eq!(l.logs, PathBuf::from("./.secmet_logs"));
    }

    #[test]
    fn polkit_policy_candidates_from_local_helper() {
        let candidates = polkit_policy_candidates("/usr/local/bin/secmet-helper");
        assert!(candidates
            .iter()
            .any(|p| p == Path::new("/usr/local/share/polkit-1/actions/secmet.policy")));
        assert!(candidates
            .iter()
            .any(|p| p == Path::new("/usr/share/polkit-1/actions/secmet.policy")));
    }
}
