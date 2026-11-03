//! Protections système d'un périphérique bloc : monté, swap actif, membre
//! d'un ensemble LVM / md RAID / dm-crypt, ou hébergeant le système.
//!
//! Remplace la lecture de `/proc/mounts` et de `lsblk -s` de SecMet. Les
//! sources sont `/proc/self/mountinfo` (numéros majeur:mineur, sans
//! ambiguïté de nom), `/proc/swaps`, et dans sysfs les liens `slaves/`
//! (pour redescendre d'un volume logique vers ses disques) et `holders/`
//! (pour savoir qu'un disque est pris par dm, md, bcache…).
//!
//! Politique : ce qui ne peut pas être rattaché à un disque fait échouer le
//! calcul ([`DeviceError::Untraceable`]). Le helper doit alors refuser, et
//! non supposer qu'un disque est libre.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::path::Path;

use crate::sysfs::{self, DiskEntry, SysfsPaths};
use crate::{DeviceError, Result};

/// Points de montage dont le disque est le disque système.
pub const SYSTEM_MOUNT_POINTS: &[&str] = &["/", "/boot", "/boot/efi", "/efi"];

/// Profondeur maximale d'une pile dm-crypt sur LVM sur md…
const MAX_STACK_DEPTH: usize = 16;

/// Numéro de périphérique majeur:mineur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DevNum {
    /// Majeur.
    pub major: u32,
    /// Mineur.
    pub minor: u32,
}

impl DevNum {
    /// Depuis `major:minor` (contenu d'un fichier `dev` de sysfs).
    pub fn parse(text: &str) -> Option<Self> {
        let (major, minor) = sysfs::parse_devnum(text)?;
        Some(Self { major, minor })
    }

    /// Depuis un `st_rdev` ou `st_dev`.
    pub fn from_rdev(rdev: u64) -> Self {
        Self {
            major: libc::major(rdev),
            minor: libc::minor(rdev),
        }
    }
}

impl fmt::Display for DevNum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.major, self.minor)
    }
}

/// Une ligne de `mountinfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountEntry {
    /// Périphérique monté (majeur 0 : système de fichiers virtuel ou anonyme).
    pub dev: DevNum,
    /// Point de montage, désescapé.
    pub mount_point: String,
    /// Type de système de fichiers.
    pub fs_type: String,
    /// Source (`/dev/sda1`, `tmpfs`…), désescapée.
    pub source: String,
}

/// Désescape `\040`, `\011`, `\012`, `\134` de `mountinfo` et `swaps`.
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            if let Ok(value) = field
                .get(i + 1..i + 4)
                .map_or(Err(()), |oct| u8::from_str_radix(oct, 8).map_err(|_| ()))
            {
                out.push(value);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Analyse le contenu de `/proc/self/mountinfo`.
pub fn parse_mountinfo(text: &str) -> Result<Vec<MountEntry>> {
    let mut entries = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let invalid =
            || DeviceError::Inventory(format!("mountinfo ligne {}: format inattendu", index + 1));
        let (before, after) = line.split_once(" - ").ok_or_else(invalid)?;
        let before: Vec<&str> = before.split(' ').collect();
        let after: Vec<&str> = after.split(' ').collect();
        if before.len() < 6 || after.len() < 2 {
            return Err(invalid());
        }
        let dev = DevNum::parse(before[2]).ok_or_else(invalid)?;
        entries.push(MountEntry {
            dev,
            mount_point: unescape(before[4]),
            fs_type: after[0].to_string(),
            source: unescape(after[1]),
        });
    }
    Ok(entries)
}

/// Une ligne de `/proc/swaps`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapEntry {
    /// Chemin (`/dev/sda2`, `/swapfile`).
    pub path: String,
    /// `partition` ou `file`.
    pub kind: String,
}

/// Analyse `/proc/swaps` (en-tête ignoré).
pub fn parse_swaps(text: &str) -> Vec<SwapEntry> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let path = unescape(fields.next()?);
            let kind = fields.next()?.to_string();
            Some(SwapEntry { path, kind })
        })
        .collect()
}

/// Ce qui rend un disque intouchable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Protection {
    /// Héberge `/`, `/boot` ou la partition EFI.
    pub hosts_system: bool,
    /// Le disque ou une de ses partitions est monté (directement ou via dm/md/loop).
    pub mounted: bool,
    /// Une partition est un swap actif.
    pub swap: bool,
    /// Le disque ou une partition est tenu par dm, md, bcache… (`holders/`).
    pub held: bool,
}

impl Protection {
    /// Au moins une protection s'applique.
    pub fn any(&self) -> bool {
        self.hosts_system || self.mounted || self.swap || self.held
    }

    /// Libellé court pour les journaux et l'UI.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.hosts_system {
            parts.push("disque système");
        }
        if self.mounted {
            parts.push("monté");
        }
        if self.swap {
            parts.push("swap actif");
        }
        if self.held {
            parts.push("membre LVM/RAID/dm");
        }
        if parts.is_empty() {
            "libre".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// Protections de tous les disques de l'inventaire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectionReport {
    /// Par nom de disque entier.
    pub by_disk: BTreeMap<String, Protection>,
    /// Le disque sous `/` a été identifié. Faux sur un système live (overlay,
    /// NFS) : aucun disque n'est alors marqué système.
    pub root_traced: bool,
}

impl ProtectionReport {
    /// Protection d'un disque ; libre s'il est inconnu de l'inventaire.
    pub fn of(&self, disk: &str) -> Protection {
        self.by_disk.get(disk).copied().unwrap_or_default()
    }
}

/// Détail des protections du disque entier qui porte `devnum`.
pub fn protection_of(paths: &SysfsPaths, devnum: DevNum) -> Result<Protection> {
    let disks = sysfs::list_disks(paths)?;
    let name = name_of(paths, devnum)
        .ok_or_else(|| DeviceError::Inventory(format!("aucun périphérique bloc {devnum}")))?;
    let owners = sysfs::owners(&disks);
    let disk = owners
        .get(&name)
        .ok_or_else(|| {
            DeviceError::Inventory(format!(
                "{name} ({devnum}) n'est pas un disque de l'allowlist ni une de ses partitions"
            ))
        })?
        .clone();
    Ok(protections(paths, &disks)?.of(&disk))
}

/// Nom noyau d'un numéro de périphérique, par `/sys/class/block/*/dev`.
pub fn name_of(paths: &SysfsPaths, devnum: DevNum) -> Option<String> {
    let class = paths.sys.join("class/block");
    sysfs::entry_names(&class).ok()?.into_iter().find(|name| {
        sysfs::read_optional(&class.join(name).join("dev")).and_then(|t| DevNum::parse(&t))
            == Some(devnum)
    })
}

/// Calcule les protections de chaque disque à partir de `mountinfo`, `swaps` et sysfs.
pub fn protections(paths: &SysfsPaths, disks: &[DiskEntry]) -> Result<ProtectionReport> {
    let mounts = parse_mountinfo(&read(&paths.proc.join("self/mountinfo"))?)?;
    let swaps = parse_swaps(&read(&paths.proc.join("swaps"))?);
    let resolver = Resolver::new(paths, disks)?;

    let mut by_disk: BTreeMap<String, Protection> = disks
        .iter()
        .map(|disk| (disk.name.clone(), Protection::default()))
        .collect();
    let mut root_traced = false;

    for mount in &mounts {
        let Some(name) = resolver.mount_device(mount)? else {
            continue; // proc, tmpfs, overlay, nfs… : aucun périphérique bloc local
        };
        let system = SYSTEM_MOUNT_POINTS.contains(&mount.mount_point.as_str());
        let physical = resolver.physical_disks(&name)?;
        if mount.mount_point == "/" && !physical.is_empty() {
            root_traced = true;
        }
        for disk in physical {
            if let Some(protection) = by_disk.get_mut(&disk) {
                protection.mounted = true;
                protection.hosts_system |= system;
            }
        }
    }

    for swap in swaps.iter().filter(|s| s.kind != "file") {
        let name = resolver
            .dev_path_name(&swap.path)
            .ok_or_else(|| DeviceError::Untraceable(swap.path.clone()))?;
        for disk in resolver.physical_disks(&name)? {
            if let Some(protection) = by_disk.get_mut(&disk) {
                protection.swap = true;
            }
        }
    }

    for disk in disks {
        if resolver.has_holders(disk) {
            if let Some(protection) = by_disk.get_mut(&disk.name) {
                protection.held = true;
            }
        }
    }

    Ok(ProtectionReport {
        by_disk,
        root_traced,
    })
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| DeviceError::io(path.display().to_string(), e))
}

/// Remonte des périphériques bloc (partitions, dm, md, loop) vers les disques.
struct Resolver<'a> {
    paths: &'a SysfsPaths,
    /// Nom de disque ou de partition → nom de disque.
    owner: BTreeMap<String, String>,
    /// Majeur:mineur → nom noyau, pour tout périphérique bloc.
    names: HashMap<DevNum, String>,
}

impl<'a> Resolver<'a> {
    fn new(paths: &'a SysfsPaths, disks: &[DiskEntry]) -> Result<Self> {
        let class = paths.sys.join("class/block");
        let mut names = HashMap::new();
        for name in sysfs::entry_names(&class)
            .map_err(|e| DeviceError::io(class.display().to_string(), e))?
        {
            if let Some(dev) =
                sysfs::read_optional(&class.join(&name).join("dev")).and_then(|t| DevNum::parse(&t))
            {
                names.insert(dev, name);
            }
        }
        Ok(Self {
            paths,
            owner: sysfs::owners(disks),
            names,
        })
    }

    /// Nom noyau du périphérique d'un montage ; `None` sans périphérique bloc.
    fn mount_device(&self, mount: &MountEntry) -> Result<Option<String>> {
        if mount.dev.major != 0 {
            return self
                .names
                .get(&mount.dev)
                .cloned()
                .map(Some)
                .ok_or_else(|| {
                    DeviceError::Untraceable(format!("{} ({})", mount.source, mount.dev))
                });
        }
        if mount.source.starts_with("/dev/") {
            // btrfs et quelques autres annoncent un majeur anonyme.
            return self
                .dev_path_name(&mount.source)
                .map(Some)
                .ok_or_else(|| DeviceError::Untraceable(mount.source.clone()));
        }
        // Racine sans périphérique bloc local (overlay d'un média live,
        // tmpfs, NFS) : aucun disque n'est le disque système, et c'est
        // précisément le cas d'usage d'un banc démarré sur média live ;
        // `root_traced` reste faux pour le signaler. Un pool ZFS (`pool/ds`)
        // n'est pas tracé non plus : limite connue, documentée.
        Ok(None)
    }

    /// Nom noyau derrière `/dev/…`, liens résolus (`/dev/mapper/x` → `dm-0`).
    fn dev_path_name(&self, path: &str) -> Option<String> {
        let relative = path.strip_prefix("/dev/")?;
        let real = fs::canonicalize(self.paths.dev.join(relative)).ok()?;
        Some(real.file_name()?.to_string_lossy().into_owned())
    }

    fn physical_disks(&self, name: &str) -> Result<BTreeSet<String>> {
        let mut disks = BTreeSet::new();
        self.collect(name, 0, &mut disks)?;
        Ok(disks)
    }

    fn collect(&self, name: &str, depth: usize, disks: &mut BTreeSet<String>) -> Result<()> {
        if let Some(disk) = self.owner.get(name) {
            disks.insert(disk.clone());
            return Ok(());
        }
        if depth >= MAX_STACK_DEPTH {
            return Err(DeviceError::Untraceable(name.to_string()));
        }
        let class = self.paths.sys.join("class/block").join(name);
        if let Some(backing) = sysfs::read_optional(&class.join("loop/backing_file")) {
            if backing.starts_with("/dev/") {
                let backing = self
                    .dev_path_name(&backing)
                    .ok_or_else(|| DeviceError::Untraceable(backing.clone()))?;
                self.collect(&backing, depth + 1, disks)?;
            }
            // Loop sur un fichier ordinaire : son système de fichiers est déjà tracé.
            return Ok(());
        }
        let slaves = class.join("slaves");
        let slaves = match sysfs::entry_names(&slaves) {
            Ok(slaves) => slaves,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(DeviceError::io(slaves.display().to_string(), e)),
        };
        // Sans pile dessous (zram, nbd, sr0…) : pas un de nos disques.
        for slave in slaves {
            self.collect(&slave, depth + 1, disks)?;
        }
        Ok(())
    }

    /// Le disque ou une partition a-t-il des `holders/` ? Un répertoire
    /// illisible compte comme tenu : on ne peut pas prouver qu'il est libre.
    fn has_holders(&self, disk: &DiskEntry) -> bool {
        let class = self.paths.sys.join("class/block");
        std::iter::once(disk.name.as_str())
            .chain(disk.partitions.iter().map(String::as_str))
            .any(
                |name| match sysfs::entry_names(&class.join(name).join("holders")) {
                    Ok(entries) => !entries.is_empty(),
                    Err(e) => e.kind() != std::io::ErrorKind::NotFound,
                },
            )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::sysfs::tests::Fixture;
    use std::os::unix::fs::symlink;

    fn mountinfo(lines: &[(&str, &str, &str, &str)]) -> String {
        lines
            .iter()
            .enumerate()
            .map(|(i, (dev, point, fs, source))| {
                format!(
                    "{} 1 {dev} / {point} rw,relatime shared:1 - {fs} {source} rw",
                    i + 20
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    fn prepare(f: &Fixture, mounts: &str, swaps: &str) {
        f.write("proc/self/mountinfo", mounts);
        f.write(
            "proc/swaps",
            &format!("Filename Type Size Used Priority\n{swaps}"),
        );
    }

    fn scan(f: &Fixture) -> ProtectionReport {
        let disks = sysfs::list_disks(&f.paths()).expect("disques");
        protections(&f.paths(), &disks).expect("protections")
    }

    #[test]
    fn devnum_parses_and_displays() {
        let dev = DevNum::parse("8:16\n").expect("devnum");
        assert_eq!(
            dev,
            DevNum {
                major: 8,
                minor: 16
            }
        );
        assert_eq!(dev.to_string(), "8:16");
        assert_eq!(DevNum::parse("x"), None);
        assert_eq!(
            DevNum::from_rdev(libc::makedev(259, 3)),
            DevNum {
                major: 259,
                minor: 3
            }
        );
    }

    #[test]
    fn parses_mountinfo_and_swaps_with_escapes() {
        let text = "36 1 8:1 / /mnt/my\\040disk rw - ext4 /dev/sda1 rw\n37 1 0:25 / /tmp rw - tmpfs tmpfs rw\n";
        let entries = parse_mountinfo(text).expect("valide");
        assert_eq!(entries[0].mount_point, "/mnt/my disk");
        assert_eq!(entries[0].dev, DevNum { major: 8, minor: 1 });
        assert_eq!(entries[1].fs_type, "tmpfs");
        assert!(parse_mountinfo("garbage\n").is_err());
        let swaps = parse_swaps(
            "Filename Type Size Used Priority\n/dev/sda2 partition 8388604 0 -2\n/swap\\040file file 1 0 -3\n",
        );
        assert_eq!(swaps[0].path, "/dev/sda2");
        assert_eq!(swaps[1].path, "/swap file");
        assert_eq!(swaps[1].kind, "file");
    }

    #[test]
    fn system_disk_data_disk_and_free_disk() {
        let f = Fixture::new();
        f.disk("nvme0n1", "259:0", "pci0000:00/nvme/nvme0")
            .partition("nvme0n1", "nvme0n1p1", "259:1")
            .partition("nvme0n1", "nvme0n1p2", "259:2")
            .disk("sda", "8:0", "pci0000:00/ata1/host0/target0:0:0/0:0:0:0")
            .partition("sda", "sda1", "8:1")
            .disk("sdb", "8:16", "pci0000:00/ata2/host1/target1:0:0/1:0:0:0");
        prepare(
            &f,
            &mountinfo(&[
                ("259:2", "/", "ext4", "/dev/nvme0n1p2"),
                ("259:1", "/boot/efi", "vfat", "/dev/nvme0n1p1"),
                ("8:1", "/mnt/data", "xfs", "/dev/sda1"),
                ("0:22", "/proc", "proc", "proc"),
            ]),
            "",
        );
        let report = scan(&f);
        assert!(report.root_traced);
        let system = report.of("nvme0n1");
        assert!(system.hosts_system && system.mounted && !system.swap && !system.held);
        assert!(report.of("sda").mounted && !report.of("sda").hosts_system);
        assert_eq!(report.of("sdb"), Protection::default());
        assert_eq!(report.of("sdb").summary(), "libre");
        assert!(report.of("nvme0n1").summary().contains("disque système"));

        // L'API par numéro de périphérique : partition, disque, inconnu.
        let paths = f.paths();
        assert!(
            protection_of(&paths, DevNum { major: 8, minor: 1 })
                .expect("sda1")
                .mounted
        );
        assert!(
            protection_of(&paths, DevNum { major: 8, minor: 0 })
                .expect("sda")
                .mounted
        );
        assert!(!protection_of(
            &paths,
            DevNum {
                major: 8,
                minor: 16
            }
        )
        .expect("sdb")
        .any());
        assert!(matches!(
            protection_of(
                &paths,
                DevNum {
                    major: 8,
                    minor: 99
                }
            ),
            Err(DeviceError::Inventory(_))
        ));
    }

    #[test]
    fn root_on_luks_over_lvm_is_traced_to_the_physical_disk() {
        let f = Fixture::new();
        f.disk("sda", "8:0", "pci0000:00/ata1/host0/target0:0:0/0:0:0:0")
            .partition("sda", "sda2", "8:2")
            .disk("sdb", "8:16", "pci0000:00/ata2/host1/target1:0:0/1:0:0:0");
        f.class_entry("dm-0", "253:0");
        f.class_entry("dm-1", "253:1");
        fs::create_dir_all(f.path("sys/class/block/dm-0/slaves/sda2")).expect("slave");
        fs::create_dir_all(f.path("sys/class/block/dm-1/slaves/dm-0")).expect("slave");
        fs::create_dir_all(f.path("sys/class/block/sda2/holders/dm-0")).expect("holder");
        symlink("../dm-1", f.path("dev/mapper/vg-root")).expect("lien mapper");
        prepare(
            &f,
            &mountinfo(&[("253:1", "/", "ext4", "/dev/mapper/vg-root")]),
            "",
        );
        let report = scan(&f);
        let sda = report.of("sda");
        assert!(sda.hosts_system && sda.mounted && sda.held);
        assert_eq!(report.of("sdb"), Protection::default());
        assert!(protection_of(&f.paths(), DevNum { major: 8, minor: 0 })
            .expect("sda")
            .any());
    }

    #[test]
    fn swap_partition_btrfs_anonymous_major_and_loop_over_partition() {
        let f = Fixture::new();
        f.disk("sda", "8:0", "pci0000:00/ata1/host0/target0:0:0/0:0:0:0")
            .partition("sda", "sda1", "8:1")
            .partition("sda", "sda2", "8:2")
            .disk("sdb", "8:16", "pci0000:00/ata2/host1/target1:0:0/1:0:0:0")
            .partition("sdb", "sdb1", "8:17");
        f.class_entry("loop3", "7:3");
        f.write("sys/class/block/loop3/loop/backing_file", "/dev/sdb1");
        prepare(
            &f,
            &mountinfo(&[
                ("0:31", "/", "btrfs", "/dev/sda1"),
                ("7:3", "/srv/image", "ext4", "/dev/loop3"),
            ]),
            "/dev/sda2 partition 8388604 0 -2\n",
        );
        let report = scan(&f);
        assert!(report.of("sda").hosts_system && report.of("sda").swap);
        assert!(
            report.of("sdb").mounted,
            "loop sur partition tracé jusqu'au disque"
        );
    }

    #[test]
    fn untraceable_mount_or_swap_fails_closed() {
        let f = Fixture::new();
        f.disk("sda", "8:0", "pci0000:00/ata1/host0/target0:0:0/0:0:0:0");
        prepare(&f, &mountinfo(&[("8:99", "/srv", "ext4", "/dev/sdz1")]), "");
        let disks = sysfs::list_disks(&f.paths()).expect("disques");
        assert!(matches!(
            protections(&f.paths(), &disks),
            Err(DeviceError::Untraceable(_))
        ));
        prepare(&f, "", "/dev/nowhere partition 1 0 -2\n");
        assert!(matches!(
            protections(&f.paths(), &disks),
            Err(DeviceError::Untraceable(_))
        ));
    }

    #[test]
    fn live_system_without_root_disk_is_reported_and_unreadable_holders_count_as_held() {
        let f = Fixture::new();
        f.disk("sda", "8:0", "pci0000:00/ata1/host0/target0:0:0/0:0:0:0");
        prepare(&f, &mountinfo(&[("0:28", "/", "overlay", "overlay")]), "");
        let report = scan(&f);
        assert!(!report.root_traced);
        assert_eq!(report.of("sda"), Protection::default());
        // holders/ remplacé par un fichier : illisible, donc « tenu ».
        fs::remove_dir(f.path("sys/class/block/sda/holders")).expect("rm");
        f.write("sys/class/block/sda/holders", "x");
        assert!(scan(&f).of("sda").held);
    }
}
