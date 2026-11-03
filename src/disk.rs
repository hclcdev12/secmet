//! Modèle disque de l'UI, alimenté par le helper (`ListDisks` puis `ScanDevice`).

use crate::helper_client::{self, InventoryDisk, RecoveryView, ScanResult, VerdictView};
use crate::i18n::{tr, trf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskKind {
    Hdd,
    SsdSata,
    SsdNvme,
    HddUsb,
    SsdUsb,
    /// SSD non rotatif, mais transport inconnu (fallback).
    Ssd,
    /// Média non déterminé (inventaire sans ROTA, inspection firmware impossible).
    Unknown,
}

impl DiskKind {
    pub fn label(&self) -> &'static str {
        match self {
            DiskKind::Hdd => "HDD",
            DiskKind::SsdSata => "SSD_SATA",
            DiskKind::SsdNvme => "SSD_NVME",
            DiskKind::HddUsb => "HDD_USB",
            DiskKind::SsdUsb => "SSD_USB",
            DiskKind::Ssd => "SSD",
            DiskKind::Unknown => "INCONNU",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Disk {
    pub name: String,
    pub path: String,
    /// Taille lisible (`500 GB`).
    pub size: String,
    pub size_bytes: u64,
    pub model: String,
    pub serial: String,
    pub firmware: String,
    /// Transport (`sata`, `nvme`, `usb`, `sas`, `virtio`, vide).
    pub transport: String,
    pub is_ssd: bool,
    pub media_known: bool,
    pub is_read_only: bool,
    pub kind: DiskKind,
    pub is_usb: bool,
    /// SECURITY FREEZE LOCK actif (ATA).
    pub is_frozen: Option<bool>,
    /// ATA Security verrouillé : aucune E/S sans mot de passe.
    pub is_locked: Option<bool>,
    /// Mot de passe ATA Security actif (verrouillé ou non).
    pub security_enabled: Option<bool>,
    pub is_system: bool,
    pub is_mounted: bool,
    /// Résumé des protections (`monté, swap actif`…), vide si libre.
    pub protection_summary: String,
    /// Jeton de session du dernier scan (10 min de validité).
    pub token: Option<String>,
    /// Verdict du moteur au dernier scan.
    pub verdict: Option<VerdictView>,
    pub inspection_failed: bool,
    pub inspection_error: Option<String>,
    pub smart_failing: bool,
    /// Levée automatique d'un mot de passe ATA SecMet lors du dernier scan.
    pub recovery: Option<RecoveryView>,
    /// Contrôleur NVMe exposant plusieurs namespaces (seul celui ouvert
    /// est le disque listé).
    pub nvme_multiple_namespaces: bool,
    /// La racine de l'hôte a-t-elle été rattachée à un disque local ?
    /// `Some(false)` : média live ou ZFS, le disque système n'est protégé
    /// que par le verrou exclusif du noyau. Relevé de l'inventaire (hôte).
    pub root_traced: Option<bool>,
}

fn is_usb_transport(transport: &str) -> bool {
    transport.trim().eq_ignore_ascii_case("usb")
}

/// Classification média × transport.
pub fn classify_disk(name: &str, transport: &str, is_ssd: bool, media_known: bool) -> DiskKind {
    if !media_known {
        return DiskKind::Unknown;
    }
    let tran = transport.trim().to_ascii_lowercase();
    if tran == "usb" {
        return if is_ssd {
            DiskKind::SsdUsb
        } else {
            DiskKind::HddUsb
        };
    }
    if is_ssd && (tran == "nvme" || name.starts_with("nvme")) {
        return DiskKind::SsdNvme;
    }
    if is_ssd && (tran == "sata" || tran == "ata") {
        return DiskKind::SsdSata;
    }
    if is_ssd {
        DiskKind::Ssd
    } else {
        DiskKind::Hdd
    }
}

/// Média d'après l'inventaire : NVMe est toujours flash ; sinon `ROTA` du
/// noyau, jamais derrière un pont USB (les ponts annoncent souvent `1`).
pub fn media_from_inventory(name: &str, transport: &str, rotational: Option<bool>) -> (bool, bool) {
    if name.starts_with("nvme") {
        return (true, true);
    }
    if is_usb_transport(transport) {
        return (false, false);
    }
    match rotational {
        Some(rota) => (!rota, true),
        None => (false, false),
    }
}

impl Disk {
    /// Depuis une entrée d'inventaire (sysfs + protections), avant inspection.
    pub fn from_inventory(entry: &InventoryDisk) -> Self {
        let (is_ssd, media_known) =
            media_from_inventory(&entry.name, &entry.transport, entry.rotational);
        let model = entry
            .model
            .clone()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| "Unknown".to_string());
        Disk {
            name: entry.name.clone(),
            path: entry.path.clone(),
            size: entry.size_label.clone(),
            size_bytes: entry.size_bytes,
            model,
            serial: entry.serial.clone().unwrap_or_default(),
            firmware: entry.firmware.clone().unwrap_or_default(),
            transport: entry.transport.clone(),
            is_ssd,
            media_known,
            is_read_only: entry.read_only,
            kind: classify_disk(&entry.name, &entry.transport, is_ssd, media_known),
            is_usb: is_usb_transport(&entry.transport),
            is_frozen: None,
            is_locked: None,
            security_enabled: None,
            is_system: entry.protection.hosts_system,
            is_mounted: entry.protection.mounted || entry.protection.swap || entry.protection.held,
            protection_summary: if entry.protection.any() {
                entry.protection.summary.clone()
            } else {
                String::new()
            },
            token: None,
            verdict: None,
            inspection_failed: false,
            inspection_error: None,
            smart_failing: false,
            recovery: None,
            nvme_multiple_namespaces: false,
            root_traced: None,
        }
    }

    /// Applique une inspection firmware (`ScanDevice`).
    pub fn apply_scan(&mut self, scan: &ScanResult) {
        let d = &scan.disk;
        if !d.model.trim().is_empty() {
            self.model = d.model.clone();
        }
        if !d.serial.trim().is_empty() {
            self.serial = d.serial.clone();
        }
        if !d.firmware.trim().is_empty() {
            self.firmware = d.firmware.clone();
        }
        if !d.transport.is_empty() {
            self.transport = d.transport.clone();
            self.is_usb = is_usb_transport(&d.transport);
        }
        if d.size_bytes > 0 {
            self.size_bytes = d.size_bytes;
            self.size = d.size_label.clone();
        }
        self.inspection_failed = d.inspection_failed;
        self.inspection_error = d.inspection_error.clone();
        match d.media.as_str() {
            "hdd" => {
                self.is_ssd = false;
                self.media_known = true;
            }
            "ssd" => {
                self.is_ssd = true;
                self.media_known = true;
            }
            _ => {}
        }
        self.kind = classify_disk(&self.name, &self.transport, self.is_ssd, self.media_known);
        if let Some(ata) = &d.ata {
            self.is_frozen = Some(ata.security_frozen);
            self.is_locked = Some(ata.security_locked);
            self.security_enabled = Some(ata.security_enabled);
        }
        if let Some(nvme) = &d.nvme {
            self.is_read_only = self.is_read_only || nvme.read_only;
            self.nvme_multiple_namespaces = nvme.multiple_namespaces;
        }
        self.is_system = d.protection.hosts_system;
        self.is_mounted = d.protection.mounted || d.protection.swap || d.protection.held;
        self.protection_summary = if d.protection.any() {
            d.protection.summary.clone()
        } else {
            String::new()
        };
        self.smart_failing = d.health.smart_failing;
        self.recovery = scan.recovery.clone();
        self.token = Some(scan.token.clone());
        self.verdict = Some(scan.verdict.clone());
    }

    /// Un mot de passe ATA est posé : verrouillé, ou simplement actif.
    pub fn has_ata_password(&self) -> bool {
        self.is_locked == Some(true) || self.security_enabled == Some(true)
    }

    /// Vérifie si le disque peut être effacé en toute sécurité.
    pub fn can_be_shredded(&self) -> (bool, String) {
        if self.is_system {
            return (false, tr("Ce disque est le disque système"));
        }
        if self.is_locked == Some(true) {
            return (false, tr("ATA Security LOCKED (mot de passe requis)"));
        }
        if self.is_mounted {
            let detail = if self.protection_summary.is_empty() {
                tr("monté")
            } else {
                self.protection_summary.clone()
            };
            return (
                false,
                trf(
                    "Ce disque est utilisé par le système ({detail})",
                    &[("detail", &detail)],
                ),
            );
        }
        if self.is_read_only {
            return (false, tr("Disque en lecture seule"));
        }
        if !self.media_known {
            return (
                false,
                tr("Type de disque inconnu (inspection impossible).\n\
                    Rebranchez le disque puis Rafraîchir."),
            );
        }
        if let Some(verdict) = &self.verdict {
            match verdict.state.as_str() {
                "refused" => {
                    return (
                        false,
                        trf(
                            "Refusé par le moteur : {reasons}",
                            &[("reasons", &verdict.reasons.join(", "))],
                        ),
                    );
                }
                "blocked" => {
                    return (
                        false,
                        trf(
                            "Bloqué : {reasons} — consigne : {guidance}",
                            &[
                                ("reasons", &verdict.reasons.join(", ")),
                                ("guidance", &verdict.guidance.join(", ")),
                            ],
                        ),
                    );
                }
                _ => {}
            }
        }
        if self.is_usb {
            return (
                true,
                tr("USB : écriture hôte (Clear), pas de commande firmware"),
            );
        }
        (true, "OK".to_string())
    }

    /// Description de la méthode retenue par le moteur, pour la confirmation.
    pub fn plan_description(&self) -> String {
        match self.verdict.as_ref().and_then(|v| v.plan.as_ref()) {
            Some(plan) => {
                let mut text = format!(
                    "{} (NIST {})",
                    crate::certificate::method_label(&plan.primary.method),
                    plan.target_level
                );
                if !plan.fallbacks.is_empty() {
                    text.push_str(&trf(
                        ", {n} repli(s)",
                        &[("n", &plan.fallbacks.len().to_string())],
                    ));
                }
                if let Some(d) = &plan.degradation {
                    text.push_str(&trf(
                        " — dégradé {from} → {to}",
                        &[("from", &d.from), ("to", &d.to)],
                    ));
                }
                if plan
                    .notes
                    .iter()
                    .any(|n| n == helper_client::NOTE_NVME_OTHER_NAMESPACES)
                {
                    text.push_str(&tr(" — autres espaces de noms du contrôleur non couverts"));
                }
                if !plan.recommend_destroy.is_empty() {
                    text.push_str(&tr(
                        " — destruction physique recommandée après effacement (état du disque)",
                    ));
                }
                text
            }
            None => tr("méthode décidée au lancement"),
        }
    }
}

/// Inventaire via le helper (session déjà ouverte).
/// Inventaire rendu par le helper, avec ce qu'il sait de l'hôte.
#[derive(Debug, Clone, Default)]
pub struct DetectedDisks {
    pub disks: Vec<Disk>,
    /// Voir [`Disk::root_traced`] ; `None` sans rapport de protection.
    pub root_traced: Option<bool>,
}

pub fn detect_disks() -> Result<DetectedDisks, String> {
    let helper = helper_client::current().ok_or_else(|| {
        tr("Session administrateur absente.\n\
            Relancez SecMet et validez le mot de passe au démarrage.")
    })?;
    let inventory = helper_client::list_disks(&helper)?;
    if let Some(note) = &inventory.note {
        eprintln!("[secmet] inventaire : {note}");
    }
    if inventory.root_traced == Some(false) {
        eprintln!("[secmet] inventaire : racine de l'hôte non rattachée à un disque local");
    }
    let disks = inventory
        .disks
        .iter()
        .map(|entry| {
            let mut disk = Disk::from_inventory(entry);
            disk.root_traced = inventory.root_traced;
            disk
        })
        .collect();
    Ok(DetectedDisks {
        disks,
        root_traced: inventory.root_traced,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_client::ProtectionView;

    fn entry(name: &str, transport: &str, rotational: Option<bool>) -> InventoryDisk {
        InventoryDisk {
            name: name.into(),
            path: format!("/dev/{name}"),
            transport: transport.into(),
            size_bytes: 500_000_000_000,
            size_label: "500 GB".into(),
            rotational,
            model: Some("Model".into()),
            ..InventoryDisk::default()
        }
    }

    #[test]
    fn classification_matrix() {
        assert_eq!(classify_disk("sda", "sata", false, true), DiskKind::Hdd);
        assert_eq!(classify_disk("sda", "sata", true, true), DiskKind::SsdSata);
        assert_eq!(classify_disk("sda", "ata", true, true), DiskKind::SsdSata);
        assert_eq!(
            classify_disk("nvme0n1", "nvme", true, true),
            DiskKind::SsdNvme
        );
        assert_eq!(classify_disk("nvme1n1", "", true, true), DiskKind::SsdNvme);
        assert_eq!(classify_disk("sda", "usb", false, true), DiskKind::HddUsb);
        assert_eq!(classify_disk("sda", "usb", true, true), DiskKind::SsdUsb);
        assert_eq!(classify_disk("vda", "virtio", true, true), DiskKind::Ssd);
        assert_eq!(
            classify_disk("sda", "sata", false, false),
            DiskKind::Unknown
        );
    }

    #[test]
    fn media_from_inventory_trusts_rota_except_over_usb() {
        assert_eq!(media_from_inventory("nvme0n1", "nvme", None), (true, true));
        assert_eq!(
            media_from_inventory("sda", "sata", Some(false)),
            (true, true)
        );
        assert_eq!(
            media_from_inventory("sda", "sata", Some(true)),
            (false, true)
        );
        assert_eq!(
            media_from_inventory("sdb", "usb", Some(true)),
            (false, false)
        );
        assert_eq!(media_from_inventory("sdb", "sas", None), (false, false));
    }

    #[test]
    fn inventory_then_scan_updates_the_model() {
        let mut disk = Disk::from_inventory(&entry("sdb", "sata", Some(false)));
        assert_eq!(disk.kind, DiskKind::SsdSata);
        assert!(disk.can_be_shredded().0);
        let scan: ScanResult = serde_json::from_value(serde_json::json!({
            "token": "t",
            "identity": {"name": "sdb", "major": 8, "minor": 16, "serial": "SN", "fingerprint": "f"},
            "disk": {"name": "sdb", "model": "Samsung", "serial": "SN", "firmware": "FW", "transport": "sata",
                     "media": "ssd", "size_bytes": 1, "size_label": "1 B",
                     "protection": {"hosts_system": false, "mounted": true, "swap": false, "held": false, "summary": "monté"},
                     "ata": {"security_frozen": true, "security_locked": false},
                     "health": {"smart_failing": true, "nvme_media_errors": "0"}},
            "verdict": {"state": "refused", "reasons": ["Mounted"]}
        }))
        .unwrap();
        disk.apply_scan(&scan);
        assert_eq!(disk.model, "Samsung");
        assert_eq!(disk.serial, "SN");
        assert_eq!(disk.is_frozen, Some(true));
        assert!(disk.smart_failing);
        assert_eq!(disk.token.as_deref(), Some("t"));
        let (ok, reason) = disk.can_be_shredded();
        assert!(!ok);
        assert!(reason.contains("monté"), "{reason}");
    }

    #[test]
    fn refusals_in_order() {
        let mut disk = Disk::from_inventory(&entry("sdc", "usb", Some(true)));
        assert!(!disk.can_be_shredded().0, "média inconnu derrière USB");
        disk.media_known = true;
        disk.kind = DiskKind::HddUsb;
        let (ok, reason) = disk.can_be_shredded();
        assert!(ok && reason.contains("USB"));
        disk.is_locked = Some(true);
        assert!(disk.can_be_shredded().1.contains("LOCKED"));
        disk.is_locked = None;
        disk.is_system = true;
        assert!(disk.can_be_shredded().1.contains("système"));
        let mut blocked = Disk::from_inventory(&InventoryDisk {
            protection: ProtectionView {
                held: true,
                summary: "membre LVM/RAID/dm".into(),
                ..ProtectionView::default()
            },
            ..entry("sdd", "sata", Some(true))
        });
        assert!(blocked.can_be_shredded().1.contains("LVM"));
        blocked.is_mounted = false;
        blocked.verdict = Some(VerdictView {
            state: "blocked".into(),
            reasons: vec!["SecurityLocked".into()],
            guidance: vec!["UnlockWithPassword".into()],
            plan: None,
        });
        assert!(blocked.can_be_shredded().1.contains("Bloqué"));
    }
}
