//! Ouverture et géométrie des périphériques bloc.
//!
//! Remplace `blockdev --getsize64 / --getss / --flushbufs / --rereadpt`.
//! Seuls les nœuds de l'allowlist historique de SecMet (`sd*`, `nvme*n*`,
//! `vd*`, `xvd*`) sont ouverts, et seulement s'ils sont réellement des
//! périphériques bloc (jamais un fichier ordinaire ni un lien vers autre chose).

use std::fs::{File, OpenOptions};
use std::os::fd::AsFd;
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::{sys, DeviceError, Result};

/// Nom de nœud accepté : `sda`, `nvme0n1`, `vda`, `xvda`… (sans `/`).
pub fn is_allowed_name(name: &str) -> bool {
    if name.is_empty() || name.contains('/') || name.contains("..") {
        return false;
    }
    if let Some(rest) = name.strip_prefix("nvme") {
        return match rest.split_once('n') {
            Some((ctrl, ns)) => is_digits(ctrl) && is_digits(ns),
            None => false,
        };
    }
    for prefix in ["sd", "vd", "xvd"] {
        if let Some(letters) = name.strip_prefix(prefix) {
            return !letters.is_empty() && letters.bytes().all(|b| b.is_ascii_lowercase());
        }
    }
    false
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// Le disque entier derrière `/dev/<name>`, `/dev/<name>` lui-même ou un nom nu.
/// Refuse tout ce qui n'est pas de la forme `/dev/<nom allowlisté>`.
pub fn device_path(spec: &str) -> Result<PathBuf> {
    let name = spec.strip_prefix("/dev/").unwrap_or(spec);
    if !is_allowed_name(name) {
        return Err(DeviceError::RefusedName(spec.to_string()));
    }
    Ok(Path::new("/dev").join(name))
}

/// Nom court (`sda`) d'un chemin accepté.
pub fn device_name(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| DeviceError::RefusedName(path.display().to_string()))?;
    if !is_allowed_name(name) {
        return Err(DeviceError::RefusedName(path.display().to_string()));
    }
    Ok(name.to_string())
}

/// Est-ce un périphérique bloc (`S_ISBLK`) ? Un lien symbolique est refusé.
pub fn ensure_block_device(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| DeviceError::io(path.display().to_string(), e))?;
    if !metadata.file_type().is_block_device() {
        return Err(DeviceError::NotBlockDevice(path.to_path_buf()));
    }
    Ok(())
}

/// Mode d'ouverture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// Lecture seule, non exclusif : inspection (IDENTIFY, SMART, signatures),
    /// même sur un disque monté.
    ReadOnly,
    /// Lecture-écriture avec `O_EXCL` : le noyau refuse (`EBUSY`) si le disque
    /// ou une partition est monté, swap, ou tenu par dm/md. Pour effacer.
    Exclusive,
    /// Comme `Exclusive`, plus `O_DIRECT` pour les écritures alignées.
    ExclusiveDirect,
}

/// Un périphérique bloc ouvert, avec sa géométrie.
#[derive(Debug)]
pub struct BlockDevice {
    file: File,
    path: PathBuf,
    name: String,
    block_size: u64,
    size_bytes: u64,
}

impl BlockDevice {
    /// Ouvre `spec` (`/dev/sdb`, `nvme0n1`…) dans le mode demandé.
    pub fn open(spec: &str, mode: OpenMode) -> Result<Self> {
        let path = device_path(spec)?;
        let name = device_name(&path)?;
        ensure_block_device(&path)?;
        let mut options = OpenOptions::new();
        options.read(true);
        let flags = match mode {
            OpenMode::ReadOnly => libc::O_NONBLOCK,
            OpenMode::Exclusive => {
                options.write(true);
                libc::O_EXCL
            }
            OpenMode::ExclusiveDirect => {
                options.write(true);
                libc::O_EXCL | libc::O_DIRECT
            }
        };
        let file = options
            // `O_NOFOLLOW` : un lien planté au chemin entre la vérification et
            // l'ouverture n'est pas suivi.
            .custom_flags(flags | libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|e| match e.raw_os_error() {
                Some(libc::EBUSY) => DeviceError::Busy(path.clone()),
                _ => DeviceError::io(format!("ouverture {}", path.display()), e),
            })?;
        let block_size = u64::from(
            sys::logical_block_size(file.as_fd()).map_err(|e| DeviceError::io("BLKSSZGET", e))?,
        );
        let size_bytes =
            sys::block_device_size(file.as_fd()).map_err(|e| DeviceError::io("BLKGETSIZE64", e))?;
        if block_size == 0 || size_bytes % block_size != 0 {
            return Err(DeviceError::Inventory(format!(
                "{} : géométrie incohérente (bloc {block_size}, taille {size_bytes})",
                path.display()
            )));
        }
        Ok(Self {
            file,
            path,
            name,
            block_size,
            size_bytes,
        })
    }

    /// Descripteur ouvert, pour les commandes pass-through.
    pub fn file(&self) -> &File {
        &self.file
    }

    /// Chemin `/dev/...`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Nom court.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Le nœud est-il un namespace NVMe ?
    pub fn is_nvme(&self) -> bool {
        self.name.starts_with("nvme")
    }

    /// Taille de bloc logique.
    pub fn block_size(&self) -> u64 {
        self.block_size
    }

    /// Taille en octets.
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// Nombre de blocs logiques.
    pub fn block_count(&self) -> u64 {
        self.size_bytes / self.block_size
    }

    /// Lecture seule côté noyau ?
    pub fn is_read_only(&self) -> Result<bool> {
        sys::block_device_read_only(self.file.as_fd()).map_err(|e| DeviceError::io("BLKROGET", e))
    }

    /// `fsync` puis `BLKFLSBUF` : ce que faisait `fsync` + `blockdev --flushbufs`.
    pub fn flush(&self) -> Result<()> {
        self.file
            .sync_all()
            .map_err(|e| DeviceError::io("fsync", e))?;
        sys::flush_buffers(self.file.as_fd()).map_err(|e| DeviceError::io("BLKFLSBUF", e))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn allowlist_matches_secmet_helper() {
        for ok in ["sda", "sdab", "nvme0n1", "nvme12n3", "vda", "xvdc"] {
            assert!(is_allowed_name(ok), "{ok}");
        }
        for bad in [
            "",
            "sda1",
            "sd",
            "nvme0",
            "nvme0n1p1",
            "loop0",
            "dm-0",
            "md0",
            "sr0",
            "../sda",
            "sd/a",
            "SDA",
            "mmcblk0",
        ] {
            assert!(!is_allowed_name(bad), "{bad}");
        }
    }

    #[test]
    fn device_path_normalizes_and_refuses() {
        assert_eq!(device_path("/dev/sdb").unwrap(), Path::new("/dev/sdb"));
        assert_eq!(device_path("nvme0n1").unwrap(), Path::new("/dev/nvme0n1"));
        assert!(matches!(
            device_path("/dev/loop0"),
            Err(DeviceError::RefusedName(_))
        ));
        assert!(matches!(
            device_path("/etc/passwd"),
            Err(DeviceError::RefusedName(_))
        ));
        assert!(matches!(
            device_path("/dev/sd/../../etc"),
            Err(DeviceError::RefusedName(_))
        ));
    }

    #[test]
    fn regular_files_and_char_devices_are_not_block_devices() {
        let file = tempfile::NamedTempFile::new().expect("fichier");
        assert!(matches!(
            ensure_block_device(file.path()),
            Err(DeviceError::NotBlockDevice(_))
        ));
        assert!(matches!(
            ensure_block_device(Path::new("/dev/null")),
            Err(DeviceError::NotBlockDevice(_))
        ));
        assert!(matches!(
            ensure_block_device(Path::new("/nonexistent/x")),
            Err(DeviceError::Io { .. })
        ));
    }

    #[test]
    fn open_refuses_anything_outside_dev() {
        assert!(matches!(
            BlockDevice::open("/tmp/sda", OpenMode::ReadOnly),
            Err(DeviceError::RefusedName(_))
        ));
    }
}
