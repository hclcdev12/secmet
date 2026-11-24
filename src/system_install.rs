//! Installation / mise à jour du composant système depuis une AppImage.
//!
//! Une AppImage ne peut pas embarquer le composant privilégié : pkexec exige
//! un binaire setuid présent sur l'hôte, polkitd ne lit les policies que dans
//! `/usr/share/polkit-1/actions` (chemin compilé en dur) et le point de
//! montage AppImage change à chaque lancement. L'AppImage transporte donc
//! helper + policy + bundle d'outils et les installe sur l'hôte via pkexec
//! au premier démarrage.
//!
//! Le montage FUSE d'une AppImage n'est lisible que par l'utilisateur qui
//! l'a lancée : root ne peut y lire ni script ni binaire. Le script
//! d'installation est donc embarqué ici ([`INSTALLER_SCRIPT`]) et passé sur
//! l'entrée standard de `bash -s` ; il extrait lui-même le composant depuis
//! le fichier AppImage (`$APPIMAGE`, un fichier ordinaire) dans un
//! répertoire appartenant à root, puis se remplace par `secmet-helper serve`
//! ([`INSTALL_AND_SERVE`]) : un seul mot de passe pour installer et démarrer
//! (voir `helper_client::ensure_installing`).

use crate::app_identity::HELPER_ENV;
use crate::paths;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Variables d'environnement posées par le runtime AppImage.
const APPDIR_ENV: &str = "APPDIR";
const APPIMAGE_ENV: &str = "APPIMAGE";

pub const HELPER_REL: &str = "usr/bin/secmet-helper";

/// Script d'installation exécuté en root (voir `packaging/install-helper.sh`).
pub const INSTALLER_SCRIPT: &str = include_str!("../packaging/install-helper.sh");

/// Option du script : démarrer le helper une fois le composant installé.
pub const INSTALL_AND_SERVE: &str = "--serve";

/// Composant système embarqué dans l'AppImage courante.
#[derive(Debug, Clone)]
pub struct BundleComponents {
    /// Montage de l'AppImage (lecture côté utilisateur : empreinte du helper).
    pub appdir: PathBuf,
    /// Fichier AppImage lui-même (lecture côté root : extraction).
    pub appimage: PathBuf,
    pub helper: PathBuf,
}

/// `Some` si l'app tourne depuis une AppImage transportant le composant.
pub fn bundled_components() -> Option<BundleComponents> {
    let appdir = PathBuf::from(std::env::var(APPDIR_ENV).ok()?);
    let appimage = PathBuf::from(std::env::var(APPIMAGE_ENV).ok()?);
    let components = BundleComponents {
        helper: appdir.join(HELPER_REL),
        appdir,
        appimage,
    };
    (components.helper.is_file() && components.appimage.is_file()).then_some(components)
}

/// État du composant système côté hôte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallNeed {
    /// Helper + policy installés et identiques au bundle : rien à faire.
    UpToDate,
    /// Composant absent ou policy manquante/incohérente : installation requise.
    Install,
    /// Composant présent mais différent du binaire embarqué (AppImage mise à jour).
    Update,
    /// Composant installé plus récent que celui de cette AppImage : le
    /// remplacer ferait revenir la machine en arrière.
    Downgrade,
}

/// Version du composant embarqué (même version que l'application).
pub const BUNDLED_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version du helper installé (`secmet-helper version`, sans privilège) ;
/// `None` pour un helper antérieur à la 0.5.1, qui ne la donne pas.
pub fn installed_version() -> Option<String> {
    let out = std::process::Command::new(paths::helper_path())
        .arg("version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let v = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !v.is_empty()).then_some(v)
}

/// `a` est-elle strictement plus récente que `b` ? Compare les nombres de
/// `X.Y.Z` ; une préversion (`-rc1`) précède la version finale.
pub fn version_newer(a: &str, b: &str) -> bool {
    fn parse(v: &str) -> (Vec<u64>, bool) {
        let (core, pre) = match v.split_once('-') {
            Some((c, _)) => (c, true),
            None => (v, false),
        };
        (
            core.split('.').map(|n| n.parse().unwrap_or(0)).collect(),
            pre,
        )
    }
    let (na, pa) = parse(a);
    let (nb, pb) = parse(b);
    match na.cmp(&nb) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => pb && !pa,
    }
}

/// Détermine l'action requise. Ne retourne `Update` que si le helper résolu
/// est le chemin par défaut (pas de `SECMET_HELPER` custom, scénario dev).
pub fn check_install_needed(components: &BundleComponents) -> InstallNeed {
    if paths::verify_polkit_policy().is_err() {
        return InstallNeed::Install;
    }
    if std::env::var(HELPER_ENV).is_ok() {
        return InstallNeed::UpToDate;
    }
    let installed = file_sha256(Path::new(&paths::helper_path()));
    let bundled = file_sha256(&components.helper);
    if installed.is_some() && installed == bundled {
        InstallNeed::UpToDate
    } else if installed_version().is_some_and(|v| version_newer(&v, BUNDLED_VERSION)) {
        InstallNeed::Downgrade
    } else {
        InstallNeed::Update
    }
}

fn file_sha256(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Some(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_order() {
        assert!(version_newer("0.6.0", "0.5.1"));
        assert!(version_newer("0.5.10", "0.5.9"));
        assert!(version_newer("0.5.1", "0.5.1-rc2"));
        assert!(!version_newer("0.5.1-rc2", "0.5.1"));
        assert!(!version_newer("0.5.1", "0.5.1"));
        assert!(!version_newer("0.4.9", "0.5.0"));
    }

    #[test]
    fn test_file_sha256_missing_file_is_none() {
        assert_eq!(file_sha256(Path::new("/n'existe/pas/secmet")), None);
    }

    #[test]
    fn test_file_sha256_empty_content() {
        let path = std::env::temp_dir().join(format!("secmet-sha-test-{}", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        let digest = file_sha256(&path);
        let _ = std::fs::remove_file(&path);
        // SHA-256 de la chaîne vide (vecteur de référence NIST).
        assert_eq!(
            digest.as_deref(),
            Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
    }

    #[test]
    fn test_check_install_needed_without_helper_requires_install() {
        // Ni helper installé au chemin par défaut, ni SECMET_HELPER, ni
        // bundle utilisable → une installation est requise.
        if Path::new(&paths::helper_path()).exists() || std::env::var(HELPER_ENV).is_ok() {
            return; // machine de dev avec helper installé : hors périmètre
        }
        let components = BundleComponents {
            appdir: PathBuf::from("/n'existe/pas"),
            appimage: PathBuf::from("/n'existe/pas/SecMet.AppImage"),
            helper: PathBuf::from("/n'existe/pas/usr/bin/secmet-helper"),
        };
        assert_eq!(check_install_needed(&components), InstallNeed::Install);
    }

    #[test]
    fn embedded_installer_extracts_from_the_appimage_file_not_the_mount() {
        assert!(INSTALLER_SCRIPT.starts_with("#!/usr/bin/env bash"));
        assert!(INSTALLER_SCRIPT.contains("--appimage-extract"));
        assert!(INSTALLER_SCRIPT.contains("/usr/bin/secmet-helper"));
        assert!(INSTALLER_SCRIPT.contains("/usr/share/polkit-1/actions/secmet.policy"));
        // Installation puis démarrage du helper sous la même invite, sortie
        // standard réservée à sa ligne « ready ».
        assert!(INSTALLER_SCRIPT.contains(INSTALL_AND_SERVE));
        assert!(INSTALLER_SCRIPT.contains("exec 3>&1 1>&2"));
        assert!(INSTALLER_SCRIPT.contains("exec /usr/bin/secmet-helper serve 1>&3"));
        assert!(
            !INSTALLER_SCRIPT.contains("$APPDIR"),
            "le script ne doit rien lire dans le montage FUSE"
        );
    }
}
