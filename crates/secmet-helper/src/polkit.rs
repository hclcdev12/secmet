//! Matérialisation du prompt Polkit et contrôle du pair.
//!
//! L'UI lance `pkexec secmet-helper serve` : c'est **le** prompt Polkit de
//! la session. `pkexec` exporte `PKEXEC_UID`, l'uid de l'utilisateur qui l'a
//! passé. Le helper génère alors un secret d'amorçage de 32 octets, l'écrit
//! sur sa sortie standard (héritée du lanceur, donc lisible par lui seul) et
//! l'exige dans la commande `Authenticate` de chaque connexion. Une
//! connexion dont le pair (`SO_PEERCRED`) n'est ni root ni `PKEXEC_UID` est
//! fermée avant même de lire un octet.
//!
//! Seul module avec `unsafe` : `getsockopt`, `geteuid`, `umask`.

use std::fs;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;

use zeroize::Zeroizing;

use crate::error::{HelperError, IsolationError, Result};

/// Secret d'amorçage : 32 octets aléatoires, présentés en hexadécimal.
#[derive(Clone)]
pub struct Bootstrap(Zeroizing<[u8; 32]>);

impl Bootstrap {
    /// Tire un secret sur `/dev/urandom`.
    pub fn random() -> Result<Self> {
        Ok(Self(Zeroizing::new(random_bytes()?)))
    }

    /// Depuis des octets connus (tests).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Hexadécimal minuscule à transmettre au lanceur.
    pub fn to_hex(&self) -> String {
        hex(&*self.0)
    }

    /// Compare en temps constant avec l'hexadécimal présenté.
    pub fn matches(&self, presented: &str) -> bool {
        let Some(bytes) = unhex32(presented) else {
            return false;
        };
        constant_time_eq(&*self.0, &bytes)
    }
}

impl std::fmt::Debug for Bootstrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Bootstrap(…)")
    }
}

/// 32 octets de `/dev/urandom`.
pub fn random_bytes() -> Result<[u8; 32]> {
    use std::io::Read;
    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| HelperError::io("/dev/urandom", e))?;
    Ok(bytes)
}

/// Hexadécimal minuscule.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 64 caractères hexadécimaux → 32 octets.
pub fn unhex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Égalité sans court-circuit sur la première différence.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Uid de l'utilisateur ayant passé le prompt (`PKEXEC_UID`), s'il est lisible.
pub fn pkexec_uid() -> Option<u32> {
    std::env::var("PKEXEC_UID").ok()?.trim().parse().ok()
}

/// Uid effectif du processus.
pub fn effective_uid() -> u32 {
    // SAFETY: geteuid ne prend aucun argument, ne touche aucune mémoire et
    // ne peut pas échouer.
    unsafe { libc::geteuid() }
}

/// Fixe l'umask du processus (077 avant toute création de fichier d'état).
pub fn set_umask(mask: u32) {
    // SAFETY: umask ne fait que remplacer le masque du processus appelant.
    unsafe {
        libc::umask(mask as libc::mode_t);
    }
}

/// Identité du pair d'un socket Unix connecté.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    /// PID du pair.
    pub pid: u32,
    /// Uid du pair.
    pub uid: u32,
    /// Gid du pair.
    pub gid: u32,
}

/// `SO_PEERCRED` du pair.
pub fn peer_credentials(stream: &UnixStream) -> io::Result<PeerCredentials> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: le descripteur est celui d'un UnixStream vivant ; `cred` est
    // une structure `ucred` correctement dimensionnée et `len` porte sa taille,
    // comme l'exige getsockopt(SOL_SOCKET, SO_PEERCRED).
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast::<libc::c_void>(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PeerCredentials {
        pid: cred.pid as u32,
        uid: cred.uid,
        gid: cred.gid,
    })
}

/// Le pair est-il autorisé à parler au helper ? Root ou l'uid du prompt.
pub fn peer_allowed(
    peer: PeerCredentials,
    allowed_uid: u32,
) -> std::result::Result<(), IsolationError> {
    if peer.uid == 0 || peer.uid == allowed_uid {
        Ok(())
    } else {
        Err(IsolationError::PeerNotAllowed(peer.uid))
    }
}

/// Inode d'un chemin (sans suivre les liens).
pub fn inode_of_path(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(path).ok().map(|m| m.ino())
}

/// Prépare le répertoire du socket : créé si absent (mode `0711`, traversable
/// non listable), jamais un lien, jamais re-chmodé s'il existe, et retire un
/// socket périmé. Renvoie le chemin du socket.
pub fn prepare_socket_dir(dir: &Path, socket_name: &str) -> Result<std::path::PathBuf> {
    use std::os::unix::fs::MetadataExt;
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(HelperError::Protocol(format!(
                "{} est un lien symbolique : refus",
                dir.display()
            )));
        }
        Ok(meta) if !meta.is_dir() => {
            return Err(HelperError::Protocol(format!(
                "{} n'est pas un répertoire",
                dir.display()
            )));
        }
        // Répertoire existant : jamais modifié (un chmod root sur un
        // répertoire arbitraire serait une arme), seulement vérifié : à nous,
        // non inscriptible par le groupe et les autres.
        Ok(meta) => {
            if meta.uid() != effective_uid() || meta.mode() & 0o022 != 0 {
                return Err(HelperError::Protocol(format!(
                    "{} : propriétaire ou droits inattendus (uid {}, mode {:o})",
                    dir.display(),
                    meta.uid(),
                    meta.mode() & 0o7777
                )));
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(dir).map_err(|e| HelperError::io(dir.display().to_string(), e))?;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o711))
                .map_err(|e| HelperError::io(format!("chmod {}", dir.display()), e))?;
        }
        Err(e) => return Err(HelperError::io(dir.display().to_string(), e)),
    }
    let socket = dir.join(socket_name);
    match fs::symlink_metadata(&socket) {
        Ok(meta) if meta.file_type().is_socket() => {
            fs::remove_file(&socket)
                .map_err(|e| HelperError::io(format!("retrait {}", socket.display()), e))?;
        }
        Ok(_) => {
            return Err(HelperError::Protocol(format!(
                "{} existe et n'est pas un socket",
                socket.display()
            )));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(HelperError::io(socket.display().to_string(), e)),
    }
    Ok(socket)
}

/// Restreint le socket lié : `0600`, propriété de `uid` (l'utilisateur du
/// prompt, seul à pouvoir s'y connecter avec root).
pub fn restrict_socket(socket: &Path, uid: u32) -> Result<()> {
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))
        .map_err(|e| HelperError::io(format!("chmod {}", socket.display()), e))?;
    if effective_uid() == 0 {
        std::os::unix::fs::chown(socket, Some(uid), None)
            .map_err(|e| HelperError::io(format!("chown {}", socket.display()), e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn bootstrap_matches_only_its_own_hex() {
        let b = Bootstrap::from_bytes([7u8; 32]);
        let hex = b.to_hex();
        assert_eq!(hex.len(), 64);
        assert!(b.matches(&hex));
        assert!(!b.matches(&hex.to_uppercase().replace('7', "8")));
        assert!(!b.matches(""));
        assert!(!b.matches(&hex[..63]));
        assert!(!b.matches(&format!("{}0", &hex[..63])));
        assert!(!b.matches("zz".repeat(32).as_str()));
        assert_eq!(format!("{b:?}"), "Bootstrap(…)");
        assert!(Bootstrap::random().unwrap().to_hex() != Bootstrap::random().unwrap().to_hex());
    }

    #[test]
    fn constant_time_eq_and_hex_helpers() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
        assert_eq!(unhex32(&"00".repeat(32)), Some([0u8; 32]));
        assert_eq!(unhex32("00"), None);
        assert_eq!(unhex32(&"é".repeat(32)), None);
    }

    #[test]
    fn peer_policy_accepts_root_and_the_prompt_user_only() {
        let me = PeerCredentials {
            pid: 1,
            uid: 1000,
            gid: 1000,
        };
        assert!(peer_allowed(me, 1000).is_ok());
        assert!(peer_allowed(PeerCredentials { uid: 0, ..me }, 1000).is_ok());
        assert_eq!(
            peer_allowed(PeerCredentials { uid: 1001, ..me }, 1000),
            Err(IsolationError::PeerNotAllowed(1001))
        );
    }

    #[test]
    fn peer_credentials_of_a_socketpair_are_ours() {
        let (a, _b) = UnixStream::pair().unwrap();
        let cred = peer_credentials(&a).unwrap();
        assert_eq!(cred.pid, std::process::id());
        assert_eq!(cred.uid, effective_uid());
    }

    #[test]
    fn socket_dir_is_created_restricted_and_symlinks_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("run");
        let socket = prepare_socket_dir(&dir, "helper.sock").unwrap();
        assert_eq!(socket, dir.join("helper.sock"));
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o711
        );
        // Un fichier ordinaire à la place du socket : refus.
        fs::write(&socket, "x").unwrap();
        assert!(prepare_socket_dir(&dir, "helper.sock").is_err());
        fs::remove_file(&socket).unwrap();
        // Un socket périmé est retiré.
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        drop(listener);
        assert!(socket.exists());
        prepare_socket_dir(&dir, "helper.sock").unwrap();
        assert!(!socket.exists());
        // Lien symbolique : refus.
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert!(prepare_socket_dir(&link, "helper.sock").is_err());
        // Socket restreint à 0600.
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        restrict_socket(&socket, effective_uid()).unwrap();
        assert_eq!(
            fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(listener);
    }
}
