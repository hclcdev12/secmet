//! Linux : disques par `lsblk --json`, écriture directe sur `/dev/sdX`
//! ouvert en exclusif (`O_EXCL` : refusé tant qu'une partition est
//! montée), relance privilégiée par `pkexec`.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::disks::Disk;

// <linux/fs.h>, x86_64 et toutes architectures : _IO(0x12, n) sans taille.
const BLKRRPART: libc::c_ulong = 0x125f;
const BLKFLSBUF: libc::c_ulong = 0x1261;
// _IOR(0x12, 114, size_t) : taille 8 sur les architectures 64 bits.
#[cfg(target_pointer_width = "64")]
const BLKGETSIZE64: libc::c_ulong = 0x8008_1272;
#[cfg(target_pointer_width = "32")]
const BLKGETSIZE64: libc::c_ulong = 0x8004_1272;

#[derive(serde::Deserialize)]
struct Lsblk {
    blockdevices: Vec<Dev>,
}

#[derive(serde::Deserialize)]
struct Dev {
    path: Option<String>,
    size: Option<u64>,
    model: Option<String>,
    serial: Option<String>,
    vendor: Option<String>,
    tran: Option<String>,
    #[serde(default)]
    rm: bool,
    #[serde(default)]
    hotplug: bool,
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    mountpoints: Vec<Option<String>>,
    #[serde(default)]
    children: Vec<Dev>,
}

fn collect_mounts(dev: &Dev, out: &mut Vec<String>) {
    out.extend(dev.mountpoints.iter().flatten().cloned());
    for c in &dev.children {
        collect_mounts(c, out);
    }
}

/// Disques entiers vus par `lsblk`, avec les points de montage de leurs
/// partitions. Le filtrage (amovible, taille) est fait par [`Disk`].
pub fn list_disks() -> Result<Vec<Disk>, String> {
    let out = Command::new("lsblk")
        .args([
            "--json",
            "--bytes",
            "-o",
            "PATH,SIZE,MODEL,SERIAL,VENDOR,TRAN,RM,HOTPLUG,TYPE,MOUNTPOINTS",
        ])
        .output()
        .map_err(|e| format!("lsblk introuvable ({e}) : paquet util-linux requis"))?;
    if !out.status.success() {
        return Err(format!(
            "lsblk a échoué : {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let parsed: Lsblk = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("sortie de lsblk illisible : {e}"))?;
    let mut disks = Vec::new();
    for d in parsed.blockdevices {
        if d.kind.as_deref() != Some("disk") {
            continue;
        }
        let Some(path) = d.path.clone() else { continue };
        let mut mounts = Vec::new();
        collect_mounts(&d, &mut mounts);
        let clean = |s: &Option<String>| {
            s.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let vendor = clean(&d.vendor).filter(|v| v != "ATA");
        let model = clean(&d.model).unwrap_or_else(|| "Disque".to_string());
        let name = match vendor {
            Some(v) if !model.to_lowercase().starts_with(&v.to_lowercase()) => {
                format!("{v} {model}")
            }
            _ => model,
        };
        disks.push(Disk {
            path,
            name,
            serial: clean(&d.serial),
            size: d.size.unwrap_or(0),
            removable: d.rm || d.hotplug || d.tran.as_deref() == Some("usb"),
            transport: clean(&d.tran),
            mounted: mounts,
        });
    }
    Ok(disks)
}

/// Démonte toutes les partitions du disque (lecture de `/proc/mounts`).
pub fn unmount_partitions(device: &str) -> io::Result<Vec<String>> {
    let mounts = std::fs::read_to_string("/proc/mounts")?;
    let mut done = Vec::new();
    // Les plus profonds d'abord (montages imbriqués).
    let mut targets: Vec<(String, String)> = mounts
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let src = it.next()?;
            let dst = it.next()?;
            (src == device || src.starts_with(&format!("{device}p")) || {
                src.starts_with(device)
                    && src[device.len()..].chars().all(|c| c.is_ascii_digit())
                    && src.len() > device.len()
            })
            .then(|| (src.to_string(), dst.to_string()))
        })
        .collect();
    targets.sort_by_key(|a| std::cmp::Reverse(a.1.len()));
    for (src, dst) in targets {
        let path = unescape_mount(&dst);
        let c = CString::new(path.clone())
            .map_err(|_| io::Error::other("point de montage invalide"))?;
        // SAFETY: appel système sur une chaîne C valide ; aucun pointeur conservé.
        let rc = unsafe { libc::umount(c.as_ptr()) };
        if rc != 0 {
            let e = io::Error::last_os_error();
            return Err(io::Error::new(
                e.kind(),
                format!("démontage de {src} ({path}) impossible : {e}"),
            ));
        }
        done.push(path);
    }
    Ok(done)
}

fn unescape_mount(s: &str) -> String {
    // /proc/mounts encode espace, tabulation, \n, \\ en octal.
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let code: String = chars.by_ref().take(3).collect();
            if let Ok(v) = u8::from_str_radix(&code, 8) {
                out.push(v as char);
                continue;
            }
            out.push_str(&code);
        } else {
            out.push(c);
        }
    }
    out
}

/// Ouvre le disque en écriture exclusive (`EBUSY` si une partition est
/// encore montée ou utilisée) et directe (`O_DIRECT`) : chaque écriture
/// attend la clé, la progression est réelle, l'annulation reste possible
/// et le cache du noyau n'absorbe pas 11 Gio pour les rendre dans un
/// `fsync` final de plusieurs minutes. Tampons et tailles alignés sur
/// [`crate::worker::ALIGN`].
pub fn open_write(device: &str) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_EXCL | libc::O_CLOEXEC | libc::O_DIRECT)
        .open(device)
}

/// Ouvre le disque en lecture pour la relecture de contrôle.
pub fn open_read(device: &str) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(device)
}

/// Taille du disque en octets (`BLKGETSIZE64`).
pub fn device_size(f: &File) -> io::Result<u64> {
    let mut size: u64 = 0;
    // SAFETY: ioctl BLKGETSIZE64 écrit un u64 à l'adresse fournie, valide.
    let rc = unsafe { libc::ioctl(f.as_raw_fd(), BLKGETSIZE64, &mut size as *mut u64) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(size)
}

/// Vide et invalide le cache des blocs du disque : la relecture qui suit
/// vient de la clé, pas de la mémoire.
pub fn drop_caches(f: &File) -> io::Result<()> {
    // SAFETY: ioctl sans argument sur un descripteur valide.
    let rc = unsafe { libc::ioctl(f.as_raw_fd(), BLKFLSBUF, 0) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Demande au noyau de relire la table de partitions.
pub fn reread_partitions(f: &File) -> io::Result<()> {
    // SAFETY: ioctl sans argument sur un descripteur valide.
    let rc = unsafe { libc::ioctl(f.as_raw_fd(), BLKRRPART, 0) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Vrai si le processus est déjà root.
pub fn is_privileged() -> bool {
    // SAFETY: lecture d'un identifiant, sans effet de bord.
    unsafe { libc::geteuid() == 0 }
}

/// Commande qui lance ce programme en `--worker` avec les droits
/// nécessaires. Depuis une AppImage, root ne peut pas lire le montage FUSE
/// de l'utilisateur : l'exécutable est d'abord copié dans un dossier
/// lisible ; la copie est supprimée par l'appelant après usage.
pub fn elevated_worker(device: &str, image: &Path) -> io::Result<(Command, Option<PathBuf>)> {
    let exe = std::env::current_exe()?;
    let (program, temp) = if std::env::var_os("APPIMAGE").is_some() {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let copy = dir.join(format!("secmet-key-writer-{}", std::process::id()));
        std::fs::copy(&exe, &copy)?;
        std::fs::set_permissions(&copy, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
        (copy.clone(), Some(copy))
    } else {
        (exe, None)
    };
    let mut cmd = if is_privileged() {
        Command::new(&program)
    } else {
        let mut c = Command::new("pkexec");
        c.arg(&program);
        c
    };
    cmd.arg("--worker").arg(device).arg(image);
    Ok((cmd, temp))
}

/// Message quand `pkexec` refuse ou manque.
pub fn elevation_hint(code: Option<i32>) -> String {
    match code {
        Some(126) => "autorisation refusée (dialogue annulé)".to_string(),
        Some(127) => "pkexec n'a pas pu lancer le programme (PolicyKit absent ?)".to_string(),
        Some(c) => format!("le processus d'écriture s'est arrêté (code {c})"),
        None => "le processus d'écriture a été tué".to_string(),
    }
}
