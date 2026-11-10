//! Disques candidats et règles d'admissibilité : seuls les disques
//! amovibles assez grands, sans partition système montée, sont proposés.

use crate::platform;

/// Taille minimale d'une clé : l'image fait 11 Gio, la partition d'export
/// s'agrandit ensuite jusqu'au bout.
pub const MIN_SIZE: u64 = 12_000_000_000;

/// Un disque entier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    /// Chemin système (`/dev/sdb`, `\\.\PhysicalDrive2`).
    pub path: String,
    /// Constructeur et modèle.
    pub name: String,
    /// Numéro de série, s'il est connu.
    pub serial: Option<String>,
    /// Taille en octets.
    pub size: u64,
    /// Amovible ou branché à chaud (USB, lecteur de cartes).
    pub removable: bool,
    /// Bus (`usb`, `sata`, `nvme`…).
    pub transport: Option<String>,
    /// Points de montage de ses partitions.
    pub mounted: Vec<String>,
}

impl Disk {
    /// Raison pour laquelle le disque ne peut pas recevoir l'image, sinon
    /// `None`.
    pub fn refusal(&self, needed: u64) -> Option<String> {
        if !self.removable {
            return Some("disque interne".to_string());
        }
        if self.has_system_mount() {
            return Some("porte le système en cours".to_string());
        }
        if self.size < needed.max(MIN_SIZE) {
            return Some(format!(
                "trop petit ({} ; {} requis)",
                crate::fmt_size_decimal(self.size),
                crate::fmt_size_decimal(needed.max(MIN_SIZE))
            ));
        }
        None
    }

    fn has_system_mount(&self) -> bool {
        self.mounted.iter().any(|m| {
            m == "/"
                || m == "/home"
                || m == "/usr"
                || m == "/var"
                || m.starts_with("/boot")
                || m == "[SWAP]"
                || m.starts_with("C:")
        })
    }

    /// Ligne de description pour `--list-disks` et le journal.
    pub fn describe(&self) -> String {
        format!(
            "{}  {}  {}  série {}  {}{}",
            self.path,
            self.name,
            crate::fmt_size_decimal(self.size),
            self.serial.as_deref().unwrap_or("?"),
            self.transport.as_deref().unwrap_or("?"),
            if self.mounted.is_empty() {
                String::new()
            } else {
                format!("  monté : {}", self.mounted.join(", "))
            }
        )
    }
}

/// Tous les disques entiers, triés par chemin ; le filtrage est laissé à
/// l'interface pour expliquer chaque refus.
pub fn list() -> Result<Vec<Disk>, String> {
    let mut d = platform::list_disks()?;
    d.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usb(size: u64) -> Disk {
        Disk {
            path: "/dev/sdb".into(),
            name: "SanDisk Ultra".into(),
            serial: None,
            size,
            removable: true,
            transport: Some("usb".into()),
            mounted: vec![],
        }
    }

    #[test]
    fn refuse_internal_small_or_system() {
        let mut d = usb(16_000_000_000);
        assert_eq!(d.refusal(11_814_305_792), None);
        d.size = 8_000_000_000;
        assert!(d.refusal(11_814_305_792).unwrap().starts_with("trop petit"));
        d.size = 64_000_000_000;
        d.mounted = vec!["/".into()];
        assert_eq!(d.refusal(1).as_deref(), Some("porte le système en cours"));
        d.mounted.clear();
        d.removable = false;
        assert_eq!(d.refusal(1).as_deref(), Some("disque interne"));
    }
}
