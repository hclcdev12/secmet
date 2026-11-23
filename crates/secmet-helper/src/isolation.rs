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
//!
//!    Puis verrou `flock(LOCK_EX)` sur le disque entier, gardé jusqu'à la
//!    fermeture du descripteur : udev ne relit pas un disque verrouillé
//!    (« Locking Block Device Access » de systemd), donc aucune sonde ne
//!    s'ouvre pendant l'effacement ;
//! 5. `/proc/*/fd` : aucun autre processus ne tient le disque, une de ses
//!    partitions, ni (pour une commande portant sur tout le contrôleur NVMe)
//!    un namespace frère. Les sondes de udev et udisks, ouvertes quelques
//!    millisecondes après un événement (fermeture d'un descripteur en
//!    écriture, par exemple les canaris), sont attendues au plus
//!    [`SETTLE_TIMEOUT`] ; tout autre détenteur est refusé aussitôt.
//!
//! [`widen`] étend la garde aux namespaces frères sans refermer le
//! descripteur : une fermeture relancerait la sonde de udev (constaté le
//! 2026-11-20 sur NVMe Sanitize, Lenovo Yoga Pro 7 : `(udev-worker)` tenait
//! le disque à la réouverture).
//!
//! Chaque refus est une [`IsolationError`] explicite : `Busy`,
//! `IdentityMismatch`, `OpenersFound`.

use std::fs::{self, File};
use std::os::fd::AsFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

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

/// Attente maximale des sondes système (udev, udisks) et du verrou.
pub const SETTLE_TIMEOUT: Duration = Duration::from_secs(10);
const SETTLE_STEP: Duration = Duration::from_millis(100);

/// Processus système qui relisent un disque juste après un événement udev
/// et le relâchent d'eux-mêmes (`comm` tel que lu dans `/proc`).
const TRANSIENT_PROBERS: &[&str] = &["(udev-worker)", "systemd-udevd", "udisksd"];

fn only_transient_probers(openers: &[Opener]) -> bool {
    openers
        .iter()
        .all(|o| TRANSIENT_PROBERS.contains(&o.comm.as_str()))
}

/// Interroge `find` jusqu'à ce qu'il ne trouve plus personne. Tant que les
/// seuls détenteurs sont des sondes système, attend (au plus `timeout`) ;
/// tout autre détenteur, ou l'échéance, donne `OpenersFound`.
fn wait_until_released(
    mut find: impl FnMut() -> Vec<Opener>,
    timeout: Duration,
    step: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let openers = find();
        if openers.is_empty() {
            return Ok(());
        }
        if !only_transient_probers(&openers) || Instant::now() >= deadline {
            return Err(IsolationError::OpenersFound(openers).into());
        }
        thread::sleep(step);
    }
}

/// Vérifie qu'aucun autre processus ne tient `targets` (sondes udev et
/// udisks attendues au plus [`SETTLE_TIMEOUT`]).
pub fn ensure_no_openers(proc_root: &Path, targets: &[DevNum]) -> Result<()> {
    let self_pid = std::process::id();
    wait_until_released(
        || find_openers(proc_root, self_pid, targets),
        SETTLE_TIMEOUT,
        SETTLE_STEP,
    )
}

/// Verrou exclusif `flock` sur le disque entier (attendu au plus
/// `timeout` : udev le tient en partagé le temps d'une sonde).
fn lock_whole_disk(file: &File, path: &Path, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let locked = secmet_device::sys::try_lock_exclusive(file.as_fd())
            .map_err(|e| HelperError::io(format!("flock {}", path.display()), e))?;
        if locked {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(IsolationError::Busy(format!(
                "{} : verrou exclusif (flock) tenu par un autre processus depuis {} s",
                path.display(),
                timeout.as_secs()
            ))
            .into());
        }
        thread::sleep(SETTLE_STEP);
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

    // 4 bis. Verrou exclusif : plus aucune sonde udev jusqu'à la fermeture.
    lock_whole_disk(device.file(), &path, SETTLE_TIMEOUT)?;

    // 5. Aucun autre détenteur.
    let guarded = guard(paths, expected, &scan, whole_controller)?;

    Ok(Isolated {
        device,
        scan,
        guarded,
    })
}

/// Étend la garde d'une cible déjà ouverte aux namespaces frères (commande
/// sur tout le contrôleur NVMe), sans refermer son descripteur.
pub fn widen(paths: &SysfsPaths, expected: &DiskIdentity, isolated: Isolated) -> Result<Isolated> {
    let guarded = guard(paths, expected, &isolated.scan, true)?;
    Ok(Isolated {
        guarded,
        ..isolated
    })
}

/// Numéros gardés (disque, partitions, et frères si `whole_controller`),
/// après contrôle des protections des frères et des détenteurs.
fn guard(
    paths: &SysfsPaths,
    expected: &DiskIdentity,
    scan: &Scan,
    whole_controller: bool,
) -> Result<Vec<DevNum>> {
    let mut guarded = vec![devnum_of(expected)];
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
    Ok(guarded)
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

    fn opener(comm: &str) -> Opener {
        Opener {
            pid: 7,
            comm: comm.into(),
            fd: 3,
            target: "/dev/nvme0n1".into(),
        }
    }

    #[test]
    fn transient_probers_are_awaited_until_they_leave() {
        // Sonde udev puis udisks, puis plus personne : accepté.
        let mut rounds = vec![
            vec![],
            vec![opener("udisksd")],
            vec![opener("(udev-worker)"), opener("(udev-worker)")],
        ];
        let step = Duration::from_millis(1);
        assert!(
            wait_until_released(|| rounds.pop().unwrap_or_default(), SETTLE_TIMEOUT, step).is_ok()
        );
    }

    #[test]
    fn any_other_holder_is_refused_at_once() {
        let mut calls = 0;
        let err = wait_until_released(
            || {
                calls += 1;
                vec![opener("(udev-worker)"), opener("dd")]
            },
            SETTLE_TIMEOUT,
            Duration::from_millis(1),
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert!(err.to_string().contains("dd"), "{err}");
    }

    #[test]
    fn a_probe_that_never_leaves_is_refused_at_the_deadline() {
        let err = wait_until_released(
            || vec![opener("(udev-worker)")],
            Duration::from_millis(30),
            Duration::from_millis(5),
        )
        .unwrap_err();
        assert!(err.to_string().contains("udev-worker"), "{err}");
    }

    #[test]
    fn whole_disk_lock_is_exclusive_and_waits_then_refuses() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let first = File::open(tmp.path()).unwrap();
        let second = File::open(tmp.path()).unwrap();
        lock_whole_disk(&first, tmp.path(), Duration::ZERO).unwrap();
        let err = lock_whole_disk(&second, tmp.path(), Duration::from_millis(150)).unwrap_err();
        assert!(err.to_string().contains("flock"), "{err}");
        drop(first);
        lock_whole_disk(&second, tmp.path(), Duration::ZERO).unwrap();
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
