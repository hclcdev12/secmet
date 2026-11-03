//! Génération PDF native (`secmet-attest` : signature Ed25519, QR Base45,
//! PDF sans Chromium) et ouverture des certificats.

use chrono::NaiveDateTime;
use secmet_attest::model::{
    Attestation, BenchFacts, DiskFacts, ErasureFacts, NistLevel, Outcome, ValidationFacts,
    SCHEMA_VERSION,
};
use secmet_attest::sign::SigningIdentity;
use secmet_attest::{export, keystore};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::i18n::{self, tr_in, Lang};

use super::labels::method_label_in;
use super::{
    certificates_dir, disk_certificate_id, lot_certificate_id, pdf_filename, pdf_filename_grouped,
    CertificatePdfMode, DiskOperationRecord, SessionReport,
};

/// Lance `program path` sans attendre sa fin : un gestionnaire de fichiers
/// ou un lecteur PDF peut vivre aussi longtemps que sa fenêtre, et l'attendre
/// bloquerait le fil GTK. Le fils est récolté sur un fil dédié (pas de zombie).
fn launch_detached(program: &str, path: &Path) -> Result<(), String> {
    let mut child = Command::new(program)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn xdg_open(path: &Path) -> Result<(), String> {
    launch_detached("xdg-open", path)
}

fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn path_to_file_uri(path: &Path) -> Result<String, String> {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    gtk4::glib::filename_to_uri(absolute, None)
        .map(|u| u.to_string())
        .map_err(|e| format!("URI fichier: {}", e))
}

/// Ouvre le certificat PDF dans le lecteur par défaut (ou repli xdg-open).
pub fn open_pdf(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("Fichier PDF introuvable : {}", path.display()));
    }

    let header = fs::read(path).map_err(|e| format!("Lecture PDF: {}", e))?;
    if header.len() < 5 || &header[..5] != b"%PDF-" {
        return Err(format!(
            "Le fichier n'est pas un PDF valide (en-tête manquant). \
             Ouvrez uniquement le fichier .pdf, pas les .json :\n{}",
            path.display()
        ));
    }

    let uri = path_to_file_uri(path)?;

    if gtk4::gio::AppInfo::launch_default_for_uri(&uri, None::<&gtk4::gio::AppLaunchContext>)
        .is_ok()
    {
        return Ok(());
    }

    if xdg_open(path).is_ok() {
        return Ok(());
    }

    Err(format!(
        "Impossible d'ouvrir le PDF. Installez un lecteur (evince, okular, atril) \
         et ouvrez manuellement :\n{}",
        path.display()
    ))
}

fn open_folder_in_file_manager(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("Dossier introuvable : {}", path.display()));
    }

    // Gestionnaire par défaut du bureau d'abord (asynchrone, comme pour le
    // PDF), puis un gestionnaire connu lancé sans attente, puis xdg-open.
    if let Ok(uri) = path_to_file_uri(path) {
        if gtk4::gio::AppInfo::launch_default_for_uri(&uri, None::<&gtk4::gio::AppLaunchContext>)
            .is_ok()
        {
            return Ok(());
        }
    }

    for fm in [
        "nautilus",
        "thunar",
        "dolphin",
        "pcmanfm-qt",
        "pcmanfm",
        "nemo",
        "caja",
    ] {
        if command_exists(fm) && launch_detached(fm, path).is_ok() {
            return Ok(());
        }
    }

    xdg_open(path)
}

/// Ouvre le dossier des certificats dans un gestionnaire de fichiers.
pub fn open_certificates_folder() -> Result<(), String> {
    open_folder_in_file_manager(&certificates_dir())
}

/// Ouvre `path` dans le gestionnaire de fichiers (dossier d'un lot, export).
pub fn open_folder(path: &Path) -> Result<(), String> {
    open_folder_in_file_manager(path)
}

/// Clé de signature du poste (`attestation-key.hex` dans le dossier de
/// configuration : `~/.config/secmet` ou `SECMET_DATA_DIR/config`), créée au
/// premier certificat. La clé publique est écrite à côté (`.pub`).
pub(crate) fn signing_identity() -> Result<SigningIdentity, String> {
    let path = crate::paths::config_file(keystore::KEY_FILE);
    keystore::load_or_create(&path).map_err(|e| format!("Clé d'attestation : {e}"))
}

/// Langue du PDF correspondant à celle de l'interface.
pub(crate) fn pdf_lang(lang: Lang) -> secmet_attest::lang::Lang {
    match lang {
        Lang::Fr => secmet_attest::lang::Lang::Fr,
        Lang::En => secmet_attest::lang::Lang::En,
        Lang::De => secmet_attest::lang::Lang::De,
    }
}

/// Génère un certificat PDF signé (individuel ou groupé selon le mode),
/// rédigé dans la langue courante de l'interface.
pub fn render_pdf_bundle(
    session: &SessionReport,
    selected_disk_names: &[String],
    operator_name: &str,
    mode: CertificatePdfMode,
) -> Result<PathBuf, String> {
    render_pdf_bundle_in(
        &certificates_dir(),
        session,
        selected_disk_names,
        operator_name,
        mode,
    )
    .map(|e| e.pdf)
}

/// Comme [`render_pdf_bundle`], dans le dossier `dir`, en rendant les
/// enveloppes signées (lots : empreintes pour le journal et le manifeste).
pub fn render_pdf_bundle_in(
    dir: &Path,
    session: &SessionReport,
    selected_disk_names: &[String],
    operator_name: &str,
    mode: CertificatePdfMode,
) -> Result<export::Exported, String> {
    let operator_name = operator_name.trim();
    if operator_name.is_empty() {
        return Err("Nom de l'opérateur requis".to_string());
    }
    if selected_disk_names.is_empty() {
        return Err("Aucun disque sélectionné pour le certificat".to_string());
    }

    let lang = i18n::current();
    let identity = signing_identity()?;
    let report_date = crate::clock::stamp();

    let mut attestations = Vec::new();
    let mut errors = Vec::new();
    for name in selected_disk_names {
        let Some(disk) = session.disks.iter().find(|d| &d.disk_name == name) else {
            errors.push(format!("{}: inconnu dans la session", name));
            continue;
        };
        if !disk.is_finished() {
            errors.push(format!("{}: opération non terminée", name));
            continue;
        }
        attestations.push(attestation_from_record(
            session,
            disk,
            operator_name,
            &report_date,
            lang,
        ));
    }
    if attestations.is_empty() {
        return Err(if errors.is_empty() {
            "Aucun certificat généré".to_string()
        } else {
            errors.join("\n")
        });
    }

    let (pdf_file, lot) = match mode {
        CertificatePdfMode::PerDisk => (pdf_filename(&session.certificate_id), None),
        CertificatePdfMode::Grouped => (
            pdf_filename_grouped(&session.certificate_id),
            Some(lot_certificate_id(&session.certificate_id)),
        ),
    };
    let logo = crate::pdf_logo::current_jpeg();
    let exported = export::export_with_logo(
        &identity,
        dir,
        &pdf_file,
        &attestations,
        lot.as_deref(),
        logo.as_deref(),
        pdf_lang(lang),
    )
    .map_err(|e| format!("Génération du certificat : {e}"))?;

    if !errors.is_empty() {
        eprintln!("Certificats partiels — erreurs:\n{}", errors.join("\n"));
    }
    Ok(exported)
}

fn duration_seconds(start: Option<&str>, end: Option<&str>) -> Option<u64> {
    let fmt = "%Y-%m-%d %H:%M:%S";
    let s = NaiveDateTime::parse_from_str(start?, fmt).ok()?;
    let e = NaiveDateTime::parse_from_str(end?, fmt).ok()?;
    (e >= s).then(|| (e - s).num_seconds().max(0) as u64)
}

fn file_sha256(path: Option<&str>) -> Option<String> {
    let bytes = fs::read(path?).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
}

/// Empreinte des preuves : SHA-256 canonique du seed, de l'échantillon et
/// des signatures résiduelles rapportés par le helper.
pub(crate) fn evidence_digest(job: Option<&super::JobOutcomeRecord>) -> String {
    let value = serde_json::json!({
        "seed": job.map(|j| j.seed.as_str()),
        "samples": job.map(|j| j.samples),
        "anomalies": job.map(|j| j.anomalies),
        "residual_signatures": job.map(|j| j.residual_signatures.clone()),
        "method_code": job.map(|j| j.method_code.as_str()),
    });
    secmet_attest::sha256_hex(secmet_attest::canonical::to_string(&value).as_bytes())
}

/// Construit l'attestation (non signée) d'un enregistrement de session. Pur.
/// Les textes libres (méthode, réserves, valeurs par défaut) sont rédigés
/// dans `lang`, la langue du PDF qui les imprime.
pub(crate) fn attestation_from_record(
    session: &SessionReport,
    rec: &DiskOperationRecord,
    operator_name: &str,
    report_date: &str,
    lang: Lang,
) -> Attestation {
    let caveat = |key: &str| tr_in(lang, key).to_string();
    let job = rec.job.as_ref();
    let outcome = match job.map(|j| j.outcome.as_str()) {
        Some("success") => Outcome::Success,
        Some("cancelled") => Outcome::Cancelled,
        _ if rec.is_success() && job.is_none() => Outcome::Success,
        _ => Outcome::Failed,
    };
    let mut caveats = Vec::new();
    let nist_level = match (outcome, job.map(|j| j.nist_level.as_str())) {
        (Outcome::Success, Some("Purge")) => NistLevel::Purge,
        (Outcome::Success, Some("Clear")) => NistLevel::Clear,
        (Outcome::Success, _) => {
            caveats.push(caveat("aucune catégorie NIST rapportée par le moteur"));
            NistLevel::NotClassified
        }
        _ => NistLevel::NotClassified,
    };
    if rec.is_usb {
        caveats.push(caveat(
            "disque derrière un pont USB : aucune commande firmware envoyée",
        ));
    }
    if rec.nvme_multiple_namespaces && job.is_some_and(|j| !j.whole_controller) {
        caveats.push(caveat(
            "contrôleur NVMe à plusieurs espaces de noms : seul l'espace de noms effacé est \
             couvert par l'étape exécutée, les autres restent à traiter",
        ));
    }
    // Horloge du poste (clé USB, annexe M §M.7) : réserve selon le statut.
    if let Some(key) = crate::clock::caveat_key() {
        caveats.push(caveat(key));
    }
    if rec.root_traced == Some(false) {
        caveats.push(caveat(
            "disque système de l'hôte non identifié lors de l'inventaire (racine non \
             rattachée à un disque local : média live ou ZFS) ; protection du disque \
             système par verrou exclusif du noyau uniquement",
        ));
    }
    let (validation_kind, samples, anomalies, seed, residual) = match job {
        Some(j) if j.samples > 0 => (
            "canaries".to_string(),
            j.samples,
            j.anomalies,
            Some(j.seed.clone()),
            j.residual_signatures.clone(),
        ),
        _ => ("none".to_string(), 0, 0, None, Vec::new()),
    };
    // Sans relecture enregistrée, aucune catégorie n'est revendiquée : Clear
    // exige lui aussi une vérification (SP 800-88 §4.7), la catégorie n'est
    // pas à l'appréciation de l'interface mais du moteur et de sa preuve.
    let nist_level = if nist_level != NistLevel::NotClassified && validation_kind == "none" {
        caveats.push(caveat(
            "aucune relecture enregistrée : catégorie NIST non revendiquée",
        ));
        NistLevel::NotClassified
    } else {
        nist_level
    };
    if rec.serial_number.trim().is_empty() || rec.serial_number == "Non disponible" {
        caveats.push(caveat(
            "numéro de série non exposé par le disque : identification par modèle, \
             capacité et nom de périphérique",
        ));
    }
    Attestation {
        version: SCHEMA_VERSION,
        certificate_id: disk_certificate_id(&session.certificate_id, &rec.disk_name),
        session_id: session.certificate_id.clone(),
        issued_at: report_date.to_string(),
        operator: operator_name.to_string(),
        software: if session.software.is_empty() {
            rec.tool_version.clone()
        } else {
            session.software.clone()
        },
        disk: DiskFacts {
            name: rec.disk_name.clone(),
            model: rec.model.clone(),
            serial: if rec.serial_number.trim().is_empty() || rec.serial_number == "Non disponible"
            {
                tr_in(lang, "Non disponible").to_string()
            } else {
                rec.serial_number.clone()
            },
            firmware: rec.firmware.clone(),
            // Seul « INCONNU » est un mot ; HDD, SSD SATA… passent tels quels.
            media: tr_in(lang, &rec.media_type).to_string(),
            transport: rec.transport.to_lowercase(),
            capacity: rec.capacity.clone(),
            size_bytes: rec.size_bytes,
            block_size: None,
        },
        erasure: ErasureFacts {
            method_code: job
                .map(|j| j.method_code.clone())
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| "UNKNOWN".to_string()),
            method_label: job
                .filter(|j| !j.method_code.is_empty())
                .map(|j| method_label_in(lang, &j.method_code))
                .or_else(|| job.map(|j| j.method_label.clone()))
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| rec.erase_method.clone()),
            nist_level,
            policy: job
                .map(|j| j.policy.clone())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| "nist-800-88r1".to_string()),
            outcome,
            started_at: rec.started_at.clone(),
            finished_at: rec.finished_at.clone(),
            // Durée mesurée par le helper (horloge monotone) ; repli sur les
            // horodatages muraux pour un enregistrement sans job.
            duration_seconds: job.map(|j| j.seconds).filter(|s| *s > 0).or_else(|| {
                duration_seconds(rec.started_at.as_deref(), rec.finished_at.as_deref())
            }),
            caveats,
        },
        validation: ValidationFacts {
            kind: validation_kind,
            seed,
            samples,
            anomalies,
            evidence_sha256: evidence_digest(job),
            residual_signatures: residual,
            // « OK », « KO » ou « non effectuée ».
            verify_status: tr_in(lang, &rec.verify_status).to_string(),
        },
        bench: BenchFacts {
            manufacturer: session.bench.manufacturer.clone(),
            model: session.bench.model.clone(),
            serial: session.bench.serial.clone(),
        },
        report_sha256: file_sha256(rec.technical_log_path.as_deref()),
    }
}
