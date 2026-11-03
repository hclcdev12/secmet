//! Affichage de la vérification par relecture (validation du helper).
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Button, Label};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::helper_client::{JobReport, Outcome};
use crate::i18n::{self, tr, tr_in, trf, trf_in, Lang};
use crate::logger;

use super::dialogs::show_error_dialog;
use super::types::{DiskWidget, JobRecord, VerifyState};

pub(super) fn set_verification_message(label: &Label, message: &str, style: &str) {
    label.set_visible(true);
    for class in ["verify-ok", "verify-warn", "verify-fail", "verify-pending"] {
        label.remove_css_class(class);
    }
    label.add_css_class(style);
    label.set_text(message);
}

/// Libellé d'une issue d'étape.
fn outcome_label(lang: Lang, outcome: Outcome) -> &'static str {
    tr_in(
        lang,
        match outcome {
            Outcome::Success => "Succès",
            Outcome::Failed => "Échec",
            Outcome::Cancelled => "Interrompu",
        },
    )
}

/// Libellé d'un critère d'échec du moteur (code stable du helper).
fn criterion_label(lang: Lang, code: &str) -> &str {
    let key = match code {
        "FirmwareReportedFailure" => "le firmware n'a pas rendu un succès",
        "SanitizeNotCompleted" => "sanitize non achevé",
        "CanaryStillPresent" => "canari encore lisible",
        "PatternMismatch" => "motif attendu absent",
        "NotZeroed" => "bloc non remis à zéro",
        "SampleReadError" => "bloc échantillonné illisible",
        "SignatureStillPresent" => "signature résiduelle présente",
        "SecurityStillEnabled" => "mot de passe ATA encore actif",
        "CommandNotAttested" => "commande non attestée",
        other => return other,
    };
    tr_in(lang, key)
}

/// Texte détaillé de la validation d'un rapport (dialogue « Détails »),
/// dans `lang` : français pour le journal, langue courante à l'écran. Le
/// détail libre du helper (`validé`, `arrêté par l'opérateur`) est traduit
/// quand il est connu ; un message d'erreur reste tel quel.
pub(super) fn validation_details_in(lang: Lang, report: &JobReport) -> String {
    let mut lines = Vec::new();
    for step in &report.steps {
        let criterion = step
            .criterion
            .as_deref()
            .map(|c| format!(" [{}]", criterion_label(lang, c)))
            .unwrap_or_default();
        lines.push(trf_in(
            lang,
            "Étape {n} — {method} ({level}) : {outcome}{criterion}",
            &[
                ("n", &(step.index + 1).to_string()),
                ("method", &step.method),
                ("level", &step.level),
                ("outcome", outcome_label(lang, step.outcome)),
                ("criterion", &criterion),
            ],
        ));
        if !step.detail.is_empty() {
            lines.push(format!("  {}", tr_in(lang, &step.detail)));
        }
        if let Some(v) = &step.validation {
            lines.push(trf_in(
                lang,
                "  {checked} blocs relus, {anomalies} anomalie(s) : illisibles {unreadable}, canaris {canaries}, motifs {patterns}, non nuls {nonzero}",
                &[
                    ("checked", &v.checked.to_string()),
                    ("anomalies", &v.anomalies.to_string()),
                    ("unreadable", &v.read_errors.len().to_string()),
                    ("canaries", &v.canaries_intact.len().to_string()),
                    ("patterns", &v.pattern_mismatch.len().to_string()),
                    ("nonzero", &v.not_zeroed.len().to_string()),
                ],
            ));
        }
        if !step.signatures.is_empty() {
            lines.push(trf_in(
                lang,
                "  Signatures résiduelles : {list}",
                &[("list", &step.signatures.join(", "))],
            ));
        }
    }
    lines.push(trf_in(
        lang,
        "Seed de session : {seed}",
        &[("seed", &report.seed)],
    ));
    lines.join("\n")
}

/// Applique le résultat de validation du rapport final au widget et au journal.
pub(super) fn apply_job_validation(
    disk_name: &str,
    verify_label: &Label,
    verify_details_button: &Button,
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    report: &JobReport,
) {
    let last = report.steps.last();
    let validation = last.and_then(|s| s.validation.as_ref());
    let signatures = last.map(|s| s.signatures.clone()).unwrap_or_default();
    let details_fr = validation_details_in(Lang::Fr, report);
    let details = validation_details_in(i18n::current(), report);
    // `key` et ses arguments : le journal reçoit le texte français, le
    // widget la traduction.
    let (state, key, args, style, ok): (VerifyState, &str, Vec<(&str, String)>, &str, bool) =
        match (report.outcome, validation) {
            (Outcome::Success, Some(v)) => (
                VerifyState::Ok,
                "Vérification : OK — {checked} blocs relus, {anomalies} anomalie, aucune signature résiduelle",
                vec![
                    ("checked", v.checked.to_string()),
                    ("anomalies", v.anomalies.to_string()),
                ],
                "verify-ok",
                true,
            ),
            (Outcome::Success, None) => (
                VerifyState::Ok,
                "Vérification : OK (validée par le helper)",
                Vec::new(),
                "verify-ok",
                true,
            ),
            (Outcome::Cancelled, _) => (
                VerifyState::Error,
                "Vérification : non effectuée (interrompu)",
                Vec::new(),
                "verify-warn",
                false,
            ),
            (Outcome::Failed, Some(v)) if v.anomalies > 0 || !signatures.is_empty() => (
                VerifyState::Ko,
                "Vérification : KO — {anomalies} anomalie(s){signatures}",
                vec![
                    ("anomalies", v.anomalies.to_string()),
                    (
                        "signatures",
                        if signatures.is_empty() {
                            String::new()
                        } else {
                            format!(", signatures : {}", signatures.join(", "))
                        },
                    ),
                ],
                "verify-fail",
                false,
            ),
            (Outcome::Failed, _) => (
                VerifyState::Error,
                "Vérification : non effectuée (échec de l'effacement)",
                Vec::new(),
                "verify-fail",
                false,
            ),
        };
    let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let substitute = |mut text: String| {
        for (name, value) in &args {
            text = text.replace(&format!("{{{name}}}"), value);
        }
        text
    };
    let message_fr = substitute(key.to_string());
    let message = trf(key, &args);
    set_verification_message(verify_label, &message, style);
    verify_details_button.set_visible(true);
    verify_details_button.set_sensitive(true);
    if report.outcome == Outcome::Success || matches!(state, VerifyState::Ko) {
        logger::log_verify_result(disk_name, ok, &message_fr, Some(&details_fr));
    }
    let mut states = job_states.borrow_mut();
    if let Some(record) = states.get_mut(disk_name) {
        record.verify_state = Some(state);
        record.verify_output = Some(details);
    }
}

pub(super) fn attach_verify_details_button(
    window: &ApplicationWindow,
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    disk_widget: &DiskWidget,
) {
    let window = window.clone();
    let disk_name = disk_widget.disk.name.clone();
    let job_states = job_states.clone();
    let button = disk_widget.verify_details_button.clone();

    button.connect_clicked(move |_| {
        let output = job_states
            .borrow()
            .get(&disk_name)
            .and_then(|record| record.verify_output.clone())
            .unwrap_or_else(|| tr("Aucun détail disponible"));

        let message = format!("{disk_name}\n{output}");
        show_error_dialog(&window, &tr("Détails de la vérification"), &message);
    });
}
