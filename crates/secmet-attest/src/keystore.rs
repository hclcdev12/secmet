//! Clé de signature du poste : un seed Ed25519 de 32 octets en hexadécimal
//! dans un fichier `0600`, avec la clé publique à côté (`.pub`) pour la
//! distribuer aux vérificateurs.
//!
//! Aucune création implicite : [`load`] ne fait que lire, [`create`] crée
//! explicitement. La `.pub` sert de témoin qu'une clé a existé : tant
//! qu'elle est là, [`create`] en mode [`CreateMode::FirstKey`] refuse, pour
//! qu'une clé perdue ne soit jamais remplacée en silence (les clients ont
//! inscrit l'ancien `key_id` dans leur liste de confiance).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::sign::{key_id_of, SigningIdentity};
use crate::{hex, unhex, AttestError, Result};

/// Nom de fichier par défaut.
pub const KEY_FILE: &str = "attestation-key.hex";

/// Historique des clés créées dans un dossier de configuration (JSON, une
/// entrée par création), écrit à côté de la clé par [`record_creation`].
pub const HISTORY_FILE: &str = "key-history.json";

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

/// Chemin de l'historique des clés associé.
pub fn history_path(key_path: &Path) -> PathBuf {
    key_path.with_file_name(HISTORY_FILE)
}

/// État de la clé de signature d'un dossier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyState {
    /// Aucune clé n'a jamais existé ici (ni clé privée, ni `.pub`, ni
    /// historique) : la toute première clé peut être créée.
    Missing,
    /// La clé privée a disparu alors qu'une clé a existé (`.pub` ou
    /// historique présents). Clé publique de l'ancienne clé si la `.pub`
    /// est lisible.
    Lost {
        /// Clé publique de l'ancienne clé (64 hex), si connue.
        previous_public_key: Option<String>,
    },
    /// Clé privée en clair présente (`attestation-key.hex`).
    Plain,
}

impl KeyState {
    /// `key_id` de l'ancienne clé, pour [`KeyState::Lost`].
    pub fn previous_key_id(&self) -> Option<String> {
        match self {
            Self::Lost {
                previous_public_key: Some(pk),
            } => key_id_of_hex(pk),
            _ => None,
        }
    }
}

/// `key_id` d'une clé publique en hexadécimal (64 caractères).
pub fn key_id_of_hex(public_key_hex: &str) -> Option<String> {
    let bytes = unhex(public_key_hex.trim()).filter(|b| b.len() == 32)?;
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&bytes);
    Some(key_id_of(&pk))
}

fn read_public_key_file(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let text = text.trim();
            if key_id_of_hex(text).is_some() {
                Ok(Some(text.to_ascii_lowercase()))
            } else {
                Err(AttestError::Invalid(format!(
                    "{} : clé publique illisible",
                    path.display()
                )))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AttestError::io(path.display().to_string(), e)),
    }
}

/// État de la clé `path` (ne lit pas le seed).
pub fn state(path: &Path) -> Result<KeyState> {
    match fs::symlink_metadata(path) {
        Ok(_) => return Ok(KeyState::Plain),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(AttestError::io(path.display().to_string(), e)),
    }
    let pub_path = public_key_path(path);
    let pub_exists = pub_path.exists();
    if !pub_exists && !history_path(path).exists() {
        return Ok(KeyState::Missing);
    }
    let previous_public_key = if pub_exists {
        read_public_key_file(&pub_path).ok().flatten()
    } else {
        None
    };
    Ok(KeyState::Lost {
        previous_public_key,
    })
}

/// Clé publique (64 hex) de la clé `path`, lue dans la `.pub` sans charger
/// la clé privée ; à défaut de `.pub`, dérivée de la clé privée.
pub fn public_key_hex(path: &Path) -> Result<String> {
    if let Some(pk) = read_public_key_file(&public_key_path(path))? {
        return Ok(pk);
    }
    load(path).map(|identity| identity.public_key_hex())
}

/// Charge la clé de `path`, sans jamais la créer (erreur si absente). Les
/// droits doivent être `0600`. Écrit la `.pub` si elle manque.
pub fn load(path: &Path) -> Result<SigningIdentity> {
    let text = Zeroizing::new(
        fs::read_to_string(path).map_err(|e| AttestError::io(path.display().to_string(), e))?,
    );
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
    let bytes = Zeroizing::new(
        unhex(text.trim())
            .filter(|b| b.len() == 32)
            .ok_or_else(|| AttestError::Signature(format!("{}: seed illisible", path.display())))?,
    );
    let mut seed = Zeroizing::new([0u8; 32]);
    seed.copy_from_slice(&bytes);
    let identity = SigningIdentity::from_seed(&seed);
    let pub_path = public_key_path(path);
    if !pub_path.exists() {
        let _ = fs::write(&pub_path, format!("{}\n", identity.public_key_hex()));
    }
    Ok(identity)
}

/// Mode de [`create`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateMode {
    /// Toute première clé : refus si une clé a déjà existé
    /// ([`KeyState::Lost`]) ou existe ([`KeyState::Plain`]).
    FirstKey,
    /// Remplacement d'une clé perdue, sur demande explicite (opérateur,
    /// `secmet-release keygen`) : refus seulement si la clé privée existe ;
    /// la `.pub` de l'ancienne clé est remplacée.
    Replace,
}

fn random_seed() -> Result<Zeroizing<[u8; 32]>> {
    let mut seed = Zeroizing::new([0u8; 32]);
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(seed.as_mut()))
        .map_err(|e| AttestError::io("/dev/urandom", e))?;
    Ok(seed)
}

/// Crée la clé `path` (répertoire `0700`, fichier `0600`, création
/// exclusive, `fsync`) et écrit la clé publique à côté. Jamais d'écrasement
/// d'une clé privée existante ; en mode [`CreateMode::FirstKey`], refus si
/// une clé a déjà existé.
pub fn create(path: &Path, mode: CreateMode) -> Result<SigningIdentity> {
    match (state(path)?, mode) {
        (KeyState::Plain, _) => {
            return Err(AttestError::Invalid(format!(
                "{} existe déjà : rien n'est écrasé",
                path.display()
            )))
        }
        (KeyState::Lost { .. }, CreateMode::FirstKey) => {
            return Err(AttestError::Invalid(format!(
                "{} : une clé de signature a déjà existé ici ({} présent) ; \
                 une nouvelle clé ne se crée que sur demande explicite",
                path.display(),
                public_key_path(path).display()
            )))
        }
        _ => {}
    }
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
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return load(path),
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

/// Entrée de l'historique des clés.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    /// Horodatage de la création (texte libre, fourni par l'appelant).
    pub created_at: String,
    /// `key_id` de la nouvelle clé.
    pub key_id: String,
    /// Clé publique de la nouvelle clé (64 hex).
    pub public_key: String,
    /// `key_id` de la clé perdue qu'elle remplace, s'il est connu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
}

/// Ajoute une entrée à l'historique des clés (`key-history.json` à côté de
/// la clé). Un historique illisible est conservé sous `.bad` et repart vide.
pub fn record_creation(path: &Path, entry: HistoryEntry) -> Result<()> {
    let history = history_path(path);
    let mut entries: Vec<HistoryEntry> = match fs::read_to_string(&history) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(entries) => entries,
            Err(_) => {
                let _ = fs::rename(&history, history.with_extension("json.bad"));
                Vec::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(AttestError::io(history.display().to_string(), e)),
    };
    entries.push(entry);
    let text = serde_json::to_string_pretty(&entries)?;
    fs::write(&history, format!("{text}\n"))
        .map_err(|e| AttestError::io(history.display().to_string(), e))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn key_path(tmp: &tempfile::TempDir) -> PathBuf {
        tmp.path().join("cfg").join("secmet").join(KEY_FILE)
    }

    #[test]
    #[cfg(unix)]
    fn key_is_created_once_restricted_and_reloaded_identically() {
        let tmp = tempfile::tempdir().unwrap();
        let path = key_path(&tmp);
        let first = create(&path, CreateMode::FirstKey).unwrap();
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
        let second = load(&path).unwrap();
        assert_eq!(first.public_key(), second.public_key());
        assert!(!path.with_extension("hex.tmp").exists());
        // Jamais d'écrasement, quel que soit le mode.
        assert!(create(&path, CreateMode::FirstKey).is_err());
        assert!(create(&path, CreateMode::Replace).is_err());
        assert_eq!(load(&path).unwrap().public_key(), first.public_key());
        fs::write(&path, "zz").unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn too_open_permissions_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = key_path(&tmp);
        create(&path, CreateMode::FirstKey).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn load_never_creates() {
        let tmp = tempfile::tempdir().unwrap();
        let path = key_path(&tmp);
        assert!(load(&path).is_err());
        assert!(!path.exists());
        assert!(!public_key_path(&path).exists());
        assert_eq!(state(&path).unwrap(), KeyState::Missing);
    }

    #[test]
    fn state_reports_each_case() {
        let tmp = tempfile::tempdir().unwrap();
        let path = key_path(&tmp);
        assert_eq!(state(&path).unwrap(), KeyState::Missing);

        let identity = create(&path, CreateMode::FirstKey).unwrap();
        assert_eq!(state(&path).unwrap(), KeyState::Plain);

        // Clé privée disparue, `.pub` restée : clé perdue, ancien key_id connu.
        fs::remove_file(&path).unwrap();
        let lost = state(&path).unwrap();
        assert_eq!(
            lost,
            KeyState::Lost {
                previous_public_key: Some(identity.public_key_hex())
            }
        );
        assert_eq!(lost.previous_key_id(), Some(identity.key_id()));

        // `.pub` illisible : toujours perdue, ancien key_id inconnu.
        fs::write(public_key_path(&path), "pas une clé").unwrap();
        let lost = state(&path).unwrap();
        assert_eq!(
            lost,
            KeyState::Lost {
                previous_public_key: None
            }
        );
        assert_eq!(lost.previous_key_id(), None);

        // Historique seul : une clé a existé.
        fs::remove_file(public_key_path(&path)).unwrap();
        assert_eq!(state(&path).unwrap(), KeyState::Missing);
        record_creation(
            &path,
            HistoryEntry {
                created_at: "2026-11-20T10:00:00Z".into(),
                key_id: identity.key_id(),
                public_key: identity.public_key_hex(),
                replaces: None,
            },
        )
        .unwrap();
        assert!(matches!(state(&path).unwrap(), KeyState::Lost { .. }));
    }

    #[test]
    fn a_lost_key_is_only_replaced_on_explicit_request() {
        let tmp = tempfile::tempdir().unwrap();
        let path = key_path(&tmp);
        let old = create(&path, CreateMode::FirstKey).unwrap();
        fs::remove_file(&path).unwrap();

        let refused = create(&path, CreateMode::FirstKey).unwrap_err();
        assert!(refused.to_string().contains("déjà existé"), "{refused}");
        assert!(!path.exists());
        // La `.pub` de l'ancienne clé n'est pas touchée par le refus.
        assert_eq!(
            fs::read_to_string(public_key_path(&path)).unwrap().trim(),
            old.public_key_hex()
        );

        let new = create(&path, CreateMode::Replace).unwrap();
        assert_ne!(new.public_key(), old.public_key());
        assert_eq!(state(&path).unwrap(), KeyState::Plain);
        assert_eq!(public_key_hex(&path).unwrap(), new.public_key_hex());
    }

    #[test]
    fn public_key_is_read_without_the_private_key() {
        let tmp = tempfile::tempdir().unwrap();
        let path = key_path(&tmp);
        assert!(public_key_hex(&path).is_err());
        let identity = create(&path, CreateMode::FirstKey).unwrap();
        // Lecture de la `.pub` même si la clé privée est illisible.
        fs::write(&path, "zz").unwrap();
        assert_eq!(public_key_hex(&path).unwrap(), identity.public_key_hex());
        // Sans `.pub`, repli sur la clé privée, qui réécrit la `.pub`.
        fs::remove_file(&path).unwrap();
        fs::remove_file(public_key_path(&path)).unwrap();
        let identity = create(&path, CreateMode::FirstKey).unwrap();
        fs::remove_file(public_key_path(&path)).unwrap();
        assert_eq!(public_key_hex(&path).unwrap(), identity.public_key_hex());
        assert!(public_key_path(&path).exists());
        // `.pub` corrompue : erreur, pas de valeur inventée.
        fs::write(public_key_path(&path), "xyz").unwrap();
        assert!(public_key_hex(&path).is_err());
    }

    #[test]
    fn history_accumulates_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let path = key_path(&tmp);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for (i, replaces) in [None, Some("aaaa".to_string())].into_iter().enumerate() {
            record_creation(
                &path,
                HistoryEntry {
                    created_at: format!("t{i}"),
                    key_id: format!("k{i}"),
                    public_key: "00".repeat(32),
                    replaces,
                },
            )
            .unwrap();
        }
        let text = fs::read_to_string(history_path(&path)).unwrap();
        let entries: Vec<HistoryEntry> = serde_json::from_str(&text).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].replaces.as_deref(), Some("aaaa"));
        assert!(!text.contains("\"replaces\": null"));
    }

    #[test]
    fn default_path_follows_xdg() {
        let p = default_path();
        assert!(p.is_none_or(|p| p.ends_with(Path::new("secmet").join(KEY_FILE))));
    }
}
