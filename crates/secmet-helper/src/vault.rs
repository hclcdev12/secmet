//! Enregistrement durable du mot de passe ATA Security avant qu'il soit posé
//! sur le disque. Si le helper meurt entre SET PASSWORD et la fin de l'erase,
//! le mot de passe reste récupérable ici (fichier `0600` dans un répertoire
//! `0700`, `fsync` du fichier puis du répertoire avant la commande).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use secmet_device::ata::security::Password;

/// Répertoire par défaut.
pub const DEFAULT_DIR: &str = "/var/lib/secmet/ata-security";
/// Étiquette de la partition de données de la clé SecMet (Annexe M).
pub const DATA_LABEL: &str = "SECMET-DATA";
/// Sous-dossier du coffre sur la clé.
pub const KEY_VAULT_SUBDIR: &str = "vault";

/// Dossier effectif du coffre : `<montage SECMET-DATA>/vault` si la
/// partition de données d'une clé SecMet est montée, sinon `default`.
///
/// Sur un système live, `/var/lib` est en mémoire : un SECURITY ERASE
/// interrompu par une coupure laisserait le disque verrouillé avec un mot
/// de passe perdu au redémarrage. Sur la clé, le coffre suit la machine.
/// Aucune donnée du client n'intervient ; le dossier n'est utilisé que s'il
/// est sûr (voir [`secure_key_dir`]), sinon repli sur `default` avec un
/// avertissement.
pub fn resolve_dir(default: &Path) -> PathBuf {
    match key_mount().map(|m| (m.clone(), secure_key_dir(&m))) {
        Some((_, Ok(dir))) => dir,
        Some((mount, Err(e))) => {
            eprintln!(
                "coffre ATA : {} ignoré ({e}) ; repli sur {}",
                mount.display(),
                default.display()
            );
            default.to_path_buf()
        }
        None => default.to_path_buf(),
    }
}

/// Point de montage de la partition `SECMET-DATA`, si elle est montée.
pub fn key_mount() -> Option<PathBuf> {
    let device = fs::canonicalize(Path::new("/dev/disk/by-label").join(DATA_LABEL)).ok()?;
    let mounts = fs::read_to_string("/proc/self/mounts").ok()?;
    mounts.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let source = fields.next()?;
        let target = fields.next()?;
        (fs::canonicalize(source).ok()? == device)
            .then(|| PathBuf::from(target.replace("\\040", " ")))
    })
}

/// `<mount>/vault`, créé `0700` par root s'il n'existe pas. Refusé si la
/// racine du montage est inscriptible par d'autres que root (un utilisateur
/// pourrait remplacer `vault` par un lien), si `vault` est un lien
/// symbolique, n'appartient pas à root ou n'est pas en `0700`.
pub fn secure_key_dir(mount: &Path) -> io::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let root = fs::symlink_metadata(mount)?;
    if !root.is_dir() || root.uid() != 0 || root.mode() & 0o022 != 0 {
        return Err(io::Error::other(
            "racine du montage : propriété root et mode sans écriture groupe/autres attendus",
        ));
    }
    let dir = mount.join(KEY_VAULT_SUBDIR);
    match fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::DirBuilder::new().mode(0o700).create(&dir)?;
        }
        Err(e) => return Err(e),
        Ok(_) => {}
    }
    let meta = fs::symlink_metadata(&dir)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(io::Error::other(
            "vault : lien symbolique ou pas un dossier",
        ));
    }
    if meta.uid() != 0 || meta.mode() & 0o777 != 0o700 {
        return Err(io::Error::other(
            "vault : propriété root et mode 0700 attendus",
        ));
    }
    Ok(dir)
}

/// Coffre des mots de passe en attente.
#[derive(Debug, Clone)]
pub struct PasswordVault {
    dir: PathBuf,
}

impl PasswordVault {
    /// Coffre dans `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Répertoire.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file_for(&self, serial: &str) -> PathBuf {
        let safe: String = serial
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        self.dir.join(format!("{safe}.pw"))
    }

    fn ensure_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
    }

    /// Enregistre durablement le mot de passe de `serial`. Rend la main
    /// seulement quand le fichier et le répertoire sont synchronisés.
    pub fn store(&self, serial: &str, password: &Password) -> io::Result<PathBuf> {
        self.ensure_dir()?;
        let final_path = self.file_for(serial);
        let tmp_path = final_path.with_extension("pw.tmp");
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp_path)?;
            file.write_all(password.bytes())?;
            file.sync_all()?;
        }
        fs::rename(&tmp_path, &final_path)?;
        File::open(&self.dir)?.sync_all()?;
        Ok(final_path)
    }

    /// Relit le mot de passe de `serial`, s'il existe.
    pub fn load(&self, serial: &str) -> io::Result<Option<Password>> {
        let path = self.file_for(serial);
        let mut file = match File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut bytes = zeroize::Zeroizing::new([0u8; 32]);
        file.read_exact(bytes.as_mut())?;
        Ok(Some(Password::from_bytes(*bytes)))
    }

    /// Met de côté l'enregistrement de `serial` après un refus du disque :
    /// il n'est plus retenté automatiquement (le compteur de tentatives ATA
    /// est précieux) mais reste consultable (`<serial>.pw.failed`).
    pub fn mark_failed(&self, serial: &str) -> io::Result<()> {
        let path = self.file_for(serial);
        match fs::rename(&path, path.with_extension("pw.failed")) {
            Ok(()) => File::open(&self.dir)?.sync_all(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Retire l'enregistrement de `serial` (après un erase réussi).
    pub fn remove(&self, serial: &str) -> io::Result<()> {
        match fs::remove_file(self.file_for(serial)) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        File::open(&self.dir)?.sync_all()
    }

    /// Numéros de série (assainis) ayant un mot de passe en attente.
    pub fn pending(&self) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e),
        };
        for entry in entries.flatten() {
            if let Some(name) = entry
                .file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".pw"))
            {
                out.push(name.to_string());
            }
        }
        out.sort();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn store_load_remove_round_trip_with_restricted_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = PasswordVault::new(tmp.path().join("vault"));
        assert!(vault.pending().unwrap().is_empty());
        let password = Password::from_ascii("hunter2").unwrap();
        let path = vault.store("WD-WX/11 22", &password).unwrap();
        assert_eq!(path.file_name().unwrap(), "WD_WX_11_22.pw");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(vault.dir()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(vault.pending().unwrap(), ["WD_WX_11_22"]);
        let loaded = vault.load("WD-WX/11 22").unwrap().unwrap();
        assert_eq!(loaded.bytes(), password.bytes());
        assert!(vault.load("other").unwrap().is_none());
        vault.remove("WD-WX/11 22").unwrap();
        vault.remove("WD-WX/11 22").unwrap();
        assert!(vault.load("WD-WX/11 22").unwrap().is_none());
        assert!(!path.with_extension("pw.tmp").exists());
    }
}

#[cfg(test)]
mod key_dir_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Une racine de montage appartenant à l'utilisateur (ou inscriptible
    /// par d'autres) est refusée : le coffre reste sur `/var/lib`. Lancé
    /// sans privilège, le dossier temporaire appartient au testeur.
    #[test]
    fn user_owned_mount_root_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        if unsafe_is_root() {
            return; // un test root créerait un vrai dossier : non couvert ici
        }
        let err = secure_key_dir(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("racine du montage"));
        assert_eq!(
            resolve_dir(Path::new("/var/lib/secmet/x")),
            PathBuf::from("/var/lib/secmet/x")
        );
    }

    fn unsafe_is_root() -> bool {
        std::fs::metadata("/proc/self")
            .map(|m| {
                use std::os::unix::fs::MetadataExt;
                m.uid() == 0
            })
            .unwrap_or(false)
    }
}
