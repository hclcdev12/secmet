//! Point d'entrée journal de session (délègue à trace_log).

use crate::certificate;
use crate::helper_client::{JobReport, Outcome};
use crate::trace_log::{self, SessionMeta, Verdict};
use std::path::PathBuf;

/// Initialise le journal de session (appelé au lancement des effacements).
pub fn log_operation_start(disks: &[String]) {
    let Some(session) = certificate::current_session() else {
        return;
    };
    let tools = certificate::tool_versions_line();
    let meta = SessionMeta {
        started_at: session.session_started_at.clone(),
        disks: disks.to_vec(),
        tools_line: tools,
    };
    if let Ok(path) = trace_log::open_session(&session.certificate_id, meta) {
        println!(
            "[{}] Journal session : {}",
            crate::app_identity::PKG_NAME,
            path.display()
        );
    }
}

pub fn log_disk_section_begin(disk_name: &str, device_path: &str, subtitle: &str) {
    trace_log::begin_disk_section(disk_name, device_path, subtitle);
}

pub fn log_erase_start(disk_name: &str, method: &str) {
    trace_log::log_erase_start(disk_name, method);
}

/// Journalise le rapport final d'un job : étapes, validations, signatures.
pub fn log_job_report(disk_name: &str, report: &JobReport) {
    let outcome = match report.outcome {
        Outcome::Success => "terminé",
        Outcome::Failed => "échec",
        Outcome::Cancelled => "interrompu",
    };
    let mut body = format!(
        "Issue : {outcome} — méthode {} — niveau {} — politique {} — {} s\nSeed : {}\n",
        report.method.as_deref().unwrap_or("—"),
        report.achieved_level.as_deref().unwrap_or("aucun"),
        report.policy,
        report.seconds,
        report.seed
    );
    if !report.disposition.is_empty() {
        body.push_str(&format!(
            "Sort du support : {}",
            certificate::disposition_label_in(crate::i18n::Lang::Fr, &report.disposition)
        ));
        if !report.disposition_reasons.is_empty() {
            body.push_str(&format!(" — {}", report.disposition_reasons.join(", ")));
        }
        if !report.disposition_guidance.is_empty() {
            body.push_str(&format!(
                " — consigne {}",
                report.disposition_guidance.join(", ")
            ));
        }
        body.push('\n');
    }
    for step in &report.steps {
        body.push_str(&format!(
            "\n── Étape {} : {} ({}) ──\nIssue : {:?}{}\nDétail : {}\n",
            step.index + 1,
            step.method,
            step.level,
            step.outcome,
            step.criterion
                .as_ref()
                .map(|c| format!(" — critère {c}"))
                .unwrap_or_default(),
            step.detail
        ));
        if let Some(v) = &step.validation {
            body.push_str(&format!(
                "Validation : {} blocs relus, {} anomalie(s) (illisibles {:?}, canaris {:?}, motifs {:?}, non nuls {:?})\n",
                v.checked, v.anomalies, v.read_errors, v.canaries_intact, v.pattern_mismatch, v.not_zeroed
            ));
        }
        if !step.signatures.is_empty() {
            body.push_str(&format!(
                "Signatures résiduelles : {}\n",
                step.signatures.join(", ")
            ));
        }
    }
    trace_log::log_erase_finished(disk_name, outcome, &body);
    if let Some(path) = trace_log::disk_log_path(disk_name) {
        certificate::set_disk_technical_log_path(disk_name, path.display().to_string());
    }
}

/// Le job n'a pas pu être lancé ou la liaison a été perdue.
/// Décision de l'opérateur : disque sorti du circuit de réemploi.
pub fn log_disk_condemned(disk_name: &str) {
    trace_log::log_erase_finished(
        disk_name,
        "échec",
        "marqué à détruire par l'opérateur : verrou ATA Security sans mot de passe",
    );
}

pub fn log_erase_failed(disk_name: &str, reason: &str) {
    trace_log::log_erase_finished(disk_name, "échec", reason);
    if let Some(path) = trace_log::disk_log_path(disk_name) {
        certificate::set_disk_technical_log_path(disk_name, path.display().to_string());
    }
}

pub fn log_verify_result(disk_name: &str, ok: bool, detail: &str, output: Option<&str>) {
    trace_log::log_verify(disk_name, ok, detail, output);
    certificate::set_disk_verify_status(disk_name, if ok { "OK" } else { "KO" });
    if ok {
        log_disk_procedure_complete(disk_name);
    } else {
        log_disk_verify_failed(disk_name);
    }
}

pub fn log_testdisk_validation(
    disk_name: &str,
    result: &crate::verification::testdisk::TestDiskResult,
) {
    trace_log::log_testdisk_validation(disk_name, result);
}

/// Enregistre le verdict disque dans le journal (message court pour compatibilité).
/// Ligne horodatée dans le journal, sans verdict ni changement d'état.
pub fn log_note(disk_name: &str, message: &str) {
    trace_log::log_timestamped(&format!("[{disk_name}] {message}"));
}

pub fn log_disk_result(disk_name: &str, success: bool, message: &str) {
    if success {
        // Le verdict final « PROCÉDURE COMPLÈTE » est posé après la relecture.
        return;
    }
    let verdict = if message.contains("Arrêté") || message.contains("interrompu") {
        Verdict::Interrupted
    } else if message.contains("vérification") || message.contains("relecture") {
        Verdict::VerifyFailed
    } else {
        Verdict::EraseFailed
    };
    trace_log::set_verdict(disk_name, verdict);
    if !message.contains("vérification") && !message.contains("relecture") {
        certificate::set_disk_verify_status(disk_name, "non effectuée");
    }
}

pub fn log_disk_procedure_complete(disk_name: &str) {
    trace_log::set_verdict(disk_name, Verdict::Complete);
}

pub fn log_disk_verify_failed(disk_name: &str) {
    trace_log::set_verdict(disk_name, Verdict::VerifyFailed);
}

/// Finalise le journal et retourne le chemin de journal.log.
pub fn log_operation_end() -> Option<PathBuf> {
    trace_log::finalize_session()
}
