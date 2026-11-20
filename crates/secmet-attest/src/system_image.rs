//! Auto-contrôle du système de la clé USB (Annexe M §M.3).
//!
//! À la racine de la partition système `SECMET-SYS`, `SYSTEM.sums` donne
//! l'empreinte SHA-256 de chaque fichier du système (noyau, initrd,
//! squashfs, chargeurs EFI, `grub.cfg`, modules de GRUB BIOS), au format de
//! `sha256sum` avec des chemins relatifs à la partition ; `SYSTEM.sums.sig`
//! est sa signature par la **clé de version**, posée par la CI au moment de
//! publier (même mécanisme que `SHA256SUMS`, étiquette `system`). Au
//! démarrage, `secmet-image-check` (service de la clé) appelle
//! `secmet-release verify-system`, qui produit un [`IntegrityReport`] lu par
//! l'application : réserve sur les certificats si l'image n'est pas celle
//! qui a été publiée.
//!
//! **Limite** : le vérificateur fait partie du système qu'il vérifie. Il
//! détecte la corruption et l'altération naïve (fichier de démarrage
//! modifié depuis Windows), pas un attaquant qui modifie aussi le
//! vérificateur.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::release::{self, SumEntry, SumsSignature};
use crate::sign::{self, SigningIdentity, Verified, ALGORITHM};
use crate::{hex, sha256_file_hex, sha256_hex, AttestError, Result};

/// Empreintes du système, à la racine de `SECMET-SYS`.
pub const SYSTEM_SUMS_FILE: &str = "SYSTEM.sums";
/// Signature détachée de [`SYSTEM_SUMS_FILE`] (même format que
/// `SHA256SUMS.sig`).
pub const SYSTEM_SIGNATURE_FILE: &str = "SYSTEM.sums.sig";
/// Dossiers où tout fichier doit être listé : un fichier ajouté sous
/// `EFI/` (chargeur) ou `live/` (noyau, squashfs) est un écart. Ailleurs
/// (`boot/grub/i386-pc`, où `grub-install` ajoute `core.img`), seuls les
/// fichiers listés sont contrôlés.
pub const STRICT_DIRS: [&str; 2] = ["live", "EFI"];
/// Écarts rapportés au plus (les suivants sont comptés).
const MAX_PROBLEMS: usize = 40;
const LABEL: &str = "system";

/// Chemin relatif sûr : composants ordinaires seulement (ni `..`, ni `.`,
/// ni racine), séparateur `/`, pas d'antislash ni de caractère de contrôle.
pub fn valid_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// Analyse `SYSTEM.sums` : `<hex>  <chemin relatif>` par ligne, lignes
/// vides et commentaires `#` ignorés ; un chemin ne figure qu'une fois.
pub fn parse_system_sums(text: &str) -> Result<Vec<SumEntry>> {
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = || AttestError::Invalid(format!("{SYSTEM_SUMS_FILE} ligne {} : format", n + 1));
        let (sum, rest) = line.split_once(' ').ok_or_else(bad)?;
        let name = rest.trim_start().trim_start_matches('*');
        if sum.len() != 64
            || !sum.bytes().all(|b| b.is_ascii_hexdigit())
            || !valid_relative_path(name)
            || !seen.insert(name.to_ascii_lowercase())
        {
            return Err(bad());
        }
        entries.push(SumEntry {
            sha256: sum.to_ascii_lowercase(),
            name: name.to_string(),
        });
    }
    if entries.is_empty() {
        return Err(AttestError::Invalid(format!("{SYSTEM_SUMS_FILE} vide")));
    }
    Ok(entries)
}

/// Première ligne de commentaire de `SYSTEM.sums` (système de la clé, par
/// exemple « secmet 0.5.0 … »), vide s'il n'y en a pas.
pub fn system_label(text: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix('#'))
        .map(|l| l.trim().chars().take(200).collect())
        .unwrap_or_default()
}

/// Signe le texte de `SYSTEM.sums` avec la clé de version.
pub fn sign_system(identity: &SigningIdentity, sums_text: &str) -> SumsSignature {
    let signature = identity.sign_raw(&release::message(LABEL, sums_text.as_bytes()));
    SumsSignature {
        alg: ALGORITHM.to_string(),
        key_id: identity.key_id(),
        public_key: identity.public_key_hex(),
        sha256: sha256_hex(sums_text.as_bytes()),
        signature: hex(&signature),
    }
}

/// Vérifie la signature de `SYSTEM.sums` ; `trusted_keys` comme pour
/// [`sign::verify`].
pub fn verify_system_signature(
    sig: &SumsSignature,
    sums_text: &str,
    trusted_keys: &[String],
) -> Result<Verified> {
    if sig.sha256 != sha256_hex(sums_text.as_bytes()) {
        return Err(AttestError::Signature(format!(
            "{SYSTEM_SUMS_FILE} ne correspond pas à sa signature"
        )));
    }
    sign::verify_raw(
        &sig.alg,
        &sig.key_id,
        &sig.public_key,
        &sig.signature,
        &release::message(LABEL, sums_text.as_bytes()),
    )?;
    Ok(Verified {
        key_id: sig.key_id.clone(),
        trusted: sign::is_trusted(trusted_keys, &sig.key_id, &sig.public_key),
    })
}

/// Fichiers ordinaires sous `dir` (relatifs à `root`, séparateur `/`).
fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            walk(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            let rel: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(rel.join("/"));
        }
    }
}

/// Écarts entre l'arborescence `root` et les entrées : fichier absent, qui
/// n'est pas un fichier ordinaire, d'empreinte différente, ou fichier non
/// listé dans un dossier de [`STRICT_DIRS`]. Les noms sont comparés sans
/// tenir compte de la casse (FAT). Empreintes calculées par flux.
pub fn check_files(root: &Path, entries: &[SumEntry]) -> Vec<String> {
    let mut problems = Vec::new();
    for e in entries {
        let path = root.join(&e.name);
        match fs::symlink_metadata(&path) {
            Err(_) => problems.push(format!("{} : absent", e.name)),
            Ok(meta) if !meta.is_file() => {
                problems.push(format!("{} : pas un fichier ordinaire", e.name))
            }
            Ok(_) => match sha256_file_hex(&path) {
                Ok(sum) if sum == e.sha256 => {}
                Ok(_) => problems.push(format!("{} : empreinte différente", e.name)),
                Err(err) => problems.push(format!("{} : illisible ({err})", e.name)),
            },
        }
    }
    let listed: BTreeSet<String> = entries
        .iter()
        .map(|e| e.name.to_ascii_lowercase())
        .collect();
    let Ok(top) = fs::read_dir(root) else {
        return problems;
    };
    let mut extra = Vec::new();
    for entry in top.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if STRICT_DIRS.iter().any(|d| d.eq_ignore_ascii_case(&name)) {
            walk(root, &entry.path(), &mut extra);
        }
    }
    extra.sort();
    for name in extra {
        if !listed.contains(&name.to_ascii_lowercase()) {
            problems.push(format!("{name} : fichier inattendu"));
        }
    }
    problems
}

/// Résultat de l'auto-contrôle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntegrityStatus {
    /// Empreintes conformes, signées par une clé de version de confiance.
    Ok,
    /// Fichier modifié, absent ou ajouté, ou signature invalide ou
    /// inconnue : l'image n'est pas celle qui a été publiée.
    Altered,
    /// Pas d'empreintes signées (clé d'avant la 0.5.0, construction locale
    /// ou candidate `-rc`) : rien n'est contredit, rien n'est prouvé.
    Unsigned,
    /// Partition système introuvable (clé retirée après un démarrage
    /// `toram`).
    Unavailable,
    /// Contrôle impossible (erreur de lecture, outil absent).
    Error,
    /// Contrôle en cours.
    Pending,
}

/// Rapport écrit par `secmet-release verify-system --json`, lu par
/// l'application. Champs inconnus tolérés : le script de la clé et
/// l'application peuvent être de versions différentes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityReport {
    /// Verdict.
    pub status: IntegrityStatus,
    /// Système annoncé par `SYSTEM.sums` (première ligne de commentaire).
    #[serde(default)]
    pub system: String,
    /// Clé de version qui a signé, si la signature est valide.
    #[serde(default)]
    pub key_id: Option<String>,
    /// Nombre de fichiers listés.
    #[serde(default)]
    pub files: usize,
    /// Écarts constatés, lisibles.
    #[serde(default)]
    pub problems: Vec<String>,
}

impl IntegrityReport {
    fn new(status: IntegrityStatus, problem: impl Into<String>) -> Self {
        Self {
            status,
            system: String::new(),
            key_id: None,
            files: 0,
            problems: vec![problem.into()],
        }
    }
}

/// Contrôle de la partition système montée en `root` : `SYSTEM.sums`,
/// empreintes, fichiers en trop dans [`STRICT_DIRS`], puis signature avec
/// `trusted_keys` (clés de version acceptées).
pub fn check_tree(root: &Path, trusted_keys: &[String]) -> IntegrityReport {
    let sums_text = match fs::read_to_string(root.join(SYSTEM_SUMS_FILE)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return IntegrityReport::new(
                IntegrityStatus::Unsigned,
                format!("{SYSTEM_SUMS_FILE} absent : image sans empreintes du système"),
            )
        }
        Err(e) => {
            return IntegrityReport::new(
                IntegrityStatus::Error,
                format!("{SYSTEM_SUMS_FILE} : {e}"),
            )
        }
    };
    let entries = match parse_system_sums(&sums_text) {
        Ok(entries) => entries,
        Err(e) => return IntegrityReport::new(IntegrityStatus::Altered, e.to_string()),
    };
    let mut problems = check_files(root, &entries);
    let mut key_id = None;
    let mut signed = false;
    match fs::read_to_string(root.join(SYSTEM_SIGNATURE_FILE)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => problems.push(format!("{SYSTEM_SIGNATURE_FILE} : {e}")),
        Ok(text) => match serde_json::from_str::<SumsSignature>(&text) {
            Err(e) => problems.push(format!("{SYSTEM_SIGNATURE_FILE} illisible : {e}")),
            Ok(sig) => match verify_system_signature(&sig, &sums_text, trusted_keys) {
                Err(e) => problems.push(format!("{SYSTEM_SIGNATURE_FILE} : {e}")),
                Ok(v) if !v.trusted => problems.push(format!(
                    "{SYSTEM_SIGNATURE_FILE} : signée par une clé de version inconnue ({})",
                    v.key_id
                )),
                Ok(v) => {
                    key_id = Some(v.key_id);
                    signed = true;
                }
            },
        },
    }
    let status = if !problems.is_empty() {
        IntegrityStatus::Altered
    } else if signed {
        IntegrityStatus::Ok
    } else {
        problems.push(format!(
            "{SYSTEM_SIGNATURE_FILE} absent : empreintes conformes mais non signées"
        ));
        IntegrityStatus::Unsigned
    };
    if problems.len() > MAX_PROBLEMS {
        let more = problems.len() - MAX_PROBLEMS;
        problems.truncate(MAX_PROBLEMS);
        problems.push(format!("… et {more} autres écarts"));
    }
    IntegrityReport {
        status,
        system: system_label(&sums_text),
        key_id,
        files: entries.len(),
        problems,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn identity() -> SigningIdentity {
        SigningIdentity::from_seed(&[7u8; 32])
    }

    /// Partition système d'essai : fichiers, `SYSTEM.sums` comme le produit
    /// `system-sums.sh`, signé si `sign`.
    fn tree(sign: bool) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let files = [
            ("live/vmlinuz", "noyau"),
            ("live/initrd.img", "initrd"),
            ("live/filesystem.squashfs", "squashfs"),
            ("EFI/BOOT/BOOTX64.EFI", "shim"),
            ("EFI/BOOT/grubx64.efi", "grub"),
            ("EFI/BOOT/grub.cfg", "cfg"),
            ("boot/grub/grub.cfg", "cfg"),
            ("boot/grub/i386-pc/normal.mod", "mod"),
        ];
        let mut text = String::from("# secmet 0.5.0~debian13 (2026-11-20)\n");
        for (name, content) in files {
            let path = dir.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
            text.push_str(&format!("{}  {name}\n", sha256_hex(content.as_bytes())));
        }
        fs::write(dir.path().join(SYSTEM_SUMS_FILE), &text).unwrap();
        if sign {
            let sig = sign_system(&identity(), &text);
            fs::write(
                dir.path().join(SYSTEM_SIGNATURE_FILE),
                serde_json::to_string_pretty(&sig).unwrap(),
            )
            .unwrap();
        }
        (dir, text)
    }

    fn trusted() -> Vec<String> {
        vec![identity().public_key_hex()]
    }

    #[test]
    fn sealed_tree_is_ok() {
        let (dir, _) = tree(true);
        let r = check_tree(dir.path(), &trusted());
        assert_eq!(r.status, IntegrityStatus::Ok, "{:?}", r.problems);
        assert_eq!(r.files, 8);
        assert_eq!(r.system, "secmet 0.5.0~debian13 (2026-11-20)");
        assert_eq!(r.key_id, Some(identity().key_id()));
        assert!(r.problems.is_empty());
        // Fichier non listé hors des dossiers stricts (core.img de
        // grub-install) ou à la racine (System Volume Information) : toléré.
        fs::write(dir.path().join("boot/grub/i386-pc/core.img"), "core").unwrap();
        fs::create_dir_all(dir.path().join("System Volume Information")).unwrap();
        fs::write(dir.path().join("System Volume Information/x"), "w").unwrap();
        assert_eq!(
            check_tree(dir.path(), &trusted()).status,
            IntegrityStatus::Ok
        );
    }

    #[test]
    fn modified_missing_or_added_files_are_altered() {
        let (dir, _) = tree(true);
        fs::write(dir.path().join("boot/grub/grub.cfg"), "cfg modifié").unwrap();
        fs::remove_file(dir.path().join("live/initrd.img")).unwrap();
        fs::write(dir.path().join("EFI/BOOT/fbx64.efi"), "ajout").unwrap();
        let r = check_tree(dir.path(), &trusted());
        assert_eq!(r.status, IntegrityStatus::Altered);
        assert_eq!(
            r.problems,
            [
                "live/initrd.img : absent",
                "boot/grub/grub.cfg : empreinte différente",
                "EFI/BOOT/fbx64.efi : fichier inattendu",
            ]
        );
    }

    #[test]
    fn names_are_compared_without_case() {
        let (dir, text) = tree(true);
        // Liste en minuscules, fichiers en majuscules (montage vfat) : rien
        // d'inattendu.
        let entries = parse_system_sums(&text.replace("BOOTX64.EFI", "bootx64.efi")).unwrap();
        let problems = check_files(dir.path(), &entries);
        assert!(
            problems.iter().all(|p| !p.contains("inattendu")),
            "{problems:?}"
        );
    }

    #[test]
    fn signature_problems_are_altered_and_absence_is_unsigned() {
        let (dir, text) = tree(false);
        let r = check_tree(dir.path(), &trusted());
        assert_eq!(r.status, IntegrityStatus::Unsigned);
        assert!(r.problems[0].contains("non signées"), "{:?}", r.problems);
        // Signature d'une autre clé de version : altérée.
        let other = SigningIdentity::from_seed(&[8u8; 32]);
        let sig = sign_system(&other, &text);
        fs::write(
            dir.path().join(SYSTEM_SIGNATURE_FILE),
            serde_json::to_string(&sig).unwrap(),
        )
        .unwrap();
        let r = check_tree(dir.path(), &trusted());
        assert_eq!(r.status, IntegrityStatus::Altered);
        assert!(
            r.problems[0].contains("clé de version inconnue"),
            "{:?}",
            r.problems
        );
        // Empreintes réécrites après signature : signature invalide.
        let sig = sign_system(&identity(), &text);
        fs::write(
            dir.path().join(SYSTEM_SIGNATURE_FILE),
            serde_json::to_string(&sig).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.path().join(SYSTEM_SUMS_FILE),
            text.replace("secmet 0.5.0", "secmet 9.9.9"),
        )
        .unwrap();
        assert_eq!(
            check_tree(dir.path(), &trusted()).status,
            IntegrityStatus::Altered
        );
        // Plus de SYSTEM.sums du tout : non signée (clé d'avant la 0.5.0).
        fs::remove_file(dir.path().join(SYSTEM_SUMS_FILE)).unwrap();
        assert_eq!(
            check_tree(dir.path(), &trusted()).status,
            IntegrityStatus::Unsigned
        );
    }

    #[test]
    fn system_label_is_not_confused_with_sums_signature() {
        let (_, text) = tree(false);
        let id = identity();
        let sig = sign_system(&id, &text);
        assert!(
            verify_system_signature(&sig, &text, &trusted())
                .unwrap()
                .trusted
        );
        // La même signature ne vaut pas pour SHA256SUMS (autre étiquette).
        assert!(release::verify_sums(&sig, &text, &trusted()).is_err());
        let sums_sig = release::sign_sums(&id, &text);
        assert!(verify_system_signature(&sums_sig, &text, &trusted()).is_err());
    }

    #[test]
    fn unsafe_paths_are_refused() {
        let h = "0".repeat(64);
        for bad in [
            "../etc/passwd",
            "live/../../x",
            "/live/vmlinuz",
            "./live/vmlinuz",
            "live//vmlinuz",
            "live\\vmlinuz",
            "",
        ] {
            assert!(!valid_relative_path(bad), "{bad}");
            assert!(
                parse_system_sums(&format!("{h}  {bad}\n")).is_err(),
                "{bad}"
            );
        }
        assert!(valid_relative_path("EFI/BOOT/BOOTX64.EFI"));
        // Doublon (à la casse près) refusé.
        assert!(parse_system_sums(&format!("{h}  live/a\n{h}  LIVE/A\n")).is_err());
        assert!(parse_system_sums("# rien\n").is_err());
    }

    #[test]
    fn report_round_trips_and_tolerates_unknown_fields() {
        let (dir, _) = tree(true);
        let r = check_tree(dir.path(), &trusted());
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"status\":\"ok\""), "{json}");
        let back: IntegrityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
        let minimal: IntegrityReport =
            serde_json::from_str(r#"{"status":"unavailable","checked_at":"x"}"#).unwrap();
        assert_eq!(minimal.status, IntegrityStatus::Unavailable);
        assert!(minimal.problems.is_empty());
    }
}
