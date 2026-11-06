//! Nettoyage de la clé USB (Annexe M) : retire de la partition de données
//! et de la partition d'export ce que les lots et les effacements y ont
//! laissé, au choix de l'opérateur. Module pur (pas de GTK), sur des
//! chemins explicites pour être testable ; l'interface passe ceux du
//! [`crate::paths`] courant.
//!
//! Jamais touchés : `config/` (réglages, clé d'attestation), `app/`
//! (mises à jour), `vault/` (mots de passe ATA, côté root), `logs/system`
//! (journaux de démarrage, écrits par le système et plafonnés à 20) et,
//! sur l'export, tout ce qui n'est pas un dossier de lot (`L-…`) ou de
//! session (`SEC-…`).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::lot::{Lot, LotStatus, LOT_FILE};

/// Préfixes des dossiers que SecMet crée sur la partition d'export.
const EXPORT_PREFIXES: [&str; 2] = ["L-", "SEC-"];
/// Journal du nettoyage, sous `logs/`.
pub const CLEANUP_LOG: &str = "cleanup.log";

/// Emplacements concernés.
#[derive(Debug, Clone)]
pub struct Places {
    /// `archive/` : lots clôturés ou abandonnés.
    pub archive: PathBuf,
    /// `logs/` : certificats hors lot, rapports et journaux de session.
    pub logs: PathBuf,
    /// Partition d'export montée, si présente.
    pub export: Option<PathBuf>,
}

/// Ce que le nettoyage trouverait.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    /// Lots archivés, par identifiant.
    pub archived: Vec<String>,
    /// Lots clôturés dont le manifeste manque sur l'export : l'archive est
    /// alors leur seule copie signée, et l'interface refuse de la supprimer.
    pub not_exported: Vec<String>,
    /// Dossiers de livrables sur l'export.
    pub exports: Vec<String>,
    /// Fichiers hors lot sous `logs/` (certificats, rapports, sessions).
    pub standalone_files: usize,
    /// Octets concernés, tous groupes confondus.
    pub bytes: u64,
}

/// Groupes retenus par l'opérateur.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Choice {
    /// Lots archivés.
    pub archived: bool,
    /// Livrables sur l'export.
    pub exports: bool,
    /// Certificats hors lot, rapports et journaux de session.
    pub standalone: bool,
}

impl Choice {
    /// Aucun groupe coché.
    pub fn is_empty(self) -> bool {
        !(self.archived || self.exports || self.standalone)
    }
}

/// Résultat d'un nettoyage.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Dossiers et fichiers retirés.
    pub removed: usize,
    /// Octets libérés.
    pub bytes: u64,
    /// Échecs : `(chemin, cause)`.
    pub errors: Vec<(String, String)>,
}

/// Chemins hors lot sous `logs/` : `certificates/`, `sessions/` et les
/// rapports `session_report_*.json`.
fn standalone_paths(logs: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for sub in ["certificates", "sessions"] {
        let p = logs.join(sub);
        if p.is_dir() {
            paths.push(p);
        }
    }
    if let Ok(entries) = fs::read_dir(logs) {
        for e in entries.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("session_report_") && name.ends_with(".json") {
                paths.push(e.path());
            }
        }
    }
    paths.sort();
    paths
}

/// Dossiers de lot ou de session sur l'export, par nom.
fn export_dirs(export: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(export) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| EXPORT_PREFIXES.iter().any(|p| n.starts_with(p)))
        .collect();
    names.sort();
    names
}

/// Lots archivés, triés par identifiant.
fn archived_lots(archive: &Path) -> Vec<Lot> {
    let Ok(entries) = fs::read_dir(archive) else {
        return Vec::new();
    };
    let mut lots: Vec<Lot> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join(LOT_FILE).is_file())
        .filter_map(|e| Lot::load(&e.path()).ok())
        .collect();
    lots.sort_by(|a, b| a.id().cmp(b.id()));
    lots
}

/// Taille d'un fichier ou d'une arborescence (0 si illisible).
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_dir() {
        fs::read_dir(path)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| size_of(&e.path())).sum())
            .unwrap_or(0)
    } else {
        meta.len()
    }
}

/// Fait l'inventaire sans rien modifier.
pub fn inventory(places: &Places) -> Inventory {
    let mut inv = Inventory::default();
    for lot in archived_lots(&places.archive) {
        let id = lot.id().to_string();
        let exported = places.export.as_ref().is_some_and(|mount| {
            mount
                .join(&id)
                .join(format!("{id}-manifest.json"))
                .is_file()
        });
        if lot.state().status == LotStatus::Closed && !exported {
            inv.not_exported.push(id.clone());
        }
        inv.bytes += size_of(lot.dir());
        inv.archived.push(id);
    }
    if let Some(mount) = &places.export {
        for name in export_dirs(mount) {
            inv.bytes += size_of(&mount.join(&name));
            inv.exports.push(name);
        }
    }
    for p in standalone_paths(&places.logs) {
        inv.bytes += size_of(&p);
        inv.standalone_files += 1;
    }
    inv
}

fn remove(path: &Path, report: &mut Report) {
    let bytes = size_of(path);
    let result = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    match result {
        Ok(()) => {
            report.removed += 1;
            report.bytes += bytes;
        }
        Err(e) => report
            .errors
            .push((path.display().to_string(), e.to_string())),
    }
}

/// Supprime les groupes cochés, puis note le bilan dans `logs/cleanup.log`
/// (une ligne horodatée : groupes, éléments retirés, octets, erreurs).
pub fn clean(places: &Places, choice: Choice) -> Report {
    let mut report = Report::default();
    if choice.archived {
        for lot in archived_lots(&places.archive) {
            remove(lot.dir(), &mut report);
        }
    }
    if choice.exports {
        if let Some(mount) = &places.export {
            for name in export_dirs(mount) {
                remove(&mount.join(name), &mut report);
            }
        }
    }
    if choice.standalone {
        for p in standalone_paths(&places.logs) {
            remove(&p, &mut report);
        }
    }
    let mut groups = Vec::new();
    if choice.archived {
        groups.push("archive");
    }
    if choice.exports {
        groups.push("export");
    }
    if choice.standalone {
        groups.push("hors-lot");
    }
    let line = format!(
        "{} nettoyage [{}] : {} élément(s), {} octets, {} erreur(s){}\n",
        crate::clock::stamp(),
        groups.join(","),
        report.removed,
        report.bytes,
        report.errors.len(),
        report
            .errors
            .iter()
            .map(|(p, e)| format!(" ; {p} : {e}"))
            .collect::<String>()
    );
    let _ = fs::create_dir_all(&places.logs);
    if let Ok(mut f) = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(places.logs.join(CLEANUP_LOG))
    {
        let _ = f.write_all(line.as_bytes());
    }
    report
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::lot::LotScope;

    fn places(tmp: &Path) -> Places {
        let p = Places {
            archive: tmp.join("archive"),
            logs: tmp.join("logs"),
            export: Some(tmp.join("export")),
        };
        fs::create_dir_all(&p.archive).unwrap();
        fs::create_dir_all(&p.logs).unwrap();
        fs::create_dir_all(p.export.as_ref().unwrap()).unwrap();
        p
    }

    fn archived(root: &Path, archive: &Path, close: bool) -> String {
        let mut lot = Lot::open(root, "Jean", "", "secmet 0.3.9", LotScope::Machines).unwrap();
        fs::write(lot.certificates_dir().join("x.pdf"), b"%PDF").unwrap();
        if close {
            fs::write(lot.manifest_path(), b"{}").unwrap();
            lot.close("00").unwrap();
        } else {
            lot.abandon().unwrap();
        }
        let id = lot.id().to_string();
        lot.archive(archive).unwrap();
        id
    }

    #[test]
    fn inventory_then_clean_respects_the_choice_and_spares_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let p = places(tmp.path());
        let root = tmp.path().join("lots");
        let export = p.export.clone().unwrap();

        let exported = archived(&root, &p.archive, true);
        let orphan = archived(&root, &p.archive, true);
        let dropped = archived(&root, &p.archive, false);
        fs::create_dir_all(export.join(&exported)).unwrap();
        fs::write(
            export
                .join(&exported)
                .join(format!("{exported}-manifest.json")),
            b"{}",
        )
        .unwrap();
        fs::create_dir_all(export.join("SEC-20261105-AB12")).unwrap();
        fs::write(export.join("SEC-20261105-AB12").join("a.pdf"), b"%PDF").unwrap();
        fs::create_dir_all(export.join("_live")).unwrap();
        fs::write(export.join("notes.txt"), b"client").unwrap();
        fs::create_dir_all(p.logs.join("certificates")).unwrap();
        fs::write(p.logs.join("certificates").join("c.json"), b"{}").unwrap();
        fs::write(p.logs.join("session_report_20261105_1.json"), b"{}").unwrap();
        fs::create_dir_all(p.logs.join("system")).unwrap();
        fs::write(p.logs.join("system").join("boot.log"), b"journal").unwrap();

        let inv = inventory(&p);
        assert_eq!(inv.archived.len(), 3);
        assert_eq!(inv.not_exported, vec![orphan.clone()], "abandonné exclu");
        assert_eq!(
            inv.exports,
            vec![exported.clone(), "SEC-20261105-AB12".to_string()]
        );
        assert_eq!(inv.standalone_files, 2);
        assert!(inv.bytes > 0);

        // Archive seule : l'export et le hors-lot restent.
        let r = clean(
            &p,
            Choice {
                archived: true,
                ..Choice::default()
            },
        );
        assert_eq!(r.removed, 3);
        assert!(r.errors.is_empty());
        assert!(!p.archive.join(&dropped).exists());
        assert!(export.join(&exported).exists());
        assert!(p.logs.join("certificates").exists());

        let r = clean(
            &p,
            Choice {
                exports: true,
                standalone: true,
                ..Choice::default()
            },
        );
        assert_eq!(r.removed, 4);
        assert!(export.join("_live").exists(), "zone hors SecMet préservée");
        assert!(export.join("notes.txt").exists());
        assert!(p.logs.join("system").join("boot.log").exists());
        assert_eq!(inventory(&p), Inventory::default());
        let log = fs::read_to_string(p.logs.join(CLEANUP_LOG)).unwrap();
        assert_eq!(log.lines().count(), 2);
        assert!(log.contains("[export,hors-lot] : 4 élément(s)"));
    }

    #[test]
    fn missing_places_are_harmless() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Places {
            archive: tmp.path().join("nope"),
            logs: tmp.path().join("logs"),
            export: None,
        };
        assert_eq!(inventory(&p), Inventory::default());
        let r = clean(
            &p,
            Choice {
                archived: true,
                exports: true,
                standalone: true,
            },
        );
        assert_eq!(r, Report::default());
    }
}
