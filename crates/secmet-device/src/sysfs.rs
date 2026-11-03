//! Inventaire des disques par sysfs : remplace `lsblk -d -o NAME,SIZE,MODEL,TYPE,RO,ROTA,TRAN`.
//!
//! Lecture seule de `/sys` : aucun nœud `/dev` n'est ouvert ici. Le transport
//! est déduit du chemin réel du périphérique dans `/sys/devices` (`/usb`,
//! `/ata`, `sas_address`, `/virtio`) et, pour NVMe, du fichier `transport`
//! du contrôleur (`pcie` seulement : NVMe-oF est « autre »).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::block::is_allowed_name;
use crate::{DeviceError, Result};

/// Emplacement des pseudo-systèmes de fichiers. Les tests pointent un faux arbre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysfsPaths {
    /// sysfs, normalement `/sys`.
    pub sys: PathBuf,
    /// procfs, normalement `/proc`.
    pub proc: PathBuf,
    /// devtmpfs, normalement `/dev` (seuls les liens y sont résolus).
    pub dev: PathBuf,
}

impl Default for SysfsPaths {
    fn default() -> Self {
        Self {
            sys: PathBuf::from("/sys"),
            proc: PathBuf::from("/proc"),
            dev: PathBuf::from("/dev"),
        }
    }
}

/// Transport du disque, ce que `lsblk -o TRAN` approximait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transport {
    /// SATA natif (libata / AHCI).
    Sata,
    /// Derrière un HBA SAS (SATL du pilote, `-d sat` chez smartctl).
    Sas,
    /// Pont USB (UAS ou bulk-only), SAT du pont.
    Usb,
    /// NVMe local (PCIe).
    Nvme,
    /// virtio-blk.
    Virtio,
    /// Indéterminé, ou NVMe sur réseau.
    Other,
}

impl Transport {
    /// Libellé court, compatible avec les étiquettes SecMet existantes.
    pub fn label(self) -> &'static str {
        match self {
            Self::Sata => "sata",
            Self::Sas => "sas",
            Self::Usb => "usb",
            Self::Nvme => "nvme",
            Self::Virtio => "virtio",
            Self::Other => "",
        }
    }

    /// Les commandes ATA passent-elles par un SATL (pont USB ou HBA SAS) ?
    /// Sur ces transports, le repli vers ATA PASS-THROUGH (12) a un sens.
    pub fn behind_satl(self) -> bool {
        matches!(self, Self::Usb | Self::Sas)
    }
}

/// Un disque de l'inventaire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskEntry {
    /// Nom noyau (`sda`, `nvme0n1`).
    pub name: String,
    /// Chemin `/dev/<name>`.
    pub path: PathBuf,
    /// Majeur:mineur.
    pub major: u32,
    /// Mineur.
    pub minor: u32,
    /// Transport.
    pub transport: Transport,
    /// Taille en octets (`size` × 512).
    pub size_bytes: u64,
    /// `queue/rotational`, si lisible.
    pub rotational: Option<bool>,
    /// `ro`.
    pub read_only: bool,
    /// `removable`.
    pub removable: bool,
    /// `device/model`.
    pub model: Option<String>,
    /// `device/serial` ou page VPD 80h.
    pub serial: Option<String>,
    /// `device/firmware_rev` ou `device/rev`.
    pub firmware: Option<String>,
    /// `wwid`.
    pub wwid: Option<String>,
    /// Noms des partitions, triés.
    pub partitions: Vec<String>,
}

impl DiskEntry {
    /// Indice SSD d'après le noyau (`ROTA=0`) ; `None` si inconnu. Sur un
    /// pont USB ou SAS le noyau se trompe souvent : préférer IDENTIFY mot 217.
    pub fn solid_state_hint(&self) -> Option<bool> {
        self.rotational.map(|r| !r)
    }

    /// Taille lisible en unités binaires, comme la colonne `SIZE` de lsblk.
    pub fn size_label(&self) -> String {
        human_size(self.size_bytes)
    }
}

/// Taille lisible (`931,5G`), style lsblk.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}B")
    } else {
        format!("{value:.1}{}", UNITS[unit]).replace(".0", "")
    }
}

/// Liste les disques entiers de l'allowlist, avec leurs attributs sysfs.
pub fn list_disks(paths: &SysfsPaths) -> Result<Vec<DiskEntry>> {
    let block = paths.sys.join("block");
    let mut names =
        entry_names(&block).map_err(|e| DeviceError::io(block.display().to_string(), e))?;
    names.sort();
    names
        .into_iter()
        .filter(|name| is_allowed_name(name))
        .map(|name| read_disk(paths, &name))
        .collect()
}

/// Lit un disque donné de sysfs.
pub fn read_disk(paths: &SysfsPaths, name: &str) -> Result<DiskEntry> {
    if !is_allowed_name(name) {
        return Err(DeviceError::RefusedName(name.to_string()));
    }
    let dir = paths.sys.join("block").join(name);
    let (major, minor) = read_required(&dir.join("dev")).and_then(|text| {
        parse_devnum(&text).ok_or_else(|| DeviceError::Inventory(format!("{name}: dev illisible")))
    })?;
    let sectors: u64 = read_required(&dir.join("size"))?
        .trim()
        .parse()
        .map_err(|_| DeviceError::Inventory(format!("{name}: size illisible")))?;
    let mut partitions: Vec<String> = entry_names(&dir)
        .map_err(|e| DeviceError::io(dir.display().to_string(), e))?
        .into_iter()
        .filter(|entry| dir.join(entry).join("partition").is_file())
        .collect();
    partitions.sort();

    let real = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
    let transport = transport_of(name, &real, &dir);
    let device = dir.join("device");
    Ok(DiskEntry {
        name: name.to_string(),
        path: paths.dev.join(name),
        major,
        minor,
        transport,
        size_bytes: sectors.saturating_mul(512),
        rotational: read_optional(&dir.join("queue/rotational")).map(|v| v == "1"),
        read_only: read_optional(&dir.join("ro")).is_some_and(|v| v == "1"),
        removable: read_optional(&dir.join("removable")).is_some_and(|v| v == "1"),
        model: read_optional(&device.join("model")),
        serial: read_optional(&device.join("serial")).or_else(|| {
            fs::read(device.join("vpd_pg80"))
                .ok()
                .and_then(|p| vpd_pg80_serial(&p))
        }),
        firmware: read_optional(&device.join("firmware_rev"))
            .or_else(|| read_optional(&device.join("rev"))),
        wwid: read_optional(&dir.join("wwid")).or_else(|| read_optional(&device.join("wwid"))),
        partitions,
    })
}

/// Transport déduit du chemin réel et des fichiers du périphérique.
pub fn transport_of(name: &str, real: &Path, dir: &Path) -> Transport {
    let real = real.to_string_lossy();
    if name.starts_with("nvme") {
        return nvme_transport(dir);
    }
    if name.starts_with("vd") || real.contains("/virtio") {
        return Transport::Virtio;
    }
    if real.contains("/usb") {
        return Transport::Usb;
    }
    if dir.join("device/sas_address").is_file() || real.contains("/sas") {
        return Transport::Sas;
    }
    if real.contains("/ata") {
        return Transport::Sata;
    }
    Transport::Other
}

/// Transport d'un namespace NVMe : `pcie` sur tous ses contrôleurs, sinon « autre ».
fn nvme_transport(dir: &Path) -> Transport {
    let device = dir.join("device");
    let mut transports: Vec<String> = read_optional(&device.join("transport"))
        .into_iter()
        .collect();
    if transports.is_empty() {
        transports = entry_names(&device)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.starts_with("nvme"))
            .filter_map(|e| read_optional(&device.join(e).join("transport")))
            .collect();
    }
    if !transports.is_empty() && transports.iter().all(|t| t == "pcie") {
        Transport::Nvme
    } else {
        Transport::Other
    }
}

/// Numéro de série de la page VPD 80h (en-tête de 4 octets puis ASCII).
pub fn vpd_pg80_serial(page: &[u8]) -> Option<String> {
    let length = usize::from(*page.get(3)?);
    let serial = page.get(4..4 + length)?;
    let text = crate::ascii_trimmed(serial);
    (!text.is_empty()).then_some(text)
}

/// `major:minor` → tuple.
pub fn parse_devnum(text: &str) -> Option<(u32, u32)> {
    let (major, minor) = text.trim().split_once(':')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// Noms des entrées d'un répertoire.
pub fn entry_names(dir: &Path) -> io::Result<Vec<String>> {
    fs::read_dir(dir)?
        .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect()
}

/// Contenu d'un fichier sysfs, bourrage retiré ; `None` s'il est absent ou vide.
pub fn read_optional(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn read_required(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| DeviceError::io(path.display().to_string(), e))
}

/// Table `nom de disque ou partition → nom de disque`, pour tracer les montages.
pub fn owners(disks: &[DiskEntry]) -> BTreeMap<String, String> {
    let mut owners = BTreeMap::new();
    for disk in disks {
        owners.insert(disk.name.clone(), disk.name.clone());
        for partition in &disk.partitions {
            owners.insert(partition.clone(), disk.name.clone());
        }
    }
    owners
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    /// Faux arbre `/sys`, `/proc`, `/dev`.
    pub(crate) struct Fixture {
        pub(crate) root: TempDir,
    }

    impl Fixture {
        pub(crate) fn new() -> Self {
            let root = tempfile::tempdir().expect("tempdir");
            for dir in [
                "sys/block",
                "sys/class/block",
                "sys/devices",
                "proc/self",
                "dev/mapper",
            ] {
                fs::create_dir_all(root.path().join(dir)).expect("mkdir");
            }
            Self { root }
        }

        pub(crate) fn paths(&self) -> SysfsPaths {
            SysfsPaths {
                sys: self.root.path().join("sys"),
                proc: self.root.path().join("proc"),
                dev: self.root.path().join("dev"),
            }
        }

        pub(crate) fn path(&self, relative: &str) -> PathBuf {
            self.root.path().join(relative)
        }

        pub(crate) fn write(&self, relative: &str, content: &str) {
            let path = self.path(relative);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(path, format!("{content}\n")).expect("write");
        }

        pub(crate) fn class_entry(&self, name: &str, dev: &str) {
            self.write(&format!("sys/class/block/{name}/dev"), dev);
            fs::create_dir_all(self.path(&format!("sys/class/block/{name}/slaves")))
                .expect("slaves");
            fs::create_dir_all(self.path(&format!("sys/class/block/{name}/holders")))
                .expect("holders");
            self.write(&format!("dev/{name}"), "");
        }

        /// Un disque sous `sys/devices/<bus>/block/<name>`, lié depuis `sys/block`.
        pub(crate) fn disk(&self, name: &str, dev: &str, bus: &str) -> &Self {
            let real = format!("sys/devices/{bus}/block/{name}");
            self.write(&format!("{real}/dev"), dev);
            self.write(&format!("{real}/size"), "1953525168");
            self.write(&format!("{real}/queue/rotational"), "0");
            self.write(&format!("{real}/ro"), "0");
            self.write(&format!("{real}/removable"), "0");
            self.write(&format!("{real}/device/model"), "Fictional Drive  ");
            self.write(&format!("{real}/device/serial"), &format!("SN-{name}"));
            fs::create_dir_all(self.path(&format!("{real}/holders"))).expect("holders");
            if name.starts_with("nvme") {
                self.write(&format!("{real}/device/transport"), "pcie");
            }
            symlink(self.path(&real), self.path(&format!("sys/block/{name}"))).expect("link");
            self.class_entry(name, dev);
            self
        }

        pub(crate) fn partition(&self, disk: &str, name: &str, dev: &str) -> &Self {
            let dir = format!("sys/block/{disk}/{name}");
            self.write(&format!("{dir}/partition"), "1");
            self.write(&format!("{dir}/dev"), dev);
            fs::create_dir_all(self.path(&format!("{dir}/holders"))).expect("holders");
            self.class_entry(name, dev);
            self
        }
    }

    #[test]
    fn lists_disks_with_transport_and_identity() {
        let f = Fixture::new();
        f.disk("nvme0n1", "259:0", "pci0000:00/nvme/nvme0")
            .partition("nvme0n1", "nvme0n1p1", "259:1")
            .disk("sda", "8:0", "pci0000:00/ata1/host0/target0:0:0/0:0:0:0")
            .disk(
                "sdb",
                "8:16",
                "pci0000:00/usb2/2-1/2-1:1.0/host6/target6:0:0/6:0:0:0",
            )
            .disk(
                "sdc",
                "8:32",
                "pci0000:00/0000:03:00.0/host7/port-7:0/end_device-7:0/target7:0:0/7:0:0:0",
            );
        f.write("sys/devices/pci0000:00/0000:03:00.0/host7/port-7:0/end_device-7:0/target7:0:0/7:0:0:0/block/sdc/device/sas_address", "0x5000c500");
        f.class_entry("loop0", "7:0");
        let disks = list_disks(&f.paths()).expect("scan");
        let names: Vec<&str> = disks.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            ["nvme0n1", "sda", "sdb", "sdc"],
            "loop0 ignoré, tri stable"
        );
        assert_eq!(disks[0].transport, Transport::Nvme);
        assert_eq!(disks[0].partitions, ["nvme0n1p1"]);
        assert_eq!(disks[0].model.as_deref(), Some("Fictional Drive"));
        assert_eq!(disks[0].serial.as_deref(), Some("SN-nvme0n1"));
        assert_eq!(disks[0].size_bytes, 1_953_525_168 * 512);
        assert_eq!((disks[0].major, disks[0].minor), (259, 0));
        assert_eq!(disks[1].transport, Transport::Sata);
        assert_eq!(disks[2].transport, Transport::Usb);
        assert_eq!(disks[3].transport, Transport::Sas);
        assert!(disks[2].transport.behind_satl() && !disks[1].transport.behind_satl());
        assert_eq!(disks[0].solid_state_hint(), Some(true));
        assert_eq!(disks[0].path, f.path("dev/nvme0n1"));
    }

    #[test]
    fn nvme_over_fabrics_is_not_a_local_nvme() {
        let f = Fixture::new();
        f.disk("nvme1n1", "259:3", "pci0000:00/nvme/nvme1");
        let transport = "sys/devices/pci0000:00/nvme/nvme1/block/nvme1n1/device/transport";
        for remote in ["tcp", "rdma", "fc", "loop"] {
            f.write(transport, remote);
            let disks = list_disks(&f.paths()).expect("scan");
            assert_eq!(disks[0].transport, Transport::Other, "{remote}");
        }
    }

    #[test]
    fn vpd_page_80_serial_and_devnum() {
        let page = [
            0x00, 0x80, 0x00, 0x08, b'S', b'N', b'1', b'2', b' ', b' ', b' ', b' ',
        ];
        assert_eq!(vpd_pg80_serial(&page).as_deref(), Some("SN12"));
        assert_eq!(vpd_pg80_serial(&[0, 0x80, 0, 0]), None);
        assert_eq!(parse_devnum("8:16\n"), Some((8, 16)));
        assert_eq!(parse_devnum("x"), None);
    }

    #[test]
    fn human_sizes_look_like_lsblk() {
        assert_eq!(human_size(0), "0B");
        assert_eq!(human_size(512), "512B");
        assert_eq!(human_size(1_953_525_168 * 512), "931.5G");
        assert_eq!(human_size(1 << 40), "1T");
    }

    #[test]
    fn unknown_disk_is_refused_before_touching_sysfs() {
        let f = Fixture::new();
        assert!(matches!(
            read_disk(&f.paths(), "loop0"),
            Err(DeviceError::RefusedName(_))
        ));
        assert!(matches!(
            read_disk(&f.paths(), "sdz"),
            Err(DeviceError::Io { .. })
        ));
    }
}
