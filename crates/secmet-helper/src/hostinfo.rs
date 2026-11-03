//! Inventaire du banc depuis sysfs et procfs (remplace `dmidecode` et `lshw`).
//! Les numéros de série DMI ne sont lisibles que par root : c'est pourquoi
//! le helper les fournit à l'UI.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Inventaire du poste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HostInfo {
    /// Nom d'hôte.
    pub hostname: String,
    /// Constructeur (DMI `sys_vendor`).
    pub manufacturer: String,
    /// Modèle (DMI `product_name`).
    pub model: String,
    /// Numéro de série (DMI `product_serial`).
    pub serial: String,
    /// Carte mère (`board_vendor` + `board_name`).
    pub board: String,
    /// Firmware (`bios_vendor` + `bios_version` + `bios_date`).
    pub bios: String,
    /// Processeur (`/proc/cpuinfo`).
    pub cpu: String,
    /// Mémoire physique en octets (`/proc/meminfo`).
    pub memory_bytes: u64,
    /// Noyau (`/proc/sys/kernel/osrelease`).
    pub kernel: String,
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn join(parts: &[String]) -> String {
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Modèle de processeur dans un `/proc/cpuinfo`.
pub fn cpu_model(cpuinfo: &str) -> String {
    cpuinfo
        .lines()
        .find_map(|l| {
            let (key, value) = l.split_once(':')?;
            (key.trim() == "model name").then(|| value.trim().to_string())
        })
        .unwrap_or_default()
}

/// Mémoire totale d'un `/proc/meminfo`, en octets.
pub fn memory_total(meminfo: &str) -> u64 {
    meminfo
        .lines()
        .find_map(|l| {
            let value = l.strip_prefix("MemTotal:")?;
            value.trim().split(' ').next()?.parse::<u64>().ok()
        })
        .map(|kib| kib * 1024)
        .unwrap_or(0)
}

/// Collecte avec les racines `sys` et `proc` données.
pub fn collect(sys: &Path, proc: &Path) -> HostInfo {
    let dmi = sys.join("class/dmi/id");
    HostInfo {
        hostname: read(&proc.join("sys/kernel/hostname")),
        manufacturer: read(&dmi.join("sys_vendor")),
        model: read(&dmi.join("product_name")),
        serial: read(&dmi.join("product_serial")),
        board: join(&[
            read(&dmi.join("board_vendor")),
            read(&dmi.join("board_name")),
        ]),
        bios: join(&[
            read(&dmi.join("bios_vendor")),
            read(&dmi.join("bios_version")),
            read(&dmi.join("bios_date")),
        ]),
        cpu: cpu_model(&read(&proc.join("cpuinfo"))),
        memory_bytes: memory_total(&read(&proc.join("meminfo"))),
        kernel: read(&proc.join("sys/kernel/osrelease")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn collects_from_a_fake_sysfs_and_procfs() {
        let tmp = tempfile::tempdir().unwrap();
        let dmi = tmp.path().join("sys/class/dmi/id");
        fs::create_dir_all(&dmi).unwrap();
        for (name, value) in [
            ("sys_vendor", "Dell Inc.\n"),
            ("product_name", "OptiPlex 7090\n"),
            ("product_serial", "ABC1234\n"),
            ("board_vendor", "Dell Inc.\n"),
            ("board_name", "0XYZ\n"),
            ("bios_vendor", "Dell Inc.\n"),
            ("bios_version", "1.2.3\n"),
            ("bios_date", "01/02/2026\n"),
        ] {
            fs::write(dmi.join(name), value).unwrap();
        }
        let proc = tmp.path().join("proc");
        fs::create_dir_all(proc.join("sys/kernel")).unwrap();
        fs::write(proc.join("sys/kernel/hostname"), "banc-1\n").unwrap();
        fs::write(proc.join("sys/kernel/osrelease"), "7.2.5\n").unwrap();
        fs::write(
            proc.join("cpuinfo"),
            "processor\t: 0\nmodel name\t: Intel(R) Core(TM) i5\n",
        )
        .unwrap();
        fs::write(
            proc.join("meminfo"),
            "MemTotal:       16384000 kB\nMemFree: 1 kB\n",
        )
        .unwrap();
        let info = collect(&tmp.path().join("sys"), &proc);
        assert_eq!(info.manufacturer, "Dell Inc.");
        assert_eq!(info.model, "OptiPlex 7090");
        assert_eq!(info.serial, "ABC1234");
        assert_eq!(info.board, "Dell Inc. 0XYZ");
        assert_eq!(info.bios, "Dell Inc. 1.2.3 01/02/2026");
        assert_eq!(info.cpu, "Intel(R) Core(TM) i5");
        assert_eq!(info.memory_bytes, 16_384_000 * 1024);
        assert_eq!(info.hostname, "banc-1");
        assert_eq!(info.kernel, "7.2.5");
        // Fichiers absents : chaînes vides, jamais d'erreur.
        let empty = collect(Path::new("/nonexistent"), Path::new("/nonexistent"));
        assert_eq!(empty, HostInfo::default());
    }
}
