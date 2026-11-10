//! Clé de signature du poste : un seed Ed25519 de 32 octets en hexadécimal
//! dans un fichier `0600`, créé au premier usage, avec la clé publique à
//! côté (`.pub`) pour la distribuer aux vérificateurs.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::sign::SigningIdentity;
use crate::{hex, unhex, AttestError, Result};

/// Nom de fichier par défaut.
pub const KEY_FILE: &str = "attestation-key.hex";

/// Chemin par défaut : `$XDG_CONFIG_HOME/secmet/attestation-key.hex` ou
/// `~/.config/secmet/attestation-key.hex`.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("secmet").join(KEY_FILE))
}

/// Chemin de la clé publique associée.
pub fn public_key_path(key_path: &Path) -> PathBuf {
    key_path.with_extension("pub")
}

fn random_seed() -> Result<Zeroizing<[u8; 32]>> {
    let mut seed = Zeroizing::new([0u8; 32]);
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(seed.as_mut()))
        .map_err(|e| AttestError::io("/dev/urandom", e))?;
    Ok(seed)
}

/// Charge la clé de `path`, ou la crée (répertoire `0700`, fichier `0600`,
/// `fsync`) et écrit la clé publique à côté.
pub fn load_or_create(path: &Path) -> Result<SigningIdentity> {
    match fs::read_to_string(path) {
        Ok(text) => {
            #[cfg(unix)]
            if let Ok(meta) = fs::metadata(path) {
                if meta.permissions().mode() & 0o077 != 0 {
                    return Err(AttestError::Invalid(format!(
                        "{} : droits {:o} trop ouverts (0600 attendu)",
                        path.display(),
                        meta.permissions().mode() & 0o777
                    )));
                }
            }
            let bytes = unhex(text.trim())
                .filter(|b| b.len() == 32)
                .ok_or_else(|| {
                    AttestError::Signature(format!("{}: seed illisible", path.display()))
                })?;
            let mut seed = Zeroizing::new([0u8; 32]);
            seed.copy_from_slice(&bytes);
            let identity = SigningIdentity::from_seed(&seed);
            let pub_path = public_key_path(path);
            if !pub_path.exists() {
                let _ = fs::write(&pub_path, format!("{}\n", identity.public_key_hex()));
            }
            Ok(identity)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => create(path),
        Err(e) => Err(AttestError::io(path.display().to_string(), e)),
    }
}

fn create(path: &Path) -> Result<SigningIdentity> {
    let dir = path
        .parent()
        .ok_or_else(|| AttestError::Invalid(format!("{}: pas de répertoire", path.display())))?;
    fs::create_dir_all(dir).map_err(|e| AttestError::io(dir.display().to_string(), e))?;
    #[cfg(unix)]
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| AttestError::io(format!("chmod {}", dir.display()), e))?;
    let seed = random_seed()?;
    let identity = SigningIdentity::from_seed(&seed);
    // Création exclusive du fichier final : deux instances lancées avant
    // toute clé ne peuvent pas se remplacer l'une l'autre ; la seconde
    // relit la clé de la première.
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return load_or_create(path),
        Err(e) => return Err(AttestError::io(path.display().to_string(), e)),
    };
    let text = Zeroizing::new(format!("{}\n", hex(seed.as_ref())));
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| AttestError::io(path.display().to_string(), e))?;
    drop(file);
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| AttestError::io(format!("fsync {}", dir.display()), e))?;
    fs::write(
        public_key_path(path),
        format!("{}\n", identity.public_key_hex()),
    )
    .map_err(|e| AttestError::io("clé publique", e))?;
    Ok(identity)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    #[cfg(unix)]
    fn key_is_created_once_restricted_and_reloaded_identically() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("cfg").join("secmet").join(KEY_FILE);
        let first = load_or_create(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let pub_text = fs::read_to_string(public_key_path(&path)).unwrap();
        assert_eq!(pub_text.trim(), first.public_key_hex());
        let second = load_or_create(&path).unwrap();
        assert_eq!(first.public_key(), second.public_key());
        assert!(!path.with_extension("hex.tmp").exists());
        fs::write(&path, "zz").unwrap();
        assert!(load_or_create(&path).is_err());
    }

    #[test]
    fn default_path_follows_xdg() {
        let p = default_path();
        assert!(p.is_none_or(|p| p.ends_with(Path::new("secmet").join(KEY_FILE))));
    }
}
