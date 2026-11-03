//! Dialogue et file d'attente TestDisk.
use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, CheckButton, Dialog, DrawingArea, Label,
    Orientation, ScrolledWindow, TextBuffer, TextView, WrapMode,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::certificate;
use crate::i18n::{tr, trf};
use crate::logger;

use super::app_theme::{apply_color_scheme_class, ui_prefers_dark_theme};
use super::dialogs::show_error_dialog;
use super::types::{DiskWidget, JobRecord, JobState, TestDiskState};

pub(super) fn create_testdisk_spinner() -> TestDiskSpinner {
    let area = DrawingArea::new();
    area.set_content_width(18);
    area.set_content_height(18);
    area.set_visible(false);
    area.set_halign(Align::Start);
    area.set_valign(Align::Center);

    let angle = Rc::new(Cell::<f64>::new(0.0));
    let angle_draw = angle.clone();
    area.set_draw_func(move |_, cr, width, height| {
        let w = width as f64;
        let h = height as f64;
        let cx = w / 2.0;
        let cy = h / 2.0;
        let radius = w.min(h) / 2.0 - 2.0;
        let start = angle_draw.get().to_radians();
        let dark = ui_prefers_dark_theme();

        cr.set_line_width(2.0);
        cr.set_line_cap(cairo::LineCap::Round);

        // Piste : lisible sur clair et sombre
        if dark {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.22);
        } else {
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.18);
        }
        cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
        let _ = cr.stroke();

        // Arc actif : bleu clair (sombre #93c5fd / clair #60a5fa)
        if dark {
            cr.set_source_rgb(0.576, 0.773, 0.992); // #93c5fd
        } else {
            cr.set_source_rgb(0.376, 0.647, 0.980); // #60a5fa
        }
        cr.arc(cx, cy, radius, start, start + std::f64::consts::PI * 1.35);
        let _ = cr.stroke();
    });

    TestDiskSpinner {
        area,
        angle,
        timer: Rc::new(RefCell::new(None)),
    }
}

pub(super) fn set_testdisk_spinner_active(spinner: &TestDiskSpinner, active: bool) {
    spinner.area.set_visible(active);
    if active {
        if spinner.timer.borrow().is_some() {
            return;
        }
        let angle = spinner.angle.clone();
        let area = spinner.area.clone();
        let timer = spinner.timer.clone();
        let id = glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
            angle.set(angle.get() + 12.0);
            area.queue_draw();
            glib::ControlFlow::Continue
        });
        *timer.borrow_mut() = Some(id);
    } else if let Some(id) = spinner.timer.borrow_mut().take() {
        id.remove();
        spinner.angle.set(0.0);
        spinner.area.queue_draw();
    }
}

#[derive(Clone)]
pub(super) struct TestDiskSpinner {
    pub(super) area: DrawingArea,
    pub(super) angle: Rc<Cell<f64>>,
    pub(super) timer: Rc<RefCell<Option<glib::SourceId>>>,
}

pub(super) fn set_testdisk_dialog_status(label: &Label, message: &str, style: &str) {
    label.set_text(message);
    for class in [
        "testdisk-idle",
        "testdisk-ok",
        "testdisk-warn",
        "testdisk-error",
        "testdisk-pending",
    ] {
        label.remove_css_class(class);
    }
    label.add_css_class("testdisk-label");
    label.add_css_class(style);
}

pub(super) fn set_testdisk_row_pending(
    pending: &Label,
    result: &Label,
    details_btn: &Button,
    spinner: &TestDiskSpinner,
) {
    set_testdisk_dialog_status(pending, &tr("En cours…"), "testdisk-pending");
    pending.set_visible(true);
    set_testdisk_spinner_active(spinner, true);
    if let Some(parent) = result.parent() {
        parent.set_visible(false);
    }
    details_btn.set_visible(false);
    details_btn.set_sensitive(false);
}

pub(super) fn set_testdisk_row_result(
    pending: &Label,
    result: &Label,
    details_btn: &Button,
    spinner: &TestDiskSpinner,
    message: &str,
    style: &str,
) {
    pending.set_visible(false);
    pending.set_text("");
    set_testdisk_spinner_active(spinner, false);
    set_testdisk_dialog_status(result, message, style);
    if let Some(parent) = result.parent() {
        parent.set_visible(true);
    }
    details_btn.set_visible(true);
    details_btn.set_sensitive(true);
}

pub(super) enum TestDiskWorkerResult {
    Ok(crate::verification::testdisk::TestDiskResult),
    Err(String),
}

pub(super) struct TestDiskOutcome {
    pub(super) auth_ok: bool,
    pub(super) result: TestDiskWorkerResult,
}

pub(super) fn run_testdisk_worker(
    disk_path: &str,
    auth_hint: bool,
    handle: &crate::verification::testdisk::TestDiskHandle,
) -> TestDiskOutcome {
    let _ = auth_hint;
    let auth = crate::helper_client::ensure().map(|_| ());
    let auth_ok = auth.is_ok();
    match auth {
        Ok(()) => match crate::verification::testdisk::run_with_handle(disk_path, handle) {
            Ok(result) => TestDiskOutcome {
                auth_ok,
                result: TestDiskWorkerResult::Ok(result),
            },
            Err(err) => TestDiskOutcome {
                auth_ok,
                result: TestDiskWorkerResult::Err(err),
            },
        },
        Err(err) => TestDiskOutcome {
            auth_ok: false,
            result: TestDiskWorkerResult::Err(err),
        },
    }
}

/// Enregistre le résultat TestDisk (job_states + logs). Retourne (libellé, classe CSS).
pub(super) fn store_testdisk_result(
    disk_name: &str,
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    outcome: &TestDiskOutcome,
) -> (String, &'static str) {
    match &outcome.result {
        TestDiskWorkerResult::Ok(result) => {
            logger::log_testdisk_validation(disk_name, result);
            let mut states = job_states.borrow_mut();
            let entry = ensure_job_record(&mut states, disk_name);
            if result.success {
                entry.testdisk_state = Some(TestDiskState::Ok);
                entry.testdisk_output = Some(crate::verification::testdisk::details_output(result));
                entry.testdisk_summary = Some(result.summary.clone());
                (
                    trf("✓ Succès — {summary}", &[("summary", &tr(&result.summary))]),
                    "testdisk-ok",
                )
            } else {
                entry.testdisk_state = Some(TestDiskState::Warn);
                entry.testdisk_output = Some(crate::verification::testdisk::details_output(result));
                entry.testdisk_summary = Some(result.summary.clone());
                (
                    trf(
                        "⚠ Attention — {summary}",
                        &[("summary", &tr(&result.summary))],
                    ),
                    "testdisk-warn",
                )
            }
        }
        TestDiskWorkerResult::Err(err) if err == crate::verification::testdisk::CANCELLED_MSG => {
            let mut states = job_states.borrow_mut();
            let entry = ensure_job_record(&mut states, disk_name);
            entry.testdisk_state = Some(TestDiskState::Cancelled);
            entry.testdisk_output = Some(err.clone());
            entry.testdisk_summary = None;
            (
                trf("⏹ Arrêté — {error}", &[("error", err)]),
                "testdisk-idle",
            )
        }
        TestDiskWorkerResult::Err(err) => {
            let mut states = job_states.borrow_mut();
            let entry = ensure_job_record(&mut states, disk_name);
            entry.testdisk_state = Some(TestDiskState::Error);
            entry.testdisk_output = Some(err.clone());
            entry.testdisk_summary = None;
            (
                trf("! Erreur — {error}", &[("error", err)]),
                "testdisk-error",
            )
        }
    }
}

pub(super) fn show_testdisk_details_dialog(
    parent: &impl IsA<gtk4::Window>,
    title: &str,
    content: &str,
) {
    let dialog = Dialog::builder()
        .title(title)
        .modal(true)
        .transient_for(parent)
        .build();
    apply_color_scheme_class(&dialog);
    dialog.set_default_size(720, 480);

    let scroll = ScrolledWindow::builder().min_content_height(400).build();
    scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);

    let buffer = TextBuffer::new(None);
    buffer.set_text(content);
    let view = TextView::with_buffer(&buffer);
    view.set_editable(false);
    view.set_monospace(true);
    view.set_wrap_mode(WrapMode::Word);
    scroll.set_child(Some(&view));

    let content_box = GtkBox::new(Orientation::Vertical, 8);
    content_box.set_margin_top(12);
    content_box.set_margin_bottom(12);
    content_box.set_margin_start(12);
    content_box.set_margin_end(12);
    content_box.append(&scroll);

    let copy_btn = Button::with_label(&tr("Copier les logs"));
    let close_btn = Button::with_label(&tr("Fermer"));
    let btn_row = GtkBox::new(Orientation::Horizontal, 8);
    btn_row.set_halign(Align::End);
    btn_row.append(&copy_btn);
    btn_row.append(&close_btn);
    content_box.append(&btn_row);

    dialog.set_child(Some(&content_box));

    let content_for_copy = content.to_string();
    copy_btn.connect_clicked(move |btn| {
        if let Some(display) = gtk4::gdk::Display::default() {
            display.clipboard().set_text(&content_for_copy);
            btn.set_label(&tr("Copié !"));
            let btn_reset = btn.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                btn_reset.set_label(&tr("Copier les logs"));
            });
        }
    });

    let dialog_for_close = dialog.clone();
    close_btn.connect_clicked(move |_| {
        dialog_for_close.close();
    });
    dialog.present();
}

pub(super) fn update_testdisk_button_state(
    testdisk_button: &Button,
    disk_widgets: &Rc<RefCell<Vec<DiskWidget>>>,
    operations_active: &Rc<Cell<bool>>,
) {
    let has_disks = disk_widgets.borrow().iter().any(|w| !w.disk.is_system);
    testdisk_button.set_sensitive(has_disks && !operations_active.get());
}

/// File TestDisk partagée : survit à la fermeture du dialogue.
pub(super) struct TestDiskController {
    running: Cell<bool>,
    cancelled: Cell<bool>,
    handle: RefCell<Option<crate::verification::testdisk::TestDiskHandle>>,
    queue: RefCell<Vec<(String, String)>>,
}

impl TestDiskController {
    pub(super) fn new() -> Rc<Self> {
        Rc::new(Self {
            running: Cell::new(false),
            cancelled: Cell::new(false),
            handle: RefCell::new(None),
            queue: RefCell::new(Vec::new()),
        })
    }

    pub(super) fn is_running(&self) -> bool {
        self.running.get()
    }

    fn stop_requested(&self) -> bool {
        self.cancelled.get()
    }

    fn request_stop(&self) -> Result<(), String> {
        self.cancelled.set(true);
        if let Some(handle) = self.handle.borrow().clone() {
            crate::verification::testdisk::stop(&handle)
        } else {
            Ok(())
        }
    }
}

fn ensure_job_record<'a>(
    states: &'a mut HashMap<String, JobRecord>,
    disk_name: &str,
) -> &'a mut JobRecord {
    states.entry(disk_name.to_string()).or_insert(JobRecord {
        status: JobState::Pending,
        attempts: Vec::new(),
        verify_state: None,
        verify_output: None,
        testdisk_state: None,
        testdisk_output: None,
        testdisk_summary: None,
    })
}

fn mark_testdisk_pending(job_states: &Rc<RefCell<HashMap<String, JobRecord>>>, disk_name: &str) {
    let mut states = job_states.borrow_mut();
    let entry = ensure_job_record(&mut states, disk_name);
    entry.testdisk_state = Some(TestDiskState::Pending);
    entry.testdisk_output = None;
    entry.testdisk_summary = None;
}

fn mark_testdisk_cancelled(job_states: &Rc<RefCell<HashMap<String, JobRecord>>>, disk_name: &str) {
    let msg = crate::verification::testdisk::CANCELLED_MSG;
    let mut states = job_states.borrow_mut();
    let entry = ensure_job_record(&mut states, disk_name);
    entry.testdisk_state = Some(TestDiskState::Cancelled);
    entry.testdisk_output = Some(msg.to_string());
    entry.testdisk_summary = None;
}

fn row_status_from_record(record: &JobRecord) -> Option<(String, &'static str)> {
    match &record.testdisk_state {
        Some(TestDiskState::Ok) => {
            let msg = record
                .testdisk_summary
                .as_deref()
                .map(|s| trf("✓ Succès — {summary}", &[("summary", &tr(s))]))
                .unwrap_or_else(|| tr("✓ Succès"));
            Some((msg, "testdisk-ok"))
        }
        Some(TestDiskState::Warn) => {
            let msg = record
                .testdisk_summary
                .as_deref()
                .map(|s| trf("⚠ Attention — {summary}", &[("summary", &tr(s))]))
                .unwrap_or_else(|| tr("⚠ Attention"));
            Some((msg, "testdisk-warn"))
        }
        Some(TestDiskState::Error) => {
            let msg = record
                .testdisk_output
                .as_deref()
                .map(|s| trf("! Erreur — {error}", &[("error", s)]))
                .unwrap_or_else(|| tr("! Erreur"));
            Some((msg, "testdisk-error"))
        }
        Some(TestDiskState::Cancelled) => {
            let msg = record
                .testdisk_output
                .as_deref()
                .map(|s| trf("⏹ Arrêté — {error}", &[("error", s)]))
                .unwrap_or_else(|| tr("⏹ Arrêté"));
            Some((msg, "testdisk-idle"))
        }
        Some(TestDiskState::Pending) | None => None,
    }
}

fn sync_testdisk_row_from_state(
    disk_name: &str,
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    pending: &Label,
    status: &Label,
    details_btn: &Button,
    spinner: &TestDiskSpinner,
) {
    let states = job_states.borrow();
    let Some(record) = states.get(disk_name) else {
        return;
    };
    match &record.testdisk_state {
        Some(TestDiskState::Pending) => {
            drop(states);
            set_testdisk_row_pending(pending, status, details_btn, spinner);
        }
        Some(_) => {
            if let Some((msg, style)) = row_status_from_record(record) {
                drop(states);
                set_testdisk_row_result(pending, status, details_btn, spinner, &msg, style);
            }
        }
        None => {}
    }
}

fn build_testdisk_recap(
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    disk_names: &[String],
) -> String {
    let states = job_states.borrow();
    let mut lines = Vec::new();
    for name in disk_names {
        if let Some(record) = states.get(name) {
            if let Some((msg, _)) = row_status_from_record(record) {
                lines.push(format!("• {name} : {msg}"));
            } else if matches!(record.testdisk_state, Some(TestDiskState::Pending)) {
                lines.push(format!("• {name} : {}", tr("En cours…")));
            }
        }
    }
    if lines.is_empty() {
        String::new()
    } else {
        trf(
            "Terminé ({count}) :\n{lines}",
            &[
                ("count", &lines.len().to_string()),
                ("lines", &lines.join("\n")),
            ],
        )
    }
}

/// Démarre (ou poursuit) la file TestDisk hors dialogue.
/// Retourne Err si le pkexec agent échoue (un seul prompt pour toute la file).
fn start_testdisk_background(
    controller: Rc<TestDiskController>,
    job_states: Rc<RefCell<HashMap<String, JobRecord>>>,
    smart_auth_ok: Rc<Cell<bool>>,
    selected: Vec<(String, String)>,
) -> Result<(), String> {
    if controller.running.get() || selected.is_empty() {
        return Ok(());
    }

    crate::helper_client::ensure().inspect_err(|_| smart_auth_ok.set(false))?;
    smart_auth_ok.set(true);

    controller.cancelled.set(false);
    controller.running.set(true);
    for (name, _) in &selected {
        mark_testdisk_pending(&job_states, name);
    }
    *controller.queue.borrow_mut() = selected;
    run_testdisk_bg_next(controller, job_states, smart_auth_ok);
    Ok(())
}

fn finish_testdisk_queue(controller: &TestDiskController) {
    *controller.handle.borrow_mut() = None;
    controller.running.set(false);
}

fn run_testdisk_bg_next(
    controller: Rc<TestDiskController>,
    job_states: Rc<RefCell<HashMap<String, JobRecord>>>,
    smart_auth_ok: Rc<Cell<bool>>,
) {
    if controller.cancelled.get() {
        let remaining: Vec<String> = controller
            .queue
            .borrow_mut()
            .drain(..)
            .map(|(name, _)| name)
            .collect();
        for disk_name in remaining {
            mark_testdisk_cancelled(&job_states, &disk_name);
        }
        finish_testdisk_queue(&controller);
        return;
    }

    let next = {
        let mut q = controller.queue.borrow_mut();
        if q.is_empty() {
            None
        } else {
            Some(q.remove(0))
        }
    };

    let Some((disk_name, disk_path)) = next else {
        finish_testdisk_queue(&controller);
        return;
    };

    mark_testdisk_pending(&job_states, &disk_name);

    let auth_hint = smart_auth_ok.get();
    let handle = crate::verification::testdisk::TestDiskHandle::new();
    *controller.handle.borrow_mut() = Some(handle.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    let path_for_thread = disk_path.clone();
    std::thread::spawn(move || {
        let outcome = run_testdisk_worker(&path_for_thread, auth_hint, &handle);
        let _ = tx.send(outcome);
    });

    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        match rx.try_recv() {
            Ok(outcome) => {
                smart_auth_ok.set(outcome.auth_ok);
                let _ = store_testdisk_result(&disk_name, &job_states, &outcome);
                *controller.handle.borrow_mut() = None;
                run_testdisk_bg_next(
                    controller.clone(),
                    job_states.clone(),
                    smart_auth_ok.clone(),
                );
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                {
                    let mut states = job_states.borrow_mut();
                    let entry = ensure_job_record(&mut states, &disk_name);
                    entry.testdisk_state = Some(TestDiskState::Error);
                    entry.testdisk_output = Some(tr("Erreur — connexion interrompue"));
                    entry.testdisk_summary = None;
                }
                *controller.handle.borrow_mut() = None;
                run_testdisk_bg_next(
                    controller.clone(),
                    job_states.clone(),
                    smart_auth_ok.clone(),
                );
                glib::ControlFlow::Break
            }
        }
    });
}

pub(super) fn show_testdisk_dialog(
    parent: &ApplicationWindow,
    disk_widgets: &Rc<RefCell<Vec<DiskWidget>>>,
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    smart_auth_ok: &Rc<Cell<bool>>,
    testdisk_button: &Button,
    operations_active: &Rc<Cell<bool>>,
    controller: &Rc<TestDiskController>,
) {
    if operations_active.get() {
        show_error_dialog(
            parent,
            &tr("Validation TestDisk"),
            &tr("Une suppression sécurisée est en cours.\n\
             Attendez la fin avant de lancer TestDisk."),
        );
        return;
    }

    if !crate::verification::testdisk::is_available() {
        show_error_dialog(
            parent,
            &tr("Validation TestDisk"),
            &tr("TestDisk est absent ou indisponible.\nInstallez : apt install testdisk"),
        );
        return;
    }

    let dialog = Dialog::builder()
        .title(tr("Validation TestDisk"))
        .modal(true)
        .transient_for(parent)
        .build();
    apply_color_scheme_class(&dialog);

    let content = GtkBox::new(Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let hint = Label::new(Some(&tr(
        "Analyse optionnelle en lecture seule (avant ou après effacement). \
         Ne remplace pas la vérification par relecture du helper. \
         Cochez les disques à analyser ; le statut et les détails restent dans cette fenêtre.",
    )));
    hint.set_wrap(true);
    content.append(&hint);

    let list_frame = GtkBox::new(Orientation::Vertical, 0);
    list_frame.add_css_class("testdisk-disk-list-frame");
    list_frame.set_vexpand(true);

    let scroll = ScrolledWindow::builder()
        .min_content_height(220)
        .vexpand(true)
        .build();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    let list_box = GtkBox::new(Orientation::Vertical, 8);
    list_box.set_margin_top(8);
    list_box.set_margin_bottom(8);
    list_box.set_margin_start(10);
    list_box.set_margin_end(10);
    scroll.set_child(Some(&list_box));
    list_frame.append(&scroll);
    content.append(&list_frame);

    // (disk_name, device_path, checkbox, pending_label, result_label, details_button, spinner)
    let mut rows: Vec<(
        String,
        String,
        CheckButton,
        Label,
        Label,
        Button,
        TestDiskSpinner,
    )> = Vec::new();
    let states_snapshot = job_states.borrow();
    let session = certificate::current_session();
    for widget in disk_widgets.borrow().iter().filter(|w| !w.disk.is_system) {
        let disk = &widget.disk;
        let mut note = disk.model.clone();
        if disk.is_locked == Some(true) {
            note.push_str(" — LOCKED");
        }
        if let Some(ref session) = session {
            if let Some(rec) = session
                .disks
                .iter()
                .find(|d| d.disk_name == disk.name && d.is_finished())
            {
                if !rec.result_label.is_empty() {
                    note.push_str(" — ");
                    note.push_str(&rec.result_label);
                }
            }
        }
        let label = format!(
            "{} — {} ({}) — {}",
            disk.name,
            disk.path,
            disk.kind.label(),
            note
        );

        let row = GtkBox::new(Orientation::Vertical, 4);
        row.add_css_class("testdisk-dialog-row");

        let disk_row = GtkBox::new(Orientation::Horizontal, 8);
        let cb = CheckButton::with_label(&label);
        cb.add_css_class("disk-checkbox");
        cb.set_active(true);
        cb.set_hexpand(true);
        cb.set_halign(Align::Start);

        let pending = Label::new(Some(""));
        pending.set_xalign(1.0);
        pending.set_visible(false);
        pending.add_css_class("testdisk-label");

        let spinner = create_testdisk_spinner();

        disk_row.append(&cb);
        disk_row.append(&pending);
        disk_row.append(&spinner.area);
        row.append(&disk_row);

        let status_row = GtkBox::new(Orientation::Horizontal, 8);
        let status = Label::new(Some(""));
        status.set_xalign(0.0);
        status.set_hexpand(true);
        status.set_wrap(true);
        status.add_css_class("testdisk-label");

        let details_btn = Button::with_label(&tr("Afficher les détails"));
        details_btn.set_visible(false);
        details_btn.set_sensitive(false);

        let mut has_result = false;
        let mut is_pending = false;
        if let Some(record) = states_snapshot.get(&disk.name) {
            match &record.testdisk_state {
                Some(TestDiskState::Pending) => {
                    is_pending = true;
                }
                Some(_) => {
                    if let Some((msg, style)) = row_status_from_record(record) {
                        set_testdisk_dialog_status(&status, &msg, style);
                        details_btn.set_visible(true);
                        details_btn.set_sensitive(true);
                        has_result = true;
                    }
                }
                None => {}
            }
        }

        status_row.append(&status);
        status_row.append(&details_btn);
        status_row.set_visible(has_result);
        row.append(&status_row);
        list_box.append(&row);

        if is_pending {
            set_testdisk_row_pending(&pending, &status, &details_btn, &spinner);
        }

        let dialog_for_details = dialog.clone();
        let disk_name_details = disk.name.clone();
        let job_states_details = job_states.clone();
        details_btn.connect_clicked(move |_| {
            let output = job_states_details
                .borrow()
                .get(&disk_name_details)
                .and_then(|record| record.testdisk_output.clone())
                .unwrap_or_else(|| tr("Aucun détail disponible"));
            show_testdisk_details_dialog(
                &dialog_for_details,
                &format!("{} — {disk_name_details}", tr("Validation TestDisk")),
                &output,
            );
        });

        rows.push((
            disk.name.clone(),
            disk.path.clone(),
            cb,
            pending,
            status,
            details_btn,
            spinner,
        ));
    }
    drop(states_snapshot);

    if rows.is_empty() {
        show_error_dialog(
            parent,
            &tr("Validation TestDisk"),
            &tr("Aucun disque disponible pour TestDisk."),
        );
        return;
    }

    let disk_names: Vec<String> = rows.iter().map(|(n, _, _, _, _, _, _)| n.clone()).collect();

    let recap_row = GtkBox::new(Orientation::Horizontal, 8);
    recap_row.set_margin_top(4);
    let global_spinner = create_testdisk_spinner();
    global_spinner.area.set_valign(Align::Start);
    let recap_label = Label::new(Some(""));
    recap_label.set_wrap(true);
    recap_label.set_xalign(0.0);
    recap_label.set_hexpand(true);
    recap_row.append(&recap_label);
    recap_row.append(&global_spinner.area);
    content.append(&recap_row);

    let button_row = GtkBox::new(Orientation::Horizontal, 8);
    button_row.set_margin_top(8);
    button_row.set_halign(gtk4::Align::End);
    let cancel_btn = Button::with_label(&tr("Annuler"));
    let stop_btn = Button::with_label(&tr("Arrêter"));
    stop_btn.add_css_class("destructive-action");
    stop_btn.set_sensitive(false);
    stop_btn.set_visible(false);
    let validate_btn = Button::with_label(&tr("Valider"));
    validate_btn.add_css_class("suggested-action");
    button_row.append(&cancel_btn);
    button_row.append(&stop_btn);
    button_row.append(&validate_btn);
    content.append(&button_row);

    dialog.set_default_size(600, 440);
    dialog.set_child(Some(&content));

    let dialog_for_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        // Fermer ne stoppe pas la file : elle continue en arrière-plan.
        dialog_for_cancel.close();
    });

    let already_running = controller.is_running();
    if already_running {
        cancel_btn.set_label(&tr("Fermer"));
        validate_btn.set_sensitive(false);
        stop_btn.set_visible(true);
        stop_btn.set_sensitive(true);
        set_testdisk_spinner_active(&global_spinner, true);
        recap_label.set_text(&tr("Analyse en cours…"));
        for (_, _, cb, _, _, _, _) in &rows {
            cb.set_sensitive(false);
        }
    }

    let controller_for_stop = controller.clone();
    let stop_btn_for_stop = stop_btn.clone();
    let recap_label_for_stop = recap_label.clone();
    stop_btn.connect_clicked(move |_| {
        stop_btn_for_stop.set_sensitive(false);
        recap_label_for_stop.set_text(&tr("Arrêt en cours…"));
        if let Err(err) = controller_for_stop.request_stop() {
            recap_label_for_stop.set_text(&trf("Arrêt : {error}", &[("error", &err)]));
        }
    });

    let parent_clone = parent.clone();
    let job_states_for_run = job_states.clone();
    let smart_auth_ok = smart_auth_ok.clone();
    let controller_for_run = controller.clone();
    let validate_btn_for_run = validate_btn.clone();
    let cancel_btn_for_run = cancel_btn.clone();
    let stop_btn_for_run = stop_btn.clone();
    let recap_label_for_run = recap_label.clone();
    let global_spinner_for_run = global_spinner.clone();
    let rows = Rc::new(rows);
    let rows_for_run = rows.clone();

    validate_btn.connect_clicked(move |_| {
        if controller_for_run.is_running() {
            return;
        }
        let selected: Vec<(String, String)> = rows_for_run
            .iter()
            .filter(|(_, _, cb, _, _, _, _)| cb.is_active())
            .map(|(name, path, _, _, _, _, _)| (name.clone(), path.clone()))
            .collect();
        if selected.is_empty() {
            show_error_dialog(
                &parent_clone,
                &tr("Validation TestDisk"),
                &tr("Sélectionnez au moins un disque."),
            );
            return;
        }

        // Un seul pkexec = démarrage de l'agent (pas de preauthorize séparé).

        if let Err(e) = start_testdisk_background(
            controller_for_run.clone(),
            job_states_for_run.clone(),
            smart_auth_ok.clone(),
            selected.clone(),
        ) {
            show_error_dialog(&parent_clone, &tr("Validation TestDisk"), &e);
            return;
        }

        validate_btn_for_run.set_sensitive(false);
        cancel_btn_for_run.set_label(&tr("Fermer"));
        stop_btn_for_run.set_visible(true);
        stop_btn_for_run.set_sensitive(true);
        set_testdisk_spinner_active(&global_spinner_for_run, true);
        recap_label_for_run.set_text(&tr("Analyse en cours…"));

        for (name, _, cb, pending, status, details, spinner) in rows_for_run.iter() {
            cb.set_sensitive(false);
            if selected.iter().any(|(n, _)| n == name) {
                set_testdisk_row_pending(pending, status, details, spinner);
            }
        }
    });

    // Poll job_states pour synchroniser l'UI (réouverture ou dialogue laissé ouvert).
    let dialog_alive = Rc::new(Cell::new(true));
    let dialog_alive_close = dialog_alive.clone();
    dialog.connect_close_request(move |_| {
        dialog_alive_close.set(false);
        glib::Propagation::Proceed
    });

    let job_states_poll = job_states.clone();
    let controller_poll = controller.clone();
    let rows_poll = rows.clone();
    let recap_poll = recap_label.clone();
    let global_spinner_poll = global_spinner.clone();
    let stop_btn_poll = stop_btn.clone();
    let validate_btn_poll = validate_btn.clone();
    let cancel_btn_poll = cancel_btn.clone();
    let testdisk_button_poll = testdisk_button.clone();
    let disk_widgets_poll = disk_widgets.clone();
    let operations_active_poll = operations_active.clone();
    let disk_names_poll = disk_names;
    let mut was_running = already_running;

    glib::timeout_add_local(std::time::Duration::from_millis(150), move || {
        if !dialog_alive.get() {
            return glib::ControlFlow::Break;
        }

        for (name, _, _, pending, status, details, spinner) in rows_poll.iter() {
            sync_testdisk_row_from_state(name, &job_states_poll, pending, status, details, spinner);
        }

        let running = controller_poll.is_running();
        set_testdisk_spinner_active(&global_spinner_poll, running);

        if running {
            cancel_btn_poll.set_label(&tr("Fermer"));
            validate_btn_poll.set_sensitive(false);
            stop_btn_poll.set_visible(true);
            if controller_poll.stop_requested() {
                stop_btn_poll.set_sensitive(false);
            } else {
                stop_btn_poll.set_sensitive(true);
                recap_poll.set_text(&tr("Analyse en cours…"));
            }
            for (_, _, cb, _, _, _, _) in rows_poll.iter() {
                cb.set_sensitive(false);
            }
        } else if was_running {
            stop_btn_poll.set_sensitive(false);
            stop_btn_poll.set_visible(false);
            let recap = build_testdisk_recap(&job_states_poll, &disk_names_poll);
            if !recap.is_empty() {
                recap_poll.set_text(&recap);
            }
            validate_btn_poll.set_sensitive(true);
            for (_, _, cb, _, _, _, _) in rows_poll.iter() {
                cb.set_sensitive(true);
            }
            update_testdisk_button_state(
                &testdisk_button_poll,
                &disk_widgets_poll,
                &operations_active_poll,
            );
        }

        was_running = running;
        glib::ControlFlow::Continue
    });

    dialog.present();
}
