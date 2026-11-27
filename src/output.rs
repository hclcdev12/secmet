//! Dossier de sortie des certificats sur un poste installé : là où
//! l'utilisateur récupère ses PDF et leurs versions signées (.json).
//!
//! - Sans lot, les certificats y sont écrits directement.
//! - Avec un lot, ils sont écrits dans le dossier interne du lot (journal
//!   chaîné, manifeste), et ses livrables sont copiés dans
//!   `<sortie>/<LOT>/` à la clôture.
//!
//! Par défaut, la sortie est `~/Documents/SecMet-Certificats`, privé (0700).
//! L'utilisateur peut en choisir une autre dans les Réglages. Ce dossier
//! n'est jamais créé par SecMet, et ses fichiers gardent les droits
//! habituels : il a pu être choisi pour être partagé.
//!
//! Sur la clé live (dossier de données dédié), la sortie est la partition
//! d'export SECMET-EXP, gérée par [`crate::lot`] : le réglage ne s'y
//! applique pas.
//!
//! Tout accès au dossier choisi ([`resolve`], [`validate_choice`]) peut
//! bloquer sur un partage réseau qui ne répond plus. Ces fonctions
//! s'appellent hors du fil GTK, sauf [`validate_choice`] : l'utilisateur
//! vient de parcourir ce dossier.

use std::fs;
use std::path::{Path, PathBuf};

use crate::i18n::{tr, trf};

/// Dossier où écrire, et la raison du repli si le dossier choisi était
/// indisponible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub dir: PathBuf,
    /// `Some(raison)` : dossier choisi indisponible, certificats écrits
    /// dans le dossier par défaut.
    pub fallback: Option<String>,
}

impl Output {
    /// Phrase à ajouter au compte rendu quand le dossier choisi n'a pas pu
    /// servir.
    pub fn fallback_note(&self) -> Option<String> {
        self.fallback.as_ref().map(|reason| {
            trf(
                "Dossier choisi indisponible ({reason}) : enregistré dans {dir}",
                &[("reason", reason), ("dir", &self.dir.display().to_string())],
            )
        })
    }
}

/// Le réglage s'applique (poste installé, pas la clé live).
pub fn applies() -> bool {
    !crate::paths::dedicated_data_dir()
}

/// Dossier choisi dans les Réglages, s'il y en a un et s'il s'applique.
pub fn configured() -> Option<(PathBuf, Option<PathBuf>)> {
    if !applies() {
        return None;
    }
    let s = crate::settings::load();
    s.certificates_output
        .map(|dir| (dir, s.certificates_output_mount))
}

/// Dossier par défaut : `~/Documents/SecMet-Certificats`, créé en 0700.
pub fn default_dir() -> PathBuf {
    let dir = crate::paths::certificates_dir();
    let _ = crate::certificate::create_private_dir(&dir);
    dir
}

/// Dossier à afficher ou à ouvrir, sans toucher au dossier choisi (fil
/// GTK).
pub fn display_dir() -> PathBuf {
    match configured() {
        Some((dir, _)) => dir,
        None => default_dir(),
    }
}

/// Dossier où écrire maintenant (hors du fil GTK) : le dossier choisi s'il
/// est utilisable, sinon le dossier par défaut avec la raison.
pub fn resolve() -> Output {
    let Some((dir, mount)) = configured() else {
        return Output {
            dir: default_dir(),
            fallback: None,
        };
    };
    match check_usable(&dir, mount.as_deref()) {
        Ok(()) => Output {
            dir,
            fallback: None,
        },
        Err(reason) => Output {
            dir: default_dir(),
            fallback: Some(reason),
        },
    }
}

/// Le dossier existe, il est sur le même support qu'au moment du choix,
/// et on peut y écrire.
fn check_usable(dir: &Path, mount: Option<&Path>) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(tr("dossier introuvable"));
    }
    if let Some(expected) = mount {
        // Support démonté : le chemin retombe sur un dossier vide du
        // disque système, où il ne faut pas écrire.
        if expected != Path::new("/") && current_mount_point(dir).as_deref() != Some(expected) {
            return Err(tr("support non monté"));
        }
    }
    probe_write(dir)
}

/// Crée puis supprime un fichier témoin.
fn probe_write(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".secmet-test-{}", std::process::id()));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|e| {
            trf(
                "écriture impossible : {error}",
                &[("error", &e.to_string())],
            )
        })?;
    let _ = fs::remove_file(&probe);
    Ok(())
}

/// Résultat du choix d'un dossier dans les Réglages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub dir: PathBuf,
    /// Point de montage du dossier, retenu pour repérer plus tard un
    /// support démonté.
    pub mount: Option<PathBuf>,
    /// Avertissement à montrer (dossier de développement).
    pub warning: Option<String>,
}

/// Contrôle un dossier choisi : refusé s'il est dans les données de SecMet
/// (le nettoyage ou l'archivage pourrait l'effacer) ou si on ne peut pas y
/// écrire ; averti s'il ressemble à un dossier de développement.
pub fn validate_choice(dir: &Path) -> Result<Choice, String> {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let l = crate::paths::layout();
    if inside_data(&dir, &[&l.config, &l.logs, &l.lots, &l.archive]) {
        return Err(tr(
            "Ce dossier fait partie des données internes de SecMet : choisissez-en un autre.",
        ));
    }
    probe_write(&dir)?;
    let warning = dev_tree(&dir).then(|| {
        tr(
            "Ce dossier semble être un dossier de développement (dépôt git ou projet Rust) : \
            les certificats s'y mêleront aux sources.",
        )
    });
    Ok(Choice {
        mount: current_mount_point(&dir),
        dir,
        warning,
    })
}

/// `dir` est l'un des dossiers de données, ou se trouve dedans.
pub fn inside_data(dir: &Path, data: &[&PathBuf]) -> bool {
    data.iter().any(|d| {
        let d = d.canonicalize().unwrap_or_else(|_| d.to_path_buf());
        dir.starts_with(&d)
    })
}

/// `dir` ou l'un de ses parents contient `.git` ou `Cargo.toml`.
fn dev_tree(dir: &Path) -> bool {
    dir.ancestors()
        .any(|a| a.join(".git").exists() || a.join("Cargo.toml").is_file())
}

/// Point de montage qui contient `path`, d'après `/proc/self/mountinfo`.
pub fn current_mount_point(path: &Path) -> Option<PathBuf> {
    let path = path.canonicalize().ok()?;
    let info = fs::read_to_string("/proc/self/mountinfo").ok()?;
    mount_point_of(&path, &info)
}

/// Résolution pure : le point de montage le plus long qui préfixe `path`.
pub fn mount_point_of(path: &Path, mountinfo: &str) -> Option<PathBuf> {
    mountinfo
        .lines()
        .filter_map(|line| line.split(' ').nth(4))
        .map(|field| PathBuf::from(unescape_mount(field)))
        .filter(|mount| path.starts_with(mount))
        .max_by_key(|mount| mount.as_os_str().len())
}

/// `/proc/self/mountinfo` code espaces et barres obliques inverses en
/// octal (`\040`).
fn unescape_mount(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 4 <= bytes.len() {
            if let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 4])
                .ok()
                .and_then(|d| u8::from_str_radix(d, 8).ok())
            {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = "\
22 1 0:21 / / rw,relatime shared:1 - btrfs /dev/nvme0n1p3 rw
40 22 0:35 / /home rw,relatime shared:2 - btrfs /dev/nvme0n1p3 rw
80 40 8:17 / /media/u/CLE\\040USB rw,nosuid - exfat /dev/sdb1 rw
90 22 0:50 / /mnt/partage rw - cifs //srv/partage rw
";

    #[test]
    fn mount_point_is_longest_prefix() {
        let m = |p: &str| mount_point_of(Path::new(p), MOUNTINFO).unwrap();
        assert_eq!(m("/home/u/Documents"), PathBuf::from("/home"));
        assert_eq!(
            m("/media/u/CLE USB/certs"),
            PathBuf::from("/media/u/CLE USB")
        );
        assert_eq!(m("/mnt/partage/x"), PathBuf::from("/mnt/partage"));
        // Partage démonté : le même chemin tombe sur la racine.
        assert_eq!(m("/mnt/partageX"), PathBuf::from("/"));
    }

    #[test]
    fn data_folders_are_refused() {
        let logs = PathBuf::from("/home/u/.secmet_logs");
        let data = [&logs];
        assert!(inside_data(Path::new("/home/u/.secmet_logs"), &data));
        assert!(inside_data(
            Path::new("/home/u/.secmet_logs/archive/x"),
            &data
        ));
        assert!(!inside_data(Path::new("/home/u/.secmet_logs2"), &data));
        assert!(!inside_data(Path::new("/home/u/Documents"), &data));
    }

    #[test]
    fn usable_folder_and_missing_folder() {
        let tmp = std::env::temp_dir().join(format!("secmet-output-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        assert!(check_usable(&tmp, None).is_ok());
        // Aucun fichier témoin laissé derrière.
        assert_eq!(fs::read_dir(&tmp).unwrap().count(), 0);
        assert!(check_usable(&tmp.join("absent"), None).is_err());
        // Monté ailleurs qu'au moment du choix : refusé.
        assert!(check_usable(&tmp, Some(Path::new("/nulle/part"))).is_err());
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn dev_tree_is_detected() {
        let tmp = std::env::temp_dir().join(format!("secmet-dev-{}", std::process::id()));
        let inner = tmp.join("a/b");
        fs::create_dir_all(&inner).unwrap();
        fs::write(tmp.join("Cargo.toml"), b"").unwrap();
        assert!(dev_tree(&inner));
        fs::remove_dir_all(&tmp).unwrap();
    }
}
