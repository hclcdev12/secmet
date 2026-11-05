//! Lancement et suivi des opérations d'effacement via le helper.
//!
//! Chaque disque a son fil : rescan (jeton frais, verdict frais), puis
//! `ExecutePlan` sur une connexion dédiée dont les événements de progression
//! sont relayés au fil GTK par un canal unique.
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Button, CheckButton, Label};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};

use secmet_helper::ipc::Event;

use crate::certificate;
use crate::disk::Disk;
use crate::helper_client::{self, CallError, JobReport, Outcome, Progress};
use crate::i18n::{tr, trf};
use crate::logger;

use super::dialogs::{show_condemn_dialog, show_error_dialog};
use super::disk_list::{
    condemn_offer, disk_trace_subtitle, locked_disk_notice, set_status_badge,
    update_select_all_state,
};
use super::pdf_ui::update_pdf_button_state;
use super::testdisk_ui::update_testdisk_button_state;
use super::types::{CondemnContext, DiskWidget, JobRecord, JobState};
use super::verify::{apply_job_validation, set_verification_message};

pub(super) fn erase_method_description(disk: &Disk, passes: u8) -> String {
    let base = disk.plan_description();
    if disk.is_ssd {
        base
    } else {
        let key = if passes > 1 {
            "{base} — {passes} passes pseudo-aléatoires + zéros"
        } else {
            "{base} — {passes} passe pseudo-aléatoire + zéros"
        };
        trf(key, &[("base", &base), ("passes", &passes.to_string())])
    }
}

type EventSender = mpsc::Sender<(String, EraseEvent)>;
type EventReceiver = Receiver<(String, EraseEvent)>;

/// Événement d'un job, routé par nom de disque.
enum EraseEvent {
    /// Jeton frais obtenu : permet l'arrêt.
    Started {
        token: String,
        method: String,
    },
    Progress(Progress),
    Step {
        step: usize,
        method: String,
        level: String,
    },
    StepDone {
        outcome: String,
        detail: String,
    },
    Done(Box<JobReport>),
    /// Échec avant ou pendant le job ; `reasons` : motifs exacts du verdict
    /// (`SecurityLocked`, `PasswordAttemptsExpired`…), vides sinon.
    Failed(CallError, Vec<String>),
}

fn map_event(event: Event) -> Option<EraseEvent> {
    match event {
        Event::Progress(p) => Some(EraseEvent::Progress(p)),
        Event::StepStarted {
            step,
            method,
            level,
        } => Some(EraseEvent::Step {
            step,
            method,
            level,
        }),
        Event::StepFinished {
            outcome, detail, ..
        } => Some(EraseEvent::StepDone { outcome, detail }),
        Event::Log { .. } | Event::Done(_) => None,
    }
}

fn run_job(disk_name: String, passes: u8, tx: EventSender) {
    let send = |event: EraseEvent| {
        let _ = tx.send((disk_name.clone(), event));
    };
    let Some(helper) = helper_client::current() else {
        send(EraseEvent::Failed(
            CallError {
                kind: "unauthenticated".into(),
                message: tr("Session administrateur absente"),
            },
            Vec::new(),
        ));
        return;
    };
    let mut preferences = helper_client::default_preferences();
    preferences.host_passes = passes;
    let scan = match helper_client::scan_device(&helper, &disk_name, preferences) {
        Ok(scan) => scan,
        Err(e) => {
            send(EraseEvent::Failed(e, Vec::new()));
            return;
        }
    };
    match scan.verdict.state.as_str() {
        "ready" => {}
        state => {
            let reasons = scan.verdict.reasons.clone();
            send(EraseEvent::Failed(
                CallError {
                    kind: state.to_string(),
                    message: format!(
                        "{} : {}{}",
                        if state == "blocked" {
                            tr("Bloqué")
                        } else {
                            tr("Refusé")
                        },
                        scan.verdict.reasons.join(", "),
                        if scan.verdict.guidance.is_empty() {
                            String::new()
                        } else {
                            trf(
                                " — consigne : {guidance}",
                                &[("guidance", &scan.verdict.guidance.join(", "))],
                            )
                        }
                    ),
                },
                reasons,
            ));
            return;
        }
    }
    let method = scan
        .verdict
        .plan
        .as_ref()
        .map(|p| p.primary.method.clone())
        .unwrap_or_default();
    send(EraseEvent::Started {
        token: scan.token.clone(),
        method,
    });
    let tx_events = tx.clone();
    let name_events = disk_name.clone();
    // Sans numéro de série, le helper attend l'empreinte d'identité.
    let confirmation = if scan.identity.serial.is_empty() {
        scan.identity.fingerprint.clone()
    } else {
        scan.identity.serial.clone()
    };
    let result = helper_client::execute_plan(
        &helper,
        &scan.token,
        &confirmation,
        preferences,
        move |event| {
            if let Some(mapped) = map_event(event) {
                let _ = tx_events.send((name_events.clone(), mapped));
            }
        },
    );
    match result {
        Ok((_accepted, report)) => send(EraseEvent::Done(Box::new(report))),
        Err(e) => send(EraseEvent::Failed(e, Vec::new())),
    }
}

/// Lance l'effacement de chaque disque sélectionné avec la préférence de
/// passes mémorisée (écriture hôte uniquement).
#[allow(clippy::too_many_arguments)]
pub(super) fn launch_erase_operations(
    window: ApplicationWindow,
    selected_disks: Vec<(DiskWidget, Disk)>,
    all_widgets: Rc<RefCell<Vec<DiskWidget>>>,
    global_status: Label,
    start_button: Button,
    retry_failed_button: Button,
    refresh_button: Button,
    select_all_button: CheckButton,
    select_all_updating: Rc<Cell<bool>>,
    selected_total_label: Label,
    job_states: Rc<RefCell<HashMap<String, JobRecord>>>,
    smart_auth_ok: Rc<Cell<bool>>,
    pdf_button: Button,
    testdisk_button: Button,
    defrozen_button: Button,
    operations_active: Rc<Cell<bool>>,
) {
    let passes = crate::settings::clamp_hdd_passes(crate::settings::load().hdd_passes);
    launch_erase_operations_with_passes(
        window,
        selected_disks,
        all_widgets,
        global_status,
        start_button,
        retry_failed_button,
        refresh_button,
        select_all_button,
        select_all_updating,
        selected_total_label,
        job_states,
        smart_auth_ok,
        pdf_button,
        testdisk_button,
        defrozen_button,
        operations_active,
        passes,
    )
}

fn format_secs_hmm(secs: u64) -> String {
    if secs < 60 {
        format!("{secs} s")
    } else if secs < 3600 {
        format!("{} min {:02}", secs / 60, secs % 60)
    } else {
        format!("{} h {:02}", secs / 3600, (secs % 3600) / 60)
    }
}

/// Réponse du helper à `Cancel`.
struct CancelOutcome {
    honored_now: bool,
}

impl CancelOutcome {
    fn from_value(value: &serde_json::Value) -> Self {
        Self {
            honored_now: value
                .get("honored_now")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }
    }
}

fn find_widget(widgets: &Rc<RefCell<Vec<DiskWidget>>>, disk_name: &str) -> Option<DiskWidget> {
    widgets
        .borrow()
        .iter()
        .find(|w| w.disk.name == disk_name)
        .cloned()
}

fn progress_text(p: &Progress) -> String {
    let mut text = format!("{} · {}", p.method, p.phase);
    if p.bytes_total > 0 || p.percent > 0 {
        text.push_str(&format!(" · {} %", p.percent));
    }
    if p.bytes_total > 0 && p.bandwidth_bps > 0 {
        text.push_str(&format!(" · {:.0} Mo/s", p.bandwidth_bps as f64 / 1e6));
    }
    match p.eta_seconds {
        Some(eta) if eta > 0 => {
            text.push_str(&trf(" · reste ~{eta}", &[("eta", &format_secs_hmm(eta))]));
        }
        Some(_) => {}
        None if p.bytes_total == 0 && p.percent == 0 => {
            text.push_str(&tr(" · en cours (durée inconnue)"));
        }
        None => {}
    }
    text
}

/// État du job d'après les motifs exacts du verdict (noms des variantes
/// `RefusalReason` et `Blocker` du moteur), jamais d'après le texte du message.
fn job_state_for_error(error: &CallError, reasons: &[String]) -> JobState {
    let has = |name: &str| reasons.iter().any(|r| r == name);
    if has("ReadOnlyMedia") {
        JobState::DestroyRequired
    } else if has("SecurityLocked") || has("PasswordAttemptsExpired") {
        JobState::BlockedLocked
    } else if has("SecurityFrozen") || error.kind == "security_frozen" {
        JobState::BlockedFrozen
    } else {
        JobState::FailedOther
    }
}

/// Sort du support et raisons pour un job jamais lancé, d'après les motifs
/// du verdict (même lecture que `secmet_core::disposition::classify_unrunnable`).
fn unrunnable_disposition(reasons: &[String]) -> (&'static str, Vec<String>) {
    let has = |name: &str| reasons.iter().any(|r| r == name);
    if has("ReadOnlyMedia") {
        return ("destroy_required", vec!["ReadOnlyMedia".into()]);
    }
    let mut codes = Vec::new();
    if has("SecurityLocked") || has("PasswordAttemptsExpired") {
        codes.push("LockedWithoutPassword".to_string());
    }
    if has("SecurityLocked")
        || has("PasswordAttemptsExpired")
        || has("SanitizeInProgress")
        || has("SanitizeFailed")
    {
        codes.push("Blocked".to_string());
    }
    if has("HostsSystem") || has("Mounted") || has("ActiveSwap") || has("VolumeMember") {
        codes.push("Protected".to_string());
    }
    if has("InspectionFailedNoFallback") || has("NoMethodAvailable") {
        codes.push("NoMethod".to_string());
    }
    ("retry", codes)
}

/// Phrase du verdict final d'un job terminé, pour le dialogue d'échec.
fn disposition_sentence(report: &JobReport) -> String {
    let reasons = certificate::reasons_label(&report.disposition_reasons);
    match report.disposition.as_str() {
        "destroy_required" => trf(
            "Destruction physique requise : {reasons}.",
            &[("reasons", &reasons)],
        ),
        "retry" => {
            let mut text = trf("À relancer : {reasons}.", &[("reasons", &reasons)]);
            if !report.disposition_guidance.is_empty() {
                text.push_str(&trf(
                    " Consigne : {guidance}.",
                    &[(
                        "guidance",
                        &certificate::guidances_label(&report.disposition_guidance),
                    )],
                ));
            }
            text
        }
        _ => String::new(),
    }
}

/// Variante explicite : `hdd_passes` vient du dialogue de confirmation.
#[allow(clippy::too_many_arguments)]
pub(super) fn launch_erase_operations_with_passes(
    window: ApplicationWindow,
    selected_disks: Vec<(DiskWidget, Disk)>,
    all_widgets: Rc<RefCell<Vec<DiskWidget>>>,
    global_status: Label,
    start_button: Button,
    retry_failed_button: Button,
    refresh_button: Button,
    select_all_button: CheckButton,
    select_all_updating: Rc<Cell<bool>>,
    selected_total_label: Label,
    job_states: Rc<RefCell<HashMap<String, JobRecord>>>,
    smart_auth_ok: Rc<Cell<bool>>,
    pdf_button: Button,
    testdisk_button: Button,
    defrozen_button: Button,
    operations_active: Rc<Cell<bool>>,
    hdd_passes: u8,
) {
    if let Err(e) = helper_client::ensure() {
        show_error_dialog(&window, &tr("Autorisation requise"), &e);
        global_status.set_markup("");
        start_button.set_sensitive(true);
        refresh_button.set_sensitive(true);
        select_all_button.set_sensitive(true);
        smart_auth_ok.set(false);
        return;
    }
    smart_auth_ok.set(true);

    operations_active.set(true);
    defrozen_button.set_sensitive(false);
    refresh_button.set_sensitive(false);
    select_all_button.set_sensitive(false);
    retry_failed_button.set_sensitive(false);

    let selected_disk_names: Vec<String> =
        selected_disks.iter().map(|(_, d)| d.name.clone()).collect();
    let disks_for_session: Vec<Disk> = selected_disks.iter().map(|(_, d)| d.clone()).collect();
    certificate::ensure_session_for_disks(&disks_for_session);
    super::lot_ui::register_start(&disks_for_session);
    pdf_button.set_sensitive(false);
    testdisk_button.set_sensitive(false);

    for widget in all_widgets.borrow().iter() {
        if !selected_disk_names.contains(&widget.disk.name) {
            widget.checkbox.set_sensitive(false);
            widget.stop_button.set_sensitive(false);
        }
    }

    let (tx, rx): (EventSender, EventReceiver) = mpsc::channel();
    let tokens: Rc<RefCell<HashMap<String, String>>> = Rc::new(RefCell::new(HashMap::new()));
    // Disques pour lesquels un arrêt a été demandé (le bouton reste inactif).
    let cancel_requested: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));

    for (idx, (widget, disk)) in selected_disks.iter().enumerate() {
        widget.progress_row.set_visible(true);
        widget.progress_bar.set_fraction(0.0);
        widget.progress_text.set_text(&tr("Démarrage…"));
        set_status_badge(&widget.status_badge, &tr("EN ATTENTE"), "status-pending");
        widget.verify_label.set_visible(false);
        widget.verify_details_button.set_visible(false);
        widget.verify_details_button.set_sensitive(false);
        widget.checkbox.set_sensitive(false);
        widget.stop_button.set_visible(true);
        widget.stop_button.set_sensitive(false);

        // Bouton d'arrêt : Cancel avec le jeton du job, hors du fil GTK.
        let disk_name = disk.name.clone();
        let tokens_for_stop = tokens.clone();
        let all_widgets_for_stop = all_widgets.clone();
        let cancel_requested_for_stop = cancel_requested.clone();
        if let Some(previous) = widget.stop_handler.borrow_mut().take() {
            widget.stop_button.disconnect(previous);
        }
        let stop_handler_id = widget.stop_button.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            let Some(token) = tokens_for_stop.borrow().get(&disk_name).cloned() else {
                btn.set_sensitive(true);
                return;
            };
            if let Some(widget) = find_widget(&all_widgets_for_stop, &disk_name) {
                set_status_badge(&widget.status_badge, &tr("ARRÊT…"), "status-stopped");
            }
            cancel_requested_for_stop
                .borrow_mut()
                .insert(disk_name.clone());
            let (reply_tx, reply_rx) = mpsc::channel::<Result<CancelOutcome, String>>();
            std::thread::spawn(move || {
                let outcome = helper_client::current()
                    .ok_or_else(|| tr("Session administrateur absente"))
                    .and_then(|h| helper_client::cancel(&h, &token).map_err(String::from))
                    .map(|value| CancelOutcome::from_value(&value));
                let _ = reply_tx.send(outcome);
            });
            let btn = btn.clone();
            let disk_name = disk_name.clone();
            let all_widgets = all_widgets_for_stop.clone();
            let cancel_requested = cancel_requested_for_stop.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let outcome = match reply_rx.try_recv() {
                    Ok(outcome) => outcome,
                    Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => Err(tr("Réponse du helper absente")),
                };
                let Some(widget) = find_widget(&all_widgets, &disk_name) else {
                    return glib::ControlFlow::Break;
                };
                match outcome {
                    Ok(CancelOutcome { honored_now: true }) => {
                        widget
                            .progress_text
                            .set_text(&tr("Arrêt demandé — interruption à la prochaine frontière"));
                    }
                    Ok(CancelOutcome { honored_now: false }) => {
                        widget.progress_text.set_text(&tr(
                            "Arrêt demandé — la commande firmware en cours ne peut pas être \
                             interrompue : arrêt dès qu'elle rend la main",
                        ));
                    }
                    Err(e) => {
                        eprintln!("Arrêt impossible: {e}");
                        cancel_requested.borrow_mut().remove(&disk_name);
                        btn.set_sensitive(true);
                        set_status_badge(
                            &widget.status_badge,
                            &tr("ARRÊT ANNULÉ"),
                            "status-stopped",
                        );
                        logger::log_note(&disk_name, &format!("Arrêt impossible : {e}"));
                    }
                }
                glib::ControlFlow::Break
            });
        });
        *widget.stop_handler.borrow_mut() = Some(stop_handler_id);

        let job_states_for_start = job_states.clone();
        let disk_name_for_start = disk.name.clone();
        let disk_for_cert = disk.clone();
        let status_badge = widget.status_badge.clone();
        let tx_for_disk = tx.clone();
        let delay_ms = (idx as u64) * 250;
        glib::timeout_add_local(std::time::Duration::from_millis(delay_ms), move || {
            certificate::mark_disk_started(&disk_name_for_start, &disk_for_cert);
            logger::log_disk_section_begin(
                &disk_name_for_start,
                &disk_for_cert.path,
                &disk_trace_subtitle(&disk_for_cert),
            );
            job_states_for_start
                .borrow_mut()
                .entry(disk_name_for_start.clone())
                .and_modify(|record| {
                    record.status = JobState::Running;
                    record.attempts.push("Démarrage".to_string());
                })
                .or_insert(JobRecord {
                    status: JobState::Running,
                    attempts: vec!["Démarrage".to_string()],
                    verify_state: None,
                    verify_output: None,
                    testdisk_state: None,
                    testdisk_output: None,
                    testdisk_summary: None,
                });
            set_status_badge(&status_badge, &tr("EN COURS"), "status-running");
            let name = disk_name_for_start.clone();
            let tx = tx_for_disk.clone();
            std::thread::spawn(move || run_job(name, hdd_passes, tx));
            glib::ControlFlow::Break
        });
    }
    drop(tx);

    let completed = Rc::new(Cell::new(0usize));
    let total = selected_disks.len();
    let disk_names: Vec<String> = selected_disks.iter().map(|(_, d)| d.name.clone()).collect();
    logger::log_operation_start(&disk_names);

    let all_widgets_clone = all_widgets.clone();
    let completed_clone = completed.clone();
    let global_status_clone = global_status.clone();
    let start_button_clone = start_button.clone();
    let retry_failed_button_clone = retry_failed_button.clone();
    let refresh_button_clone3 = refresh_button.clone();
    let select_all_clone3 = select_all_button.clone();
    let selected_total_label_clone3 = selected_total_label.clone();
    let select_all_updating_clone3 = select_all_updating.clone();
    let selected_disk_names_for_end = selected_disk_names.clone();
    let window_clone = window.clone();
    let job_states_clone = job_states.clone();
    let pdf_button_clone = pdf_button.clone();
    let testdisk_button_clone = testdisk_button.clone();
    let defrozen_button_clone = defrozen_button.clone();
    let operations_active_clone = operations_active.clone();
    let tokens_poll = tokens.clone();
    let cancel_requested_poll = cancel_requested.clone();

    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        loop {
            let (disk_name, event) = match rx.try_recv() {
                Ok(item) => item,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            };
            let widget = all_widgets_clone
                .borrow()
                .iter()
                .find(|w| w.disk.name == disk_name)
                .cloned();
            let Some(widget) = widget else {
                continue;
            };
            match event {
                EraseEvent::Started { token, method } => {
                    tokens_poll.borrow_mut().insert(disk_name.clone(), token);
                    widget.stop_button.set_sensitive(true);
                    widget
                        .stop_button
                        .set_tooltip_text(Some(&tr("Arrêter l'effacement")));
                    logger::log_erase_start(&disk_name, &method);
                    widget.progress_text.set_text(&trf(
                        "{method} · préparation",
                        &[("method", &certificate::method_label(&method))],
                    ));
                }
                EraseEvent::Progress(p) => {
                    // Commande firmware sans pourcentage ni estimation : la
                    // barre pulse à chaque tick pour montrer l'activité.
                    if p.bytes_total == 0 && p.percent == 0 && p.eta_seconds.is_none() {
                        widget.progress_bar.pulse();
                    } else {
                        widget
                            .progress_bar
                            .set_fraction((f64::from(p.percent) / 100.0).clamp(0.0, 1.0));
                    }
                    let stopping = cancel_requested_poll.borrow().contains(&disk_name);
                    let mut text = progress_text(&p);
                    if stopping {
                        text.push_str(&tr(" · arrêt demandé"));
                    }
                    widget.progress_text.set_text(&text);
                    if stopping {
                        set_status_badge(&widget.status_badge, &tr("ARRÊT…"), "status-stopped");
                    } else {
                        set_status_badge(&widget.status_badge, &tr("EN COURS"), "status-running");
                        widget.stop_button.set_sensitive(p.cancellable);
                        widget
                            .stop_button
                            .set_tooltip_text(Some(&tr(if p.cancellable {
                                "Arrêter l'effacement"
                            } else {
                                "Commande firmware en cours : non interruptible"
                            })));
                    }
                }
                EraseEvent::Step {
                    step,
                    method,
                    level,
                } => {
                    widget.progress_bar.set_fraction(0.0);
                    widget.progress_text.set_text(&trf(
                        "Étape {n} : {method} (NIST {level})",
                        &[
                            ("n", &(step + 1).to_string()),
                            ("method", &certificate::method_label(&method)),
                            ("level", &level),
                        ],
                    ));
                    if step > 0 {
                        update_job_state(
                            &job_states_clone,
                            &disk_name,
                            JobState::Running,
                            &format!("Repli : {method}"),
                        );
                    }
                }
                EraseEvent::StepDone { outcome, detail } => {
                    if outcome != "success" {
                        update_job_state(
                            &job_states_clone,
                            &disk_name,
                            JobState::Running,
                            &format!("Étape {outcome} : {detail}"),
                        );
                    }
                }
                EraseEvent::Done(report) => {
                    completed_clone.set(completed_clone.get() + 1);
                    widget.stop_button.set_sensitive(false);
                    tokens_poll.borrow_mut().remove(&disk_name);
                    cancel_requested_poll.borrow_mut().remove(&disk_name);
                    certificate::finish_disk_job(&disk_name, &report);
                    logger::log_job_report(&disk_name, &report);
                    let detail = report
                        .steps
                        .last()
                        .map(|s| s.detail.clone())
                        .unwrap_or_default();
                    match report.outcome {
                        Outcome::Success => {
                            widget.progress_bar.set_fraction(1.0);
                            let key = if report.disposition == "destroy_recommended" {
                                "OK — {method} (NIST {level}) · destruction physique recommandée"
                            } else {
                                "OK — {method} (NIST {level})"
                            };
                            widget.progress_text.set_text(&trf(
                                key,
                                &[
                                    (
                                        "method",
                                        &certificate::method_label(
                                            report.method.as_deref().unwrap_or(""),
                                        ),
                                    ),
                                    ("level", report.achieved_level.as_deref().unwrap_or("—")),
                                ],
                            ));
                            set_status_badge(&widget.status_badge, &tr("TERMINÉ"), "status-done");
                            update_job_state(
                                &job_states_clone,
                                &disk_name,
                                JobState::Success,
                                "Succès",
                            );
                        }
                        Outcome::Cancelled => {
                            widget.progress_text.set_text(&tr("Interrompu"));
                            set_status_badge(&widget.status_badge, &tr("ARRÊTÉ"), "status-stopped");
                            logger::log_disk_result(&disk_name, false, "Arrêté par l'utilisateur");
                            update_job_state(
                                &job_states_clone,
                                &disk_name,
                                JobState::Cancelled,
                                "Arrêté par l'utilisateur",
                            );
                        }
                        Outcome::Failed => {
                            widget.progress_text.set_text(&tr("Échec"));
                            // Le sort vient du moteur : « destruction requise »
                            // n'est pas relançable, « à relancer » l'est.
                            let state = if report.disposition == "destroy_required" {
                                set_status_badge(
                                    &widget.status_badge,
                                    &tr("À DÉTRUIRE"),
                                    "status-failed",
                                );
                                JobState::DestroyRequired
                            } else {
                                set_status_badge(
                                    &widget.status_badge,
                                    &tr("ÉCHEC"),
                                    "status-failed",
                                );
                                JobState::FailedOther
                            };
                            logger::log_disk_result(&disk_name, false, &detail);
                            update_job_state(&job_states_clone, &disk_name, state, &detail);
                            show_error_dialog(
                                &window_clone,
                                &trf("Échec effacement {disk}", &[("disk", &disk_name)]),
                                &trf(
                                    "Aucune étape du plan n'a abouti.\n\n{detail}\n\n{verdict}",
                                    &[
                                        ("detail", &detail),
                                        ("verdict", &disposition_sentence(&report)),
                                    ],
                                ),
                            );
                        }
                    }
                    apply_job_validation(
                        &disk_name,
                        &widget.verify_label,
                        &widget.verify_details_button,
                        &job_states_clone,
                        &report,
                    );
                }
                EraseEvent::Failed(error, reasons) => {
                    completed_clone.set(completed_clone.get() + 1);
                    widget.stop_button.set_sensitive(false);
                    tokens_poll.borrow_mut().remove(&disk_name);
                    cancel_requested_poll.borrow_mut().remove(&disk_name);
                    widget.progress_text.set_text(&tr("Échec"));
                    let state = job_state_for_error(&error, &reasons);
                    let badge = match state {
                        JobState::BlockedFrozen => "FROZEN".to_string(),
                        JobState::BlockedLocked => "LOCKED".to_string(),
                        JobState::DestroyRequired => tr("À DÉTRUIRE"),
                        _ => tr("ÉCHEC"),
                    };
                    set_status_badge(&widget.status_badge, &badge, "status-failed");
                    let (disposition, codes) = unrunnable_disposition(&reasons);
                    certificate::finish_disk_unrunnable(
                        &disk_name,
                        &error.message,
                        disposition,
                        &codes,
                    );
                    logger::log_erase_failed(&disk_name, &error.message);
                    logger::log_disk_result(&disk_name, false, &error.message);
                    update_job_state(&job_states_clone, &disk_name, state, &error.message);
                    set_verification_message(
                        &widget.verify_label,
                        &tr("Vérification : non effectuée"),
                        "verify-fail",
                    );
                    let title = trf("Effacement impossible : {disk}", &[("disk", &disk_name)]);
                    if state == JobState::BlockedLocked {
                        // Sans mot de passe, SecMet ne contourne jamais un
                        // verrou ATA : l'opérateur peut sortir le disque du
                        // circuit, le certificat et le lot le porteront.
                        let message = format!("{}\n\n{}", error.message, locked_disk_notice());
                        let job_states = job_states_clone.clone();
                        let badge_button = widget.status_badge.clone();
                        let name = disk_name.clone();
                        show_condemn_dialog(&window_clone, &title, &message, move || {
                            certificate::condemn_disk(&name, "LockedWithoutPassword");
                            set_status_badge(&badge_button, &tr("À DÉTRUIRE"), "status-failed");
                            logger::log_disk_condemned(
                                &name,
                                "verrou ATA Security sans mot de passe",
                            );
                            update_job_state(
                                &job_states,
                                &name,
                                JobState::DestroyRequired,
                                "Marqué à détruire : verrou ATA sans mot de passe",
                            );
                        });
                    } else if state == JobState::DestroyRequired {
                        show_error_dialog(
                            &window_clone,
                            &title,
                            &format!(
                                "{}\n\n{}",
                                error.message,
                                trf(
                                    "Destruction physique requise : {reasons}.",
                                    &[("reasons", &certificate::reasons_label(&codes))]
                                )
                            ),
                        );
                    } else {
                        show_error_dialog(&window_clone, &title, &error.message);
                    }
                }
            }
        }

        let completed_count = completed_clone.get();
        if completed_count >= total {
            global_status_clone.set_markup(&trf(
                "<b>✅ Toutes les opérations sont terminées ({done}/{total})</b>",
                &[
                    ("done", &completed_count.to_string()),
                    ("total", &total.to_string()),
                ],
            ));
            start_button_clone.set_sensitive(true);
            defrozen_button_clone.set_sensitive(true);
            operations_active_clone.set(false);
            refresh_button_clone3.set_sensitive(true);
            select_all_clone3.set_sensitive(true);
            update_retry_button_state(&job_states_clone, &retry_failed_button_clone);
            for widget in all_widgets_clone.borrow().iter() {
                if !selected_disk_names_for_end.contains(&widget.disk.name) {
                    // Même règle qu'à l'inventaire : le disque système et
                    // les disques indisponibles restent non cochables.
                    widget
                        .checkbox
                        .set_sensitive(widget.disk.can_be_shredded().0);
                    // Stop ne vaut que pendant un job : réactivé à `Started`.
                    widget.stop_button.set_sensitive(false);
                }
            }
            if let Some(log_path) = logger::log_operation_end() {
                certificate::attach_session_log(log_path.display().to_string());
            }
            certificate::finalize_session();
            super::lot_ui::register_end(&window_clone, &global_status_clone);
            update_pdf_button_state(&pdf_button_clone);
            update_testdisk_button_state(
                &testdisk_button_clone,
                &all_widgets_clone,
                &operations_active_clone,
            );
            update_select_all_state(
                &all_widgets_clone.borrow(),
                &select_all_clone3,
                select_all_updating_clone3.as_ref(),
                &selected_total_label_clone3,
            );
            glib::ControlFlow::Break
        } else {
            global_status_clone.set_markup(&trf(
                "🔄 En cours... ({done}/{total})",
                &[
                    ("done", &completed_count.to_string()),
                    ("total", &total.to_string()),
                ],
            ));
            update_retry_button_state(&job_states_clone, &retry_failed_button_clone);
            glib::ControlFlow::Continue
        }
    });
}

/// « Marquer à détruire » depuis la liste : un disque que l'effacement ne
/// peut pas atteindre (verrou ATA sans mot de passe, lecture seule) sort du
/// circuit de réemploi sans qu'aucune étape soit lancée. Même suite qu'une
/// fin d'effacement : session, entrée de lot et certificats (sort
/// « destruction physique requise »), journal, bouton PDF.
pub(super) fn condemn_from_list(ctx: &CondemnContext, disk_name: &str) {
    if ctx.operations_active.get() {
        show_error_dialog(
            &ctx.window,
            &tr("Opération en cours"),
            &tr("Terminez ou attendez la fin des effacements avant de marquer un disque à détruire."),
        );
        return;
    }
    let Some(widget) = find_widget(&ctx.disk_widgets, disk_name) else {
        return;
    };
    let Some((code, notice)) = condemn_offer(&widget.disk) else {
        return;
    };
    let disk = widget.disk.clone();
    let (_, reason) = disk.can_be_shredded();
    let title = trf(
        "Sortir {disk} du circuit de réemploi ?",
        &[("disk", &disk.name)],
    );
    let message = format!("{reason}\n\n{notice}");
    let window = ctx.window.clone();
    let ctx = ctx.clone();
    let disks = vec![disk.clone()];
    super::lot_ui::check_before_erase(
        &window,
        &disks,
        Box::new(move || {
            let apply = ctx.clone();
            show_condemn_dialog(&ctx.window, &title, &message, move || {
                apply_condemnation(&apply, &widget, &disk, code, &reason);
            });
        }),
    );
}

fn apply_condemnation(
    ctx: &CondemnContext,
    widget: &DiskWidget,
    disk: &Disk,
    code: &str,
    reason: &str,
) {
    let name = disk.name.clone();
    let disks = std::slice::from_ref(disk);
    certificate::ensure_session_for_disks(disks);
    certificate::mark_disk_started(&name, disk);
    certificate::finish_disk_unrunnable(&name, reason, "destroy_required", &[code.to_string()]);
    super::lot_ui::register_start(disks);
    logger::log_operation_start(std::slice::from_ref(&name));
    logger::log_disk_section_begin(&name, &disk.path, &disk_trace_subtitle(disk));
    logger::log_disk_condemned(&name, reason);
    logger::log_disk_result(&name, false, reason);
    if let Some(log_path) = logger::log_operation_end() {
        certificate::attach_session_log(log_path.display().to_string());
    }
    certificate::finalize_session();
    super::lot_ui::register_end(&ctx.window, &ctx.global_status);

    set_status_badge(&widget.status_badge, &tr("À DÉTRUIRE"), "status-failed");
    widget.condemn_button.set_visible(false);
    widget.checkbox.set_sensitive(false);
    set_verification_message(
        &widget.verify_label,
        &tr("Vérification : non effectuée"),
        "verify-fail",
    );
    update_job_state(
        &ctx.job_states,
        &name,
        JobState::DestroyRequired,
        &format!("Marqué à détruire : {reason}"),
    );
    update_retry_button_state(&ctx.job_states, &ctx.retry_failed_button);
    update_pdf_button_state(&ctx.pdf_button);
}

pub(super) fn update_job_state(
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    disk_name: &str,
    status: JobState,
    message: &str,
) {
    let mut states = job_states.borrow_mut();
    let entry = states.entry(disk_name.to_string()).or_insert(JobRecord {
        status: JobState::Pending,
        attempts: Vec::new(),
        verify_state: None,
        verify_output: None,
        testdisk_state: None,
        testdisk_output: None,
        testdisk_summary: None,
    });
    entry.status = status;
    entry.attempts.push(message.to_string());
}

pub(super) fn update_retry_button_state(
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    retry_button: &Button,
) {
    let states = job_states.borrow();
    let has_failed = states
        .values()
        .any(|record| record.status == JobState::FailedOther);
    retry_button.set_sensitive(has_failed);
}
