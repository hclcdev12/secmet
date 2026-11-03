//! Ouverture sécurisée d'une cible d'effacement (anti-TOCTOU).
//!
//! Ordre des vérifications dans [`open_for_erase`] :
//! 1. le nœud `/dev/<name>` de la session est un périphérique bloc dont le
//!    numéro majeur:mineur est celui du jeton ;
//! 2. ouverture `O_RDWR | O_EXCL | O_DIRECT` : le noyau refuse si une
//!    partition est montée, en swap ou tenue par dm/md ;
//! 3. `fstat` sur le descripteur obtenu : même numéro de périphérique (le
//!    nœud n'a pas été remplacé entre le stat et l'open) ;
//! 4. ré-identification firmware derrière ce descripteur : S/N et empreinte
//!    identiques au jeton ; protections fraîches (`mountinfo`, `swaps`,
//!    `holders/`) toutes à faux ;
//! 5. `/proc/*/fd` : aucun autre processus ne tient le disque, une de ses
//!    partitions, ni (pour une commande portant sur tout le contrôleur NVMe)
//!    un namespace frère.
//!
//! Chaque refus est une [`IsolationError`] explicite : `Busy`,
//! `IdentityMismatch`, `OpenersFound`.

use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use secmet_device::block::{device_path, BlockDevice, OpenMode};
use secmet_device::protection::DevNum;
use secmet_device::sysfs::{self, DiskEntry, SysfsPaths};
use secmet_device::DeviceError;

use crate::error::{HelperError, IsolationError, Opener, Result};
use crate::identity::{self, Scan};
use crate::session::DiskIdentity;

/// Une cible ouverte exclusivement et ré-identifiée.
#[derive(Debug)]
pub struct Isolated {
    /// Descripteur `O_EXCL | O_DIRECT`.
    pub device: BlockDevice,
    /// Inspection faite derrière ce descripteur.
    pub scan: Scan,
    /// Numéros de périphérique surveillés (disque, partitions, frères).
    pub guarded: Vec<DevNum>,
}

/// Numéros de périphérique des partitions d'un disque.
pub fn partition_devnums(paths: &SysfsPaths, entry: &DiskEntry) -> Vec<DevNum> {
    let class = paths.sys.join("class/block");
    entry
        .partitions
        .iter()
        .filter_map(|part| sysfs::read_optional(&class.join(part).join("dev")))
        .filter_map(|text| DevNum::parse(&text))
        .collect()
}

/// Namespaces frères d'un namespace NVMe (`nvme0n2` pour `nvme0n1`).
pub fn sibling_namespaces(paths: &SysfsPaths, name: &str) -> Result<Vec<DiskEntry>> {
    let Some(controller) = name
        .strip_prefix("nvme")
        .and_then(|rest| rest.split_once('n'))
        .map(|(ctrl, _)| format!("nvme{ctrl}n"))
    else {
        return Ok(Vec::new());
    };
    Ok(sysfs::list_disks(paths)?
        .into_iter()
        .filter(|d| d.name != name && d.name.starts_with(&controller))
        .collect())
}

/// Parcourt `/proc/<pid>/fd/*` et retient les descripteurs pour lesquels
/// `is_target` répond vrai (le chemin passé est celui du lien, à suivre ou
/// à lire). Le processus `self_pid` est ignoré. Un répertoire illisible
/// (processus d'un autre utilisateur, hors root) est ignoré.
pub fn scan_fds(
    proc_root: &Path,
    self_pid: u32,
    mut is_target: impl FnMut(&Path) -> bool,
) -> Vec<Opener> {
    let mut openers = Vec::new();
    let Ok(entries) = fs::read_dir(proc_root) else {
        return openers;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        let fd_dir = entry.path().join("fd");
        let Ok(fds) = fs::read_dir(&fd_dir) else {
            continue;
        };
        let comm = fs::read_to_string(entry.path().join("comm"))
            .map(|c| c.trim().to_string())
            .unwrap_or_default();
        for fd in fds.flatten() {
            let Some(number) = fd.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            let link = fd.path();
            if is_target(&link) {
                openers.push(Opener {
                    pid,
                    comm: comm.clone(),
                    fd: number,
                    target: fs::read_link(&link)
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                });
            }
        }
    }
    openers.sort_by_key(|o| (o.pid, o.fd));
    openers
}

/// Processus tiers tenant un descripteur sur l'un des `targets`.
pub fn find_openers(proc_root: &Path, self_pid: u32, targets: &[DevNum]) -> Vec<Opener> {
    if targets.is_empty() {
        return Vec::new();
    }
    scan_fds(proc_root, self_pid, |link| {
        fs::metadata(link).is_ok_and(|meta| {
            meta.file_type().is_block_device() && targets.contains(&DevNum::from_rdev(meta.rdev()))
        })
    })
}

/// Vérifie qu'aucun autre processus ne tient `targets`.
pub fn ensure_no_openers(proc_root: &Path, targets: &[DevNum]) -> Result<()> {
    let openers = find_openers(proc_root, std::process::id(), targets);
    if openers.is_empty() {
        Ok(())
    } else {
        Err(IsolationError::OpenersFound(openers).into())
    }
}

fn devnum_of(identity: &DiskIdentity) -> DevNum {
    DevNum {
        major: identity.major,
        minor: identity.minor,
    }
}

/// Ouvre le disque de `expected` pour l'effacer, après toutes les
/// vérifications du module. `whole_controller` étend la garde aux
/// namespaces frères d'un contrôleur NVMe.
pub fn open_for_erase(
    paths: &SysfsPaths,
    expected: &DiskIdentity,
    whole_controller: bool,
) -> Result<Isolated> {
    // 1. Le nœud porte toujours le numéro de périphérique de la session.
    let path: PathBuf = device_path(&expected.name)?;
    let on_path = identity::devnum_of_path(&path)
        .map_err(|e| HelperError::io(format!("stat {}", path.display()), e))?;
    let wanted = devnum_of(expected);
    if on_path != wanted {
        return Err(IsolationError::IdentityMismatch {
            field: "devnum",
            expected: wanted.to_string(),
            found: on_path.to_string(),
        }
        .into());
    }

    // 2. Ouverture exclusive directe.
    let device = match BlockDevice::open(&expected.name, OpenMode::ExclusiveDirect) {
        Ok(device) => device,
        Err(DeviceError::Busy(p)) => {
            return Err(IsolationError::Busy(format!(
                "{} : O_EXCL refusé (monté, swap, ou tenu par dm/md)",
                p.display()
            ))
            .into())
        }
        Err(e) => return Err(e.into()),
    };

    // 3. Le descripteur ouvert est bien ce périphérique.
    let on_fd = identity::devnum_of(device.file())
        .map_err(|e| HelperError::io(format!("fstat {}", path.display()), e))?;
    if on_fd != wanted {
        return Err(IsolationError::IdentityMismatch {
            field: "devnum",
            expected: wanted.to_string(),
            found: on_fd.to_string(),
        }
        .into());
    }

    // 4. Ré-identification et protections derrière ce descripteur.
    let scan = identity::inspect(paths, &device)?;
    expected.check_against(&scan.identity)?;
    if scan.caps.protection.any() {
        return Err(IsolationError::Busy(describe_protection(&scan.caps.protection)).into());
    }

    // 5. Aucun autre détenteur.
    let mut guarded = vec![wanted];
    guarded.extend(partition_devnums(paths, &scan.entry));
    if whole_controller {
        for sibling in sibling_namespaces(paths, &expected.name)? {
            let devnum = DevNum {
                major: sibling.major,
                minor: sibling.minor,
            };
            let protection = identity::fresh_protection(paths, devnum)?;
            if protection.any() {
                return Err(IsolationError::Busy(format!(
                    "namespace frère {} : {}",
                    sibling.name,
                    describe_protection(&protection)
                ))
                .into());
            }
            guarded.push(devnum);
            guarded.extend(partition_devnums(paths, &sibling));
        }
    }
    ensure_no_openers(&paths.proc, &guarded)?;

    Ok(Isolated {
        device,
        scan,
        guarded,
    })
}

fn describe_protection(p: &secmet_core::taxonomy::ProtectionFlags) -> String {
    let mut parts = Vec::new();
    if p.hosts_system {
        parts.push("disque système");
    }
    if p.mounted {
        parts.push("monté");
    }
    if p.swap {
        parts.push("swap actif");
    }
    if p.held {
        parts.push("membre LVM/RAID/dm");
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::os::unix::fs::symlink;

    fn fake_proc(root: &Path, pid: u32, comm: &str, fds: &[(u32, &str)]) {
        let dir = root.join(pid.to_string());
        fs::create_dir_all(dir.join("fd")).unwrap();
        fs::write(dir.join("comm"), format!("{comm}\n")).unwrap();
        for (fd, target) in fds {
            symlink(target, dir.join("fd").join(fd.to_string())).unwrap();
        }
    }

    #[test]
    fn scan_fds_walks_processes_reads_comm_and_skips_self() {
        let tmp = tempfile::tempdir().unwrap();
        let proc_root = tmp.path();
        fake_proc(proc_root, 100, "dd", &[(0, "/dev/pts/1"), (3, "/dev/sdz")]);
        fake_proc(proc_root, 200, "secmet-helper", &[(5, "/dev/sdz")]);
        fake_proc(proc_root, 300, "bash", &[(1, "/dev/pts/2")]);
        fake_proc(proc_root, 400, "fdisk", &[(4, "/dev/sdz1")]);
        fs::create_dir_all(proc_root.join("self")).unwrap();
        fs::create_dir_all(proc_root.join("500")).unwrap(); // sans fd/ : ignoré
        let found = scan_fds(proc_root, 200, |link| {
            fs::read_link(link).is_ok_and(|t| t.to_string_lossy().starts_with("/dev/sdz"))
        });
        assert_eq!(found.len(), 2);
        assert_eq!(
            (found[0].pid, found[0].fd, found[0].comm.as_str()),
            (100, 3, "dd")
        );
        assert_eq!(found[0].target, "/dev/sdz");
        assert_eq!((found[1].pid, found[1].comm.as_str()), (400, "fdisk"));
        assert!(scan_fds(proc_root, 0, |_| false).is_empty());
        assert!(scan_fds(Path::new("/nonexistent"), 0, |_| true).is_empty());
    }

    #[test]
    fn find_openers_on_the_real_proc_finds_nothing_for_a_fake_devnum() {
        let nowhere = DevNum {
            major: 254,
            minor: 4095,
        };
        assert!(find_openers(Path::new("/proc"), std::process::id(), &[nowhere]).is_empty());
        assert!(find_openers(Path::new("/proc"), std::process::id(), &[]).is_empty());
        assert!(ensure_no_openers(Path::new("/proc"), &[nowhere]).is_ok());
    }

    #[test]
    fn sibling_namespaces_only_apply_to_nvme_names() {
        let paths = SysfsPaths::default();
        assert!(sibling_namespaces(&paths, "sdb").unwrap().is_empty());
    }

    #[test]
    fn open_for_erase_refuses_a_node_whose_devnum_changed() {
        // /dev/sdzz n'existe pas : l'erreur arrive avant toute ouverture.
        let paths = SysfsPaths::default();
        let expected = DiskIdentity {
            name: "sdzz".into(),
            major: 8,
            minor: 0,
            serial: "S".into(),
            fingerprint: String::new(),
        };
        assert!(matches!(
            open_for_erase(&paths, &expected, false),
            Err(HelperError::Io { .. })
        ));
        let refused = DiskIdentity {
            name: "loop0".into(),
            ..expected
        };
        assert!(matches!(
            open_for_erase(&paths, &refused, false),
            Err(HelperError::Device(DeviceError::RefusedName(_)))
        ));
    }

    #[test]
    fn protection_description_lists_every_flag() {
        let all = secmet_core::taxonomy::ProtectionFlags {
            hosts_system: true,
            mounted: true,
            swap: true,
            held: true,
        };
        let text = describe_protection(&all);
        for word in ["système", "monté", "swap", "LVM"] {
            assert!(text.contains(word), "{text}");
        }
    }
}
