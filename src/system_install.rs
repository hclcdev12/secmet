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
//! d'installation est donc embarqué ici ([`INSTALLER_SCRIPT`]) et passé à
//! `bash -c` ; il extrait lui-même le composant depuis le fichier AppImage
//! (`$APPIMAGE`, un fichier ordinaire) dans un répertoire appartenant à root.

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
    } else {
        InstallNeed::Update
    }
}

/// Installe le composant système via pkexec (bloquant : prompt PolicyKit,
/// plafonné par le timeout de `pkexec_gate`). Le script embarqué reçoit le
/// chemin du fichier AppImage et en extrait le composant en root.
pub fn run_installer(components: &BundleComponents) -> Result<(), String> {
    let appimage = components.appimage.to_string_lossy().to_string();
    let output = crate::pkexec_gate::pkexec_output(&[
        "/bin/bash",
        "-c",
        INSTALLER_SCRIPT,
        "secmet-install",
        &appimage,
    ])
    .map_err(|e| format!("pkexec indisponible : {e}"))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Échec de l'installation du composant système (code {:?}).\n{}{}",
            output.status.code(),
            stdout.trim(),
            stderr.trim()
        ));
    }
    Ok(())
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
        assert!(
            !INSTALLER_SCRIPT.contains("$APPDIR"),
            "le script ne doit rien lire dans le montage FUSE"
        );
    }
}
