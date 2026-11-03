//! Certificat PDF d'effacement et rapport de session.
//!
//! La session mémorise, pour chaque disque, l'identité relue par le helper au
//! démarrage de l'opération et le `JobReport` final (méthode, catégorie NIST,
//! seed des canaris, échantillons, signatures résiduelles). Le certificat est
//! signé et rendu par `secmet-attest` ; aucun outil externe n'est relancé.

mod labels;
mod pdf;

pub use labels::{
    disposition_label, disposition_label_in, guidances_label, media_label, method_label,
    pdf_result_from_success_flag, pdf_result_label, reasons_label,
};
pub use pdf::{open_certificates_folder, open_folder, open_pdf, render_pdf_bundle};
pub(crate) use pdf::{pdf_lang, render_pdf_bundle_in, signing_identity};

use crate::app_identity::{CERT_ID_PREFIX, PKG_NAME};
use crate::disk::Disk;
use crate::helper_client::{self, HostInfo, JobReport, Outcome};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub(crate) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

static SESSION: OnceLock<Mutex<Option<SessionReport>>> = OnceLock::new();

fn session_mutex() -> &'static Mutex<Option<SessionReport>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

/// Mode de génération du certificat PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificatePdfMode {
    /// Une page par disque, un JSON signé par disque.
    PerDisk,
    /// Un certificat lot (synthèse + une page par disque), plus un JSON de lot.
    Grouped,
}

/// Le banc (inventaire fourni par le helper).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BenchRecord {
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub serial: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub cpu: String,
    #[serde(default)]
    pub memory_bytes: u64,
    #[serde(default)]
    pub kernel: String,
    #[serde(default)]
    pub bios: String,
}

impl From<HostInfo> for BenchRecord {
    fn from(h: HostInfo) -> Self {
        Self {
            manufacturer: h.manufacturer,
            model: h.model,
            serial: h.serial,
            hostname: h.hostname,
            cpu: h.cpu,
            memory_bytes: h.memory_bytes,
            kernel: h.kernel,
            bios: h.bios,
        }
    }
}

/// Ce que le helper a rapporté à la fin du job.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct JobOutcomeRecord {
    /// `success`, `failed`, `cancelled`.
    pub outcome: String,
    /// Code stable de la méthode réussie (ou dernière tentée).
    pub method_code: String,
    /// Libellé humain.
    pub method_label: String,
    /// `Purge`, `Clear`, vide si aucune catégorie.
    pub nist_level: String,
    pub policy: String,
    /// Seed de session (hex) des canaris et motifs.
    pub seed: String,
    /// Blocs relus à la validation.
    pub samples: u32,
    /// Anomalies à la validation.
    pub anomalies: u32,
    #[serde(default)]
    pub residual_signatures: Vec<String>,
    /// Étapes tentées (primaire + replis).
    pub steps_tried: u32,
    pub seconds: u64,
    /// Détail de la dernière étape.
    #[serde(default)]
    pub detail: String,
    /// La dernière étape portait sur tout le contrôleur NVMe.
    #[serde(default)]
    pub whole_controller: bool,
    /// Sort du support (`reusable`, `destroy_recommended`, `destroy_required`,
    /// `retry`) ; vide pour un enregistrement antérieur.
    #[serde(default)]
    pub disposition: String,
    /// Raisons du sort (codes stables du moteur).
    #[serde(default)]
    pub disposition_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskOperationRecord {
    pub disk_name: String,
    pub device_path: String,
    pub media_type: String,
    pub model: String,
    pub serial_number: String,
    #[serde(default)]
    pub firmware: String,
    #[serde(default)]
    pub transport: String,
    pub capacity: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    pub erase_method: String,
    pub tool_version: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub result_label: String,
    pub technical_log_path: Option<String>,
    #[serde(default)]
    pub verify_status: String,
    #[serde(default)]
    pub is_usb: bool,
    #[serde(default)]
    pub disk_kind: String,
    #[serde(default)]
    pub job: Option<JobOutcomeRecord>,
    /// Racine de l'hôte rattachée à un disque local lors de l'inventaire
    /// (`Some(false)` : média live ou ZFS, réserve sur le certificat).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_traced: Option<bool>,
    /// Contrôleur NVMe à plusieurs namespaces : réserve sur le certificat si
    /// l'étape exécutée n'a couvert que le namespace ouvert.
    #[serde(default)]
    pub nvme_multiple_namespaces: bool,
}

impl DiskOperationRecord {
    pub fn is_finished(&self) -> bool {
        self.finished_at.is_some()
    }

    pub fn is_success(&self) -> bool {
        self.result_label == "Succès"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReport {
    pub certificate_id: String,
    pub session_started_at: String,
    pub session_finished_at: Option<String>,
    /// `secmet 0.2.0`.
    #[serde(default)]
    pub software: String,
    #[serde(default)]
    pub bench: BenchRecord,
    pub session_log_path: Option<String>,
    pub disks: Vec<DiskOperationRecord>,
}

/// Crée `path` (et ses parents) en `0700` : journaux, rapports et
/// certificats portent numéros de série et seed de session.
pub(crate) fn create_private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    match fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
    {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn logs_dir() -> PathBuf {
    let path = crate::paths::logs_dir();
    let _ = create_private_dir(&path);
    path
}

pub fn certificates_dir() -> PathBuf {
    // Lot actif (annexe M) : les certificats vont dans son dossier.
    let mut path = crate::lot::active_dir().unwrap_or_else(logs_dir);
    path.push("certificates");
    let _ = create_private_dir(&path);
    path
}

/// Écrit `bytes` dans un fichier `0600`.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

pub(crate) fn software_line() -> String {
    format!("{PKG_NAME} {APP_VERSION}")
}

pub(crate) fn new_certificate_id() -> String {
    let date = crate::clock::now().format("%Y%m%d");
    let suffix: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(4)
        .map(char::from)
        .collect();
    format!("{CERT_ID_PREFIX}-{}-{}", date, suffix.to_uppercase())
}

pub(crate) fn disk_certificate_id(session_id: &str, disk_name: &str) -> String {
    format!("{}-{}", session_id, disk_name)
}

pub(crate) fn lot_certificate_id(session_id: &str) -> String {
    format!("{}-lot", session_id)
}

pub fn pdf_filename_grouped(session_id: &str) -> String {
    pdf_filename(&lot_certificate_id(session_id))
}

pub(crate) fn pdf_filename(certificate_id: &str) -> String {
    let safe: String = certificate_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.pdf", safe)
}

fn now_stamp() -> String {
    crate::clock::stamp()
}

pub fn clear_session() {
    if let Ok(mut guard) = session_mutex().lock() {
        *guard = None;
    }
}

/// Réinitialise les enregistrements pour un nouvel essai sur les disques indiqués.
/// Prépare la session pour un lancement : créée si absente, sinon les
/// disques déjà connus sont remis à zéro et les nouveaux ajoutés. Un disque
/// lancé sans « Rafraîchir » après une première passe figure ainsi toujours
/// dans le certificat.
pub fn ensure_session_for_disks(disks: &[Disk]) {
    if current_session().is_none() {
        begin_session_from_disks(disks);
        return;
    }
    let names: Vec<String> = disks.iter().map(|d| d.name.clone()).collect();
    reset_disks_for_retry(&names);
    if let Ok(mut guard) = session_mutex().lock() {
        if let Some(session) = guard.as_mut() {
            for disk in disks {
                if !session.disks.iter().any(|d| d.disk_name == disk.name) {
                    session.disks.push(record_from_disk(disk));
                }
            }
        }
    }
}

pub fn reset_disks_for_retry(disk_names: &[String]) {
    if let Ok(mut guard) = session_mutex().lock() {
        if let Some(session) = guard.as_mut() {
            for name in disk_names {
                if let Some(rec) = session.disks.iter_mut().find(|d| d.disk_name == *name) {
                    rec.started_at = None;
                    rec.finished_at = None;
                    rec.result_label.clear();
                    rec.technical_log_path = None;
                    rec.job = None;
                    rec.verify_status.clear();
                }
            }
            session.session_finished_at = None;
        }
    }
}

pub fn attach_session_log(path: String) {
    if let Ok(mut guard) = session_mutex().lock() {
        if let Some(session) = guard.as_mut() {
            session.session_log_path = Some(path);
        }
    }
}

fn record_from_disk(disk: &Disk) -> DiskOperationRecord {
    DiskOperationRecord {
        disk_name: disk.name.clone(),
        device_path: disk.path.clone(),
        media_type: media_label(disk.kind, disk.is_usb),
        model: disk.model.clone(),
        serial_number: if disk.serial.is_empty() {
            "Non disponible".to_string()
        } else {
            disk.serial.clone()
        },
        firmware: disk.firmware.clone(),
        transport: disk.transport.clone(),
        capacity: disk.size.clone(),
        size_bytes: (disk.size_bytes > 0).then_some(disk.size_bytes),
        erase_method: disk.plan_description(),
        tool_version: software_line(),
        started_at: None,
        finished_at: None,
        result_label: String::new(),
        technical_log_path: None,
        verify_status: String::new(),
        is_usb: disk.is_usb,
        disk_kind: disk.kind.label().to_string(),
        job: None,
        root_traced: disk.root_traced,
        nvme_multiple_namespaces: disk.nvme_multiple_namespaces,
    }
}

/// Démarre une session d'effacement (disques sélectionnés). Le banc est
/// relevé par le helper (DMI lisible seulement par root).
pub fn begin_session_from_disks(disks: &[Disk]) {
    let bench = helper_client::current()
        .and_then(|h| helper_client::host_info(&h).ok())
        .map(BenchRecord::from)
        .unwrap_or_default();
    let report = SessionReport {
        certificate_id: new_certificate_id(),
        session_started_at: now_stamp(),
        session_finished_at: None,
        software: software_line(),
        bench,
        session_log_path: None,
        disks: disks.iter().map(record_from_disk).collect(),
    };
    if let Ok(mut guard) = session_mutex().lock() {
        *guard = Some(report);
    }
}

/// Enregistre le début d'un disque avec son identité la plus fraîche.
pub fn mark_disk_started(disk_name: &str, disk: &Disk) {
    let now = now_stamp();
    if let Ok(mut guard) = session_mutex().lock() {
        if let Some(session) = guard.as_mut() {
            if let Some(rec) = session.disks.iter_mut().find(|d| d.disk_name == disk_name) {
                let fresh = record_from_disk(disk);
                let technical_log_path = rec.technical_log_path.take();
                *rec = DiskOperationRecord {
                    started_at: Some(now),
                    technical_log_path,
                    ..fresh
                };
            }
        }
    }
}

/// Résumé d'un `JobReport` pour la session et le certificat.
pub fn job_outcome_record(report: &JobReport) -> JobOutcomeRecord {
    let last = report.steps.last();
    let success = report.outcome == Outcome::Success;
    let method_code = report
        .method
        .clone()
        .or_else(|| last.map(|s| s.method.clone()))
        .unwrap_or_default();
    let validation = last.and_then(|s| s.validation.as_ref());
    JobOutcomeRecord {
        outcome: match report.outcome {
            Outcome::Success => "success",
            Outcome::Failed => "failed",
            Outcome::Cancelled => "cancelled",
        }
        .to_string(),
        method_label: method_label(&method_code),
        method_code,
        nist_level: if success {
            report.achieved_level.clone().unwrap_or_default()
        } else {
            String::new()
        },
        policy: report.policy.clone(),
        seed: report.seed.clone(),
        samples: validation.map(|v| v.checked as u32).unwrap_or(0),
        anomalies: validation.map(|v| v.anomalies as u32).unwrap_or(0),
        residual_signatures: last.map(|s| s.signatures.clone()).unwrap_or_default(),
        steps_tried: report.steps.len() as u32,
        seconds: report.seconds,
        detail: last.map(|s| s.detail.clone()).unwrap_or_default(),
        whole_controller: last.is_some_and(|s| s.whole_controller),
        disposition: report.disposition.clone(),
        disposition_reasons: report.disposition_reasons.clone(),
    }
}

/// Enregistre la fin d'un job (succès, échec ou interruption).
pub fn finish_disk_job(disk_name: &str, report: &JobReport) {
    let now = now_stamp();
    let job = job_outcome_record(report);
    if let Ok(mut guard) = session_mutex().lock() {
        if let Some(session) = guard.as_mut() {
            if let Some(rec) = session.disks.iter_mut().find(|d| d.disk_name == disk_name) {
                rec.finished_at = Some(now);
                rec.result_label = pdf_result_from_success_flag(job.outcome == "success");
                rec.erase_method = job.method_label.clone();
                rec.job = Some(job);
            }
        }
    }
}

/// Le job n'a pas pu être lancé (refus, blocage, liaison perdue) : sort
/// `retry`, sans raison.
pub fn finish_disk_failed(disk_name: &str, reason: &str) {
    finish_disk_unrunnable(disk_name, reason, "retry", &[]);
}

/// Le job n'a pas pu être lancé ; `disposition` et `reasons` : sort du
/// support décidé d'après le verdict du moteur (`secmet_core::disposition`).
pub fn finish_disk_unrunnable(
    disk_name: &str,
    reason: &str,
    disposition: &str,
    reasons: &[String],
) {
    let now = now_stamp();
    if let Ok(mut guard) = session_mutex().lock() {
        if let Some(session) = guard.as_mut() {
            if let Some(rec) = session.disks.iter_mut().find(|d| d.disk_name == disk_name) {
                rec.finished_at = Some(now);
                rec.result_label = pdf_result_from_success_flag(false);
                rec.job = Some(JobOutcomeRecord {
                    outcome: "failed".into(),
                    detail: reason.to_string(),
                    disposition: disposition.to_string(),
                    disposition_reasons: reasons.to_vec(),
                    ..JobOutcomeRecord::default()
                });
            }
        }
    }
}

/// Décision de l'opérateur : le disque sort du circuit de réemploi
/// (`destroy_required`, raison `reason_code`), quel que soit l'état enregistré.
pub fn condemn_disk(disk_name: &str, reason_code: &str) {
    if let Ok(mut guard) = session_mutex().lock() {
        if let Some(session) = guard.as_mut() {
            if let Some(rec) = session.disks.iter_mut().find(|d| d.disk_name == disk_name) {
                if rec.finished_at.is_none() {
                    rec.finished_at = Some(now_stamp());
                }
                rec.result_label = pdf_result_from_success_flag(false);
                let job = rec.job.get_or_insert_with(|| JobOutcomeRecord {
                    outcome: "failed".into(),
                    ..JobOutcomeRecord::default()
                });
                job.disposition = "destroy_required".to_string();
                if !job.disposition_reasons.iter().any(|r| r == reason_code) {
                    job.disposition_reasons.push(reason_code.to_string());
                }
            }
        }
    }
}

pub fn finalize_session() -> Option<PathBuf> {
    let mut path_out = None;
    if let Ok(mut guard) = session_mutex().lock() {
        if let Some(session) = guard.as_mut() {
            session.session_finished_at = Some(now_stamp());
            let mut path = crate::lot::active_dir()
                .map(|d| d.join(crate::lot::SESSIONS_DIR))
                .unwrap_or_else(logs_dir);
            let _ = create_private_dir(&path);
            path.push(format!(
                "session_report_{}.json",
                crate::clock::now().format("%Y%m%d_%H%M%S")
            ));
            if let Ok(json) = serde_json::to_string_pretty(session) {
                if write_private(&path, json.as_bytes()).is_ok() {
                    session.session_log_path = Some(path.display().to_string());
                    path_out = Some(path);
                }
            }
        }
    }
    path_out
}

pub fn current_session() -> Option<SessionReport> {
    session_mutex().lock().ok().and_then(|g| g.clone())
}

/// Vrai si une session contient au moins un disque enregistré.
pub fn session_has_results() -> bool {
    session_mutex()
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| !s.disks.is_empty()))
        .unwrap_or(false)
}

pub fn tool_versions_line() -> String {
    current_session()
        .map(|s| s.software)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(software_line)
}

pub fn set_disk_technical_log_path(disk_name: &str, path: String) {
    if let Ok(mut guard) = session_mutex().lock() {
        if let Some(session) = guard.as_mut() {
            if let Some(rec) = session.disks.iter_mut().find(|d| d.disk_name == disk_name) {
                rec.technical_log_path = Some(path);
            }
        }
    }
}

pub fn set_disk_verify_status(disk_name: &str, status: &str) {
    if let Ok(mut guard) = session_mutex().lock() {
        if let Some(session) = guard.as_mut() {
            if let Some(rec) = session.disks.iter_mut().find(|d| d.disk_name == disk_name) {
                rec.verify_status = status.to_string();
            }
        }
    }
}

pub fn session_ready_for_pdf() -> bool {
    session_mutex()
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.disks.iter().all(|d| d.is_finished())))
        .unwrap_or(false)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::disk::DiskKind;
    use crate::helper_client::{Outcome, StepReport};
    use crate::i18n::Lang;
    use std::path::Path;

    pub(crate) fn record(name: &str, success: bool) -> DiskOperationRecord {
        DiskOperationRecord {
            disk_name: name.to_string(),
            device_path: format!("/dev/{name}"),
            media_type: "HDD".to_string(),
            model: "Test".to_string(),
            serial_number: "X".to_string(),
            firmware: "F1".to_string(),
            transport: "sata".to_string(),
            capacity: "1G".to_string(),
            size_bytes: Some(1_000_000_000),
            erase_method: "Écriture hôte".to_string(),
            tool_version: "secmet test".to_string(),
            started_at: Some("2026-01-01 10:00:00".to_string()),
            finished_at: Some("2026-01-01 11:00:00".to_string()),
            result_label: pdf_result_from_success_flag(success),
            technical_log_path: None,
            verify_status: "OK".to_string(),
            is_usb: false,
            disk_kind: "HDD".to_string(),
            root_traced: None,
            nvme_multiple_namespaces: false,
            job: Some(JobOutcomeRecord {
                disposition: if success {
                    "reusable"
                } else {
                    "destroy_required"
                }
                .into(),
                disposition_reasons: if success {
                    Vec::new()
                } else {
                    vec!["UnreadableSectors".into()]
                },
                outcome: if success { "success" } else { "failed" }.into(),
                method_code: "HOST_OVERWRITE_1_ZERO".into(),
                method_label: method_label("HOST_OVERWRITE_1_ZERO"),
                nist_level: if success { "Clear" } else { "" }.into(),
                policy: "nist-800-88r1".into(),
                seed: "ab".repeat(32),
                samples: 1003,
                anomalies: 0,
                residual_signatures: vec![],
                steps_tried: 1,
                seconds: 3600,
                detail: "validé".into(),
                whole_controller: true,
            }),
        }
    }

    pub(crate) fn session_with(disks: Vec<DiskOperationRecord>) -> SessionReport {
        SessionReport {
            certificate_id: "SEC-TEST-0001".to_string(),
            session_started_at: "2026-01-01 10:00:00".to_string(),
            session_finished_at: Some("2026-01-01 11:00:00".to_string()),
            software: "secmet test".to_string(),
            bench: BenchRecord {
                manufacturer: "Dell".into(),
                model: "OptiPlex".into(),
                serial: "S1".into(),
                ..BenchRecord::default()
            },
            session_log_path: None,
            disks,
        }
    }

    fn job_report(outcome: Outcome, level: Option<&str>) -> JobReport {
        JobReport {
            disposition: String::new(),
            disposition_reasons: Vec::new(),
            disposition_guidance: Vec::new(),
            outcome,
            achieved_level: level.map(str::to_string),
            method: Some("ATA_SANITIZE_BLOCK_ERASE".into()),
            policy: "nist-800-88r1".into(),
            seed: "cd".repeat(32),
            steps: vec![StepReport {
                index: 0,
                method: "ATA_SANITIZE_BLOCK_ERASE".into(),
                level: "Purge".into(),
                outcome,
                criterion: None,
                detail: "validé".into(),
                validation: Some(secmet_helper::runner::ValidationSummary {
                    checked: 1003,
                    anomalies: 0,
                    ..Default::default()
                }),
                signatures: vec![],
                seconds: 12,
                whole_controller: true,
            }],
            seconds: 12,
        }
    }

    #[test]
    fn job_outcome_record_reads_the_report() {
        let rec = job_outcome_record(&job_report(Outcome::Success, Some("Purge")));
        assert_eq!(rec.outcome, "success");
        assert_eq!(rec.nist_level, "Purge");
        assert_eq!(rec.method_code, "ATA_SANITIZE_BLOCK_ERASE");
        assert_eq!(rec.method_label, "ATA Sanitize Block Erase");
        assert_eq!(rec.samples, 1003);
        assert_eq!(rec.seed.len(), 64);
        let failed = job_outcome_record(&job_report(Outcome::Failed, None));
        assert_eq!(failed.outcome, "failed");
        assert!(failed.nist_level.is_empty());
    }

    #[test]
    fn old_session_reports_without_the_new_fields_still_load() {
        let mut value = serde_json::to_value(record("sda", true)).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.remove("job");
        obj.remove("firmware");
        obj.remove("size_bytes");
        let back: DiskOperationRecord = serde_json::from_value(value).unwrap();
        assert!(back.job.is_none());
        assert!(back.firmware.is_empty());
        let session = serde_json::to_value(session_with(vec![])).unwrap();
        let mut session = session;
        session.as_object_mut().unwrap().remove("bench");
        session.as_object_mut().unwrap().remove("software");
        let back: SessionReport = serde_json::from_value(session).unwrap();
        assert_eq!(back.bench, BenchRecord::default());
    }

    #[test]
    fn media_labels_and_result_labels() {
        assert_eq!(media_label(DiskKind::HddUsb, true), "HDD-USB");
        assert_eq!(media_label(DiskKind::SsdNvme, true), "NVME-USB");
        assert_eq!(media_label(DiskKind::SsdUsb, true), "SSD-USB");
        assert_eq!(media_label(DiskKind::Hdd, false), "HDD");
        assert_eq!(media_label(DiskKind::SsdSata, false), "SSD SATA");
        assert_eq!(pdf_result_label(false), "Échec");
        assert_eq!(pdf_result_from_success_flag(true), "Succès");
        assert_eq!(
            method_label("HOST_OVERWRITE_3_ZERO"),
            "Écriture hôte : 3 passes pseudo-aléatoires + zéros"
        );
        assert_eq!(
            method_label("NVME_SANITIZE_OVERWRITE_2"),
            "NVMe Sanitize Overwrite (2 passes)"
        );
        assert_eq!(method_label("INCONNU"), "INCONNU");
    }

    #[test]
    fn method_labels_follow_the_requested_language() {
        assert_eq!(
            labels::method_label_in(Lang::En, "HOST_OVERWRITE_1_ZERO"),
            "Host overwrite: 1 pseudo-random pass + zeros"
        );
        assert_eq!(
            labels::method_label_in(Lang::De, "HOST_OVERWRITE_3"),
            "Host-Überschreibung: 3 Pseudozufallsdurchgänge"
        );
        assert_eq!(
            labels::method_label_in(Lang::De, "ATA_SANITIZE_BLOCK_ERASE"),
            "ATA Sanitize Block Erase"
        );
        assert_eq!(
            method_label("HOST_OVERWRITE_1_ZERO"),
            "Écriture hôte : 1 passe pseudo-aléatoire + zéros"
        );
    }

    #[test]
    fn attestation_free_text_is_written_in_the_pdf_language() {
        let session = session_with(vec![record("sda", true)]);
        let mut usb = record("sda", true);
        usb.is_usb = true;
        usb.serial_number = String::new();
        usb.verify_status = "non effectuée".into();
        let fr =
            pdf::attestation_from_record(&session, &usb, "Op", "2026-01-01 11:05:00", Lang::Fr);
        let en =
            pdf::attestation_from_record(&session, &usb, "Op", "2026-01-01 11:05:00", Lang::En);
        assert_eq!(fr.erasure.caveats.len(), en.erasure.caveats.len());
        assert!(fr.erasure.caveats[0].starts_with("disque derrière un pont USB"));
        assert!(en.erasure.caveats[0].starts_with("disk behind a USB bridge"));
        assert_eq!(en.disk.serial, "Not available");
        assert_eq!(en.validation.verify_status, "not performed");
        assert_eq!(
            en.erasure.method_label,
            "Host overwrite: 1 pseudo-random pass + zeros"
        );
        // Seuls les textes changent : les faits attestés sont identiques.
        assert_eq!(fr.erasure.method_code, en.erasure.method_code);
        assert_eq!(fr.validation.evidence_sha256, en.validation.evidence_sha256);
        en.validate().unwrap();
    }

    #[test]
    fn open_pdf_missing_file_returns_error() {
        let err = open_pdf(Path::new("/tmp/secmet-nonexistent-cert.pdf")).unwrap_err();
        assert!(err.contains("introuvable") || err.contains("Impossible"));
    }

    #[test]
    fn pdf_filename_grouped_uses_lot_suffix() {
        assert_eq!(
            pdf_filename_grouped("SEC-20260101-ABCD"),
            "SEC-20260101-ABCD-lot.pdf"
        );
    }

    #[test]
    fn attestation_is_built_from_the_job_record() {
        let session = session_with(vec![record("sda", true)]);
        let a = pdf::attestation_from_record(
            &session,
            &session.disks[0],
            "Op Test",
            "2026-01-01 11:05:00",
            Lang::Fr,
        );
        assert_eq!(a.certificate_id, "SEC-TEST-0001-sda");
        assert_eq!(a.erasure.method_code, "HOST_OVERWRITE_1_ZERO");
        assert_eq!(a.erasure.nist_level, secmet_attest::model::NistLevel::Clear);
        assert_eq!(a.erasure.duration_seconds, Some(3600));
        assert_eq!(a.validation.kind, "canaries");
        assert_eq!(a.validation.samples, 1003);
        assert_eq!(a.validation.seed.as_deref(), Some("ab".repeat(32).as_str()));
        assert_eq!(a.bench.manufacturer, "Dell");
        assert_eq!(a.disk.size_bytes, Some(1_000_000_000));
        a.validate().unwrap();

        let mut purge = record("sdb", true);
        if let Some(job) = purge.job.as_mut() {
            job.nist_level = "Purge".into();
            job.method_code = "ATA_SANITIZE_BLOCK_ERASE".into();
        }
        let a =
            pdf::attestation_from_record(&session, &purge, "Op", "2026-01-01 11:05:00", Lang::Fr);
        assert_eq!(a.erasure.nist_level, secmet_attest::model::NistLevel::Purge);
        a.validate().unwrap();

        let failed = record("sdc", false);
        let a =
            pdf::attestation_from_record(&session, &failed, "Op", "2026-01-01 11:05:00", Lang::Fr);
        assert_eq!(a.erasure.outcome, secmet_attest::model::Outcome::Failed);
        // Le sort du support et ses raisons sont signés avec l'attestation.
        assert_eq!(
            a.erasure.disposition,
            Some(secmet_attest::model::Disposition::DestroyRequired)
        );
        assert_eq!(a.erasure.disposition_reasons, ["UnreadableSectors"]);
        let ok = record("sdb", true);
        let a_ok =
            pdf::attestation_from_record(&session, &ok, "Op", "2026-01-01 11:05:00", Lang::Fr);
        assert_eq!(
            a_ok.erasure.disposition,
            Some(secmet_attest::model::Disposition::Reusable)
        );
        assert_eq!(
            a.erasure.nist_level,
            secmet_attest::model::NistLevel::NotClassified
        );
        a.validate().unwrap();

        let mut no_job = record("sdd", true);
        no_job.job = None;
        let a =
            pdf::attestation_from_record(&session, &no_job, "Op", "2026-01-01 11:05:00", Lang::Fr);
        assert_eq!(
            a.erasure.nist_level,
            secmet_attest::model::NistLevel::NotClassified
        );
        assert_eq!(a.validation.kind, "none");
    }
}
