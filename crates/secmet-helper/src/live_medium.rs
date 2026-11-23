//! Clé live : rebranchement après une veille qui a déconnecté la clé.
//!
//! Certains portables coupent l'USB pendant la veille du dégel (constaté le
//! 2026-11-23 sur HP EliteBook Folio 1020 G1) : au réveil, la clé revient
//! sous un autre nom (`sdb` → `sdc`). Le système lu sur la clé
//! (`filesystem.squashfs` derrière un périphérique loop) pointe alors vers
//! un périphérique disparu, `/run/live/medium` reste monté dessus, et les
//! partitions de données et d'export sont démontées par systemd.
//!
//! `secmet-helper key-watch` (service `secmet-key-watch`), veilleur résident
//! qui regarde chaque seconde `mountinfo` et sysfs (rien sur la clé) :
//! 1. média live sur un périphérique disparu, et partition système revenue
//!    avec le PARTUUID relevé au démarrage (`/run/secmet/`, voir
//!    `secmet-key-watch-prepare`) : démonte paresseusement l'ancien
//!    `/run/live/medium`, y monte la partition revenue en lecture seule,
//!    puis rebranche chaque loop dessus (`LOOP_CHANGE_FD`, même fichier,
//!    même taille) ;
//! 2. partition de données démontée et revenue avec son PARTUUID (et
//!    `by-label/SECMET-DATA` qui la désigne bien, jamais une autre clé) :
//!    `systemctl start /media/secmet-data /media/secmet-export`.
//!
//! Binaire, chargeur et bibliothèques sont copiés en mémoire au démarrage :
//! au réveil, rien n'est lisible sur la clé perdue, pas même `systemd-executor`
//! (un service lancé à ce moment échoue ; constaté en VM, cache vidé), d'où
//! un veilleur déjà en marche plutôt qu'une règle udev.

use std::ffi::CString;
use std::fs::{self, File};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use secmet_device::protection::{parse_mountinfo, DevNum, MountEntry};
use secmet_device::sys;
use secmet_device::sysfs::SysfsPaths;

use crate::error::{HelperError, Result};

/// Point de montage du média live (live-boot).
pub const MEDIUM: &str = "/run/live/medium";
/// PARTUUID de la partition système, relevé au démarrage.
pub const SYSTEM_PARTUUID: &str = "/run/secmet/system.partuuid";
/// PARTUUID de la partition de données, relevé au démarrage.
pub const DATA_PARTUUID: &str = "/run/secmet/data.partuuid";
/// Période du veilleur.
const PERIOD: Duration = Duration::from_secs(1);
/// Point de montage des données de la clé.
pub const DATA: &str = "/media/secmet-data";

/// Issue d'un rebranchement.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Pas de média live monté depuis un périphérique bloc (copie en mémoire,
    /// poste hors clé) : rien à rebrancher.
    NoBlockMedium,
    /// Le média est toujours là.
    Present,
    /// Média rebranché sur `device` ; loops rebranchés.
    Reattached {
        /// Partition revenue.
        device: PathBuf,
        /// Loops rebranchés (`loop0`…).
        loops: Vec<String>,
    },
}

/// Entrée de `mountinfo` du média live, s'il est monté depuis un
/// périphérique bloc.
pub fn medium_entry(mounts: &[MountEntry]) -> Option<&MountEntry> {
    mounts
        .iter()
        .rev()
        .find(|m| m.mount_point == MEDIUM && m.dev.major != 0)
}

/// Le périphérique `dev` existe-t-il encore (`/sys/dev/block/M:m`) ?
pub fn device_exists(sys_root: &Path, dev: DevNum) -> bool {
    sys_root.join("dev/block").join(dev.to_string()).exists()
}

/// Média live monté sur un périphérique disparu (clé perdue pendant la
/// veille) : sa source, pour les messages.
pub fn lost_medium(paths: &SysfsPaths) -> Option<String> {
    let text = fs::read_to_string(paths.proc.join("self/mountinfo")).ok()?;
    let mounts = parse_mountinfo(&text).ok()?;
    let entry = medium_entry(&mounts)?;
    (!device_exists(&paths.sys, entry.dev)).then(|| format!("{} ({})", entry.source, entry.dev))
}

/// Loops dont le fichier est sous `prefix/` : (nom, chemin relatif).
pub fn loops_under(sys_root: &Path, prefix: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(sys_root.join("block")) else {
        return found;
    };
    let prefix = format!("{}/", prefix.trim_end_matches('/'));
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("loop") {
            continue;
        }
        let Ok(backing) = fs::read_to_string(entry.path().join("loop/backing_file")) else {
            continue;
        };
        if let Some(relative) = backing.trim_end().strip_prefix(&prefix) {
            found.push((name, relative.to_string()));
        }
    }
    found.sort();
    found
}

fn read_partuuid(file: &str) -> Option<String> {
    let text = fs::read_to_string(file).ok()?;
    let id = text.trim();
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
        .then(|| id.to_ascii_lowercase())
}

/// GUID de partition GPT (16 octets, champs 1 à 3 petit-boutistes) au
/// format des PARTUUID (`3ac20b7a-b3b8-4f46-810f-d45ce6182d99`).
pub fn guid_text(raw: &[u8]) -> Option<String> {
    let b: &[u8; 16] = raw.get(..16)?.try_into().ok()?;
    Some(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    ))
}

/// Numéro (1, 2…) de la partition `id` dans une table GPT : `read(lba,
/// count)` lit `count` blocs logiques à partir de `lba`.
pub fn gpt_partition_number(
    mut read: impl FnMut(u64, u64) -> Option<Vec<u8>>,
    id: &str,
) -> Option<u32> {
    let header = read(1, 1)?;
    if header.get(..8)? != b"EFI PART" {
        return None;
    }
    let u64_at = |o: usize| Some(u64::from_le_bytes(header.get(o..o + 8)?.try_into().ok()?));
    let u32_at = |o: usize| Some(u32::from_le_bytes(header.get(o..o + 4)?.try_into().ok()?));
    let entries_lba = u64_at(72)?;
    let count = u32_at(80)?.min(256);
    let size = usize::try_from(u32_at(84)?).ok()?;
    if !(128..=4096).contains(&size) {
        return None;
    }
    let block = header.len();
    let total = usize::try_from(count).ok()? * size;
    let blocks = u64::try_from(total.div_ceil(block)).ok()?;
    let table = read(entries_lba, blocks)?;
    (0..count).find_map(|n| {
        let at = usize::try_from(n).ok()? * size;
        let entry = table.get(at..at + size)?;
        (guid_text(entry.get(16..32)?)? == id).then_some(n + 1)
    })
}

/// Partition de PARTUUID `id`, cherchée en lisant la table GPT de chaque
/// disque (`/dev/<disque>`, nœuds du noyau) : rien ne dépend de udev, qui
/// au réveil lance des programmes restés sur la clé perdue.
pub fn find_partition(paths: &SysfsPaths, id: &str) -> Option<PathBuf> {
    use std::io::{Read, Seek, SeekFrom};
    let disks = fs::read_dir(paths.sys.join("block")).ok()?;
    for disk in disks.flatten() {
        let name = disk.file_name().to_string_lossy().into_owned();
        if name.starts_with("loop") || name.starts_with("zram") || name.starts_with("ram") {
            continue;
        }
        let block = fs::read_to_string(disk.path().join("queue/logical_block_size"))
            .ok()
            .and_then(|t| t.trim().parse::<u64>().ok())
            .filter(|b| *b >= 512)
            .unwrap_or(512);
        let Ok(mut file) = File::open(paths.dev.join(&name)) else {
            continue;
        };
        let read = |lba: u64, count: u64| {
            let mut buf = vec![0u8; usize::try_from(block.checked_mul(count)?).ok()?];
            file.seek(SeekFrom::Start(lba.checked_mul(block)?)).ok()?;
            file.read_exact(&mut buf).ok()?;
            Some(buf)
        };
        let Some(number) = gpt_partition_number(read, id) else {
            continue;
        };
        let parts = fs::read_dir(disk.path()).ok()?;
        for part in parts.flatten() {
            let partition = fs::read_to_string(part.path().join("partition")).ok();
            if partition.as_deref().map(str::trim) == Some(&number.to_string()) {
                return Some(paths.dev.join(part.file_name()));
            }
        }
    }
    None
}

fn cstring(text: &str) -> Result<CString> {
    CString::new(text).map_err(|_| HelperError::Refused(format!("chemin invalide : {text:?}")))
}

/// Rebranche le média live si sa clé est revenue sous un autre nom.
pub fn reattach(paths: &SysfsPaths) -> Result<Outcome> {
    let mountinfo = paths.proc.join("self/mountinfo");
    let text = fs::read_to_string(&mountinfo)
        .map_err(|e| HelperError::io(mountinfo.display().to_string(), e))?;
    let mounts = parse_mountinfo(&text)?;
    let Some(entry) = medium_entry(&mounts) else {
        return Ok(Outcome::NoBlockMedium);
    };
    if device_exists(&paths.sys, entry.dev) {
        return Ok(Outcome::Present);
    }
    let id = read_partuuid(SYSTEM_PARTUUID).ok_or_else(|| {
        HelperError::Refused(format!(
            "{SYSTEM_PARTUUID} absent : clé démarrée non identifiée"
        ))
    })?;
    let device = find_partition(paths, &id)
        .ok_or_else(|| HelperError::Refused(format!("partition système {id} non revenue")))?;

    // Relevés avant le démontage : le chemin des loops change ensuite.
    let loops = loops_under(&paths.sys, MEDIUM);
    let target = cstring(MEDIUM)?;
    sys::umount_detach(&target).map_err(|e| HelperError::io(format!("umount {MEDIUM}"), e))?;
    sys::mount_read_only(
        &cstring(&device.to_string_lossy())?,
        &target,
        &cstring(&entry.fs_type)?,
        &cstring(if entry.fs_type == "vfat" {
            "utf8,shortname=mixed"
        } else {
            ""
        })?,
    )
    .map_err(|e| HelperError::io(format!("mount {} {MEDIUM}", device.display()), e))?;

    let mut done = Vec::new();
    for (name, relative) in loops {
        let loop_path = paths.dev.join(&name);
        let file_path = Path::new(MEDIUM).join(&relative);
        let loop_dev = File::open(&loop_path)
            .map_err(|e| HelperError::io(loop_path.display().to_string(), e))?;
        let file = File::open(&file_path)
            .map_err(|e| HelperError::io(file_path.display().to_string(), e))?;
        sys::loop_change_fd(loop_dev.as_fd(), file.as_fd())
            .map_err(|e| HelperError::io(format!("LOOP_CHANGE_FD {name}"), e))?;
        done.push(name);
    }
    Ok(Outcome::Reattached {
        device,
        loops: done,
    })
}

/// `by-label/SECMET-DATA` (le `LABEL=` de fstab) désigne-t-il bien
/// `ours` ? Sinon une autre clé SecMet branchée recevrait les certificats.
fn label_is(paths: &SysfsPaths, ours: &Path) -> bool {
    fs::canonicalize(paths.dev.join("disk/by-label/SECMET-DATA")).is_ok_and(|label| label == ours)
}

/// Message au journal du service et au journal du noyau (lisible sur la
/// console même quand plus rien n'est lisible sur la clé).
fn log(message: &str) {
    use std::io::Write;
    eprintln!("key-watch : {message}");
    if let Ok(mut kmsg) = fs::OpenOptions::new().write(true).open("/dev/kmsg") {
        let _ = writeln!(kmsg, "<5>secmet-key-watch: {message}");
    }
}

/// Point de montage de l'export de la clé.
pub const EXPORT: &str = "/media/secmet-export";

/// Montages de la clé restés sur un périphérique disparu : systemd n'a pas
/// pu les démonter (son `umount` est lancé par `systemd-executor`, sur la
/// clé perdue). Démontés ici, paresseusement.
fn drop_stale_mounts(paths: &SysfsPaths) {
    let Some(mounts) = fs::read_to_string(paths.proc.join("self/mountinfo"))
        .ok()
        .and_then(|text| parse_mountinfo(&text).ok())
    else {
        return;
    };
    for mount in mounts.iter().rev() {
        let ours = mount.mount_point == DATA || mount.mount_point == EXPORT;
        if ours && mount.dev.major != 0 && !device_exists(&paths.sys, mount.dev) {
            let result = cstring(&mount.mount_point).and_then(|target| {
                sys::umount_detach(&target)
                    .map_err(|e| HelperError::io(format!("umount {}", mount.mount_point), e))
            });
            match result {
                Ok(()) => log(&format!(
                    "{} démonté ({} disparu)",
                    mount.mount_point, mount.source
                )),
                Err(e) => log(&e.to_string()),
            }
        }
    }
}

/// Les données de la clé sont-elles montées (sur un périphérique présent) ?
fn data_mounted(paths: &SysfsPaths) -> bool {
    fs::read_to_string(paths.proc.join("self/mountinfo"))
        .ok()
        .and_then(|text| parse_mountinfo(&text).ok())
        .is_some_and(|mounts| {
            mounts
                .iter()
                .any(|m| m.mount_point == DATA && device_exists(&paths.sys, m.dev))
        })
}

fn run(program: &str, args: &[&str]) -> bool {
    match std::process::Command::new(program).args(args).status() {
        Ok(status) if status.success() => true,
        Ok(status) => {
            log(&format!("{program} {}: {status}", args.join(" ")));
            false
        }
        Err(e) => {
            log(&format!("{program} : {e}"));
            false
        }
    }
}

/// Remonte données et export si la partition de données revenue est la
/// nôtre : udev rejoue d'abord ses règles (au retour de la clé, il n'a pas
/// pu créer les liens `by-label`, ses programmes étant sur la clé perdue).
/// Vrai si les données sont remontées ; faux pour une nouvelle tentative.
fn remount_data(paths: &SysfsPaths, data: &Path, last_attempt: bool) -> bool {
    let _ = run(
        "/usr/bin/udevadm",
        &[
            "trigger",
            "--settle",
            "--action=add",
            "--subsystem-match=block",
        ],
    );
    if !label_is(paths, data) {
        if last_attempt {
            log(&format!(
                "SECMET-DATA ne désigne pas {} : données non remontées",
                data.display()
            ));
        }
        return false;
    }
    let done = run("/usr/bin/systemctl", &["start", DATA, EXPORT]);
    if done {
        log("données et export remontés");
    }
    done
}

/// Nouvelles tentatives de remontage (udev, juste après le rebranchement,
/// n'a pas toujours recréé les liens `by-label` : constaté en VM).
const DATA_RETRIES: u32 = 12;
const DATA_RETRY_EVERY: Duration = Duration::from_secs(5);
/// Relecture des tables GPT une fois les tentatives épuisées.
const EXHAUSTED_RESCAN: Duration = Duration::from_secs(30);

/// `secmet-helper key-watch` : veilleur résident (ne rend jamais la main).
pub fn main() -> i32 {
    let paths = SysfsPaths::default();
    let data_id = read_partuuid(DATA_PARTUUID);
    // (partition, dernière tentative, tentatives) : une partition revenue est
    // retentée toutes les 5 s, 12 fois au plus, puis seulement si elle change.
    let mut data_try: Option<(PathBuf, std::time::Instant, u32)> = None;
    let mut last_error = String::new();
    log("surveillance de la clé");
    loop {
        match reattach(&paths) {
            Ok(Outcome::Reattached { device, loops }) => log(&format!(
                "système rebranché sur {} ({})",
                device.display(),
                loops.join(", ")
            )),
            Ok(_) => last_error.clear(),
            Err(e) => {
                let text = e.to_string();
                if text != last_error {
                    log(&text);
                    last_error = text;
                }
            }
        }
        drop_stale_mounts(&paths);
        // Tentatives épuisées : la table GPT des disques n'est plus relue
        // qu'une fois par 30 s (un disque ouvert ici chaque seconde gênerait
        // le contrôle des détenteurs d'un effacement).
        let exhausted = matches!(
            &data_try,
            Some((_, at, n)) if *n >= DATA_RETRIES && at.elapsed() < EXHAUSTED_RESCAN
        );
        if data_mounted(&paths) {
            data_try = None;
        } else if !exhausted {
            if let Some(data) = data_id.as_deref().and_then(|id| find_partition(&paths, id)) {
                let attempts = match &data_try {
                    Some((device, _, _)) if *device != data => 0,
                    Some((_, _, n)) => *n,
                    None => 0,
                };
                let wait_over = match &data_try {
                    Some((device, at, _)) if *device == data => at.elapsed() >= DATA_RETRY_EVERY,
                    _ => true,
                };
                if attempts >= DATA_RETRIES {
                    // Relue après 30 s, même partition : pas de nouvelle tentative.
                    data_try = Some((data, std::time::Instant::now(), attempts));
                } else if wait_over {
                    let done = remount_data(&paths, &data, attempts + 1 == DATA_RETRIES);
                    data_try = Some((
                        data,
                        std::time::Instant::now(),
                        if done { DATA_RETRIES } else { attempts + 1 },
                    ));
                }
            }
        }
        thread::sleep(PERIOD);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    const MOUNTINFO: &str = "\
26 1 0:24 / / rw - overlay overlay rw
30 26 8:17 / /run/live/medium ro,noatime - vfat /dev/sdb1 ro
31 26 7:0 / /run/live/rootfs/filesystem.squashfs ro - squashfs /dev/loop0 ro
40 26 0:27 / /run/live/overlay rw - tmpfs tmpfs rw
";

    #[test]
    fn medium_entry_finds_the_block_backed_live_medium_only() {
        let mounts = parse_mountinfo(MOUNTINFO).unwrap();
        let entry = medium_entry(&mounts).unwrap();
        assert_eq!(entry.source, "/dev/sdb1");
        assert_eq!(entry.dev.to_string(), "8:17");
        // Copie en mémoire : le média est un tmpfs, rien à rebrancher.
        let toram = parse_mountinfo("30 26 0:27 / /run/live/medium rw - tmpfs tmpfs rw\n").unwrap();
        assert!(medium_entry(&toram).is_none());
        assert!(medium_entry(&[]).is_none());
    }

    #[test]
    fn device_existence_and_loops_are_read_from_sysfs() {
        let tmp = tempfile::tempdir().unwrap();
        let sys_root = tmp.path();
        fs::create_dir_all(sys_root.join("dev/block/8:33")).unwrap();
        assert!(device_exists(sys_root, DevNum::parse("8:33").unwrap()));
        assert!(!device_exists(sys_root, DevNum::parse("8:17").unwrap()));
        for (name, backing) in [
            ("loop0", "/run/live/medium/live/filesystem.squashfs\n"),
            ("loop1", "/home/x.img\n"),
            ("loop2", "/run/live/mediumX/y\n"),
        ] {
            let dir = sys_root.join("block").join(name).join("loop");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("backing_file"), backing).unwrap();
        }
        fs::create_dir_all(sys_root.join("block/sda")).unwrap();
        assert_eq!(
            loops_under(sys_root, MEDIUM),
            vec![("loop0".to_string(), "live/filesystem.squashfs".to_string())]
        );
    }

    #[test]
    fn gpt_lookup_finds_the_partition_by_its_unique_guid() {
        // GUID 3ac20b7a-b3b8-4f46-810f-d45ce6182d99 en octets GPT.
        let guid = [
            0x7a, 0x0b, 0xc2, 0x3a, 0xb8, 0xb3, 0x46, 0x4f, 0x81, 0x0f, 0xd4, 0x5c, 0xe6, 0x18,
            0x2d, 0x99,
        ];
        assert_eq!(
            guid_text(&guid).unwrap(),
            "3ac20b7a-b3b8-4f46-810f-d45ce6182d99"
        );
        let mut disk = vec![0u8; 512 * 40];
        disk[512..520].copy_from_slice(b"EFI PART");
        disk[512 + 72..512 + 80].copy_from_slice(&2u64.to_le_bytes());
        disk[512 + 80..512 + 84].copy_from_slice(&128u32.to_le_bytes());
        disk[512 + 84..512 + 88].copy_from_slice(&128u32.to_le_bytes());
        // Troisième entrée.
        let at = 1024 + 2 * 128 + 16;
        disk[at..at + 16].copy_from_slice(&guid);
        let read = |lba: u64, count: u64| {
            let start = usize::try_from(lba * 512).ok()?;
            disk.get(start..start + usize::try_from(count * 512).ok()?)
                .map(<[u8]>::to_vec)
        };
        assert_eq!(
            gpt_partition_number(read, "3ac20b7a-b3b8-4f46-810f-d45ce6182d99"),
            Some(3)
        );
        assert_eq!(
            gpt_partition_number(read, "00000000-0000-0000-0000-000000000001"),
            None
        );
        assert_eq!(gpt_partition_number(|_, _| Some(vec![0u8; 512]), "x"), None);
    }

    #[test]
    fn lost_medium_reports_only_a_vanished_device() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = SysfsPaths {
            sys: tmp.path().join("sys"),
            proc: tmp.path().join("proc"),
            dev: tmp.path().join("dev"),
        };
        fs::create_dir_all(paths.proc.join("self")).unwrap();
        fs::write(paths.proc.join("self/mountinfo"), MOUNTINFO).unwrap();
        fs::create_dir_all(paths.sys.join("dev/block")).unwrap();
        assert_eq!(lost_medium(&paths).as_deref(), Some("/dev/sdb1 (8:17)"));
        fs::create_dir_all(paths.sys.join("dev/block/8:17")).unwrap();
        assert_eq!(lost_medium(&paths), None);
    }
}
