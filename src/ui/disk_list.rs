//! Liste des disques, widgets, sélection globale.
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Button, CheckButton, Label, Orientation, ProgressBar,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use crate::disk::{detect_disks, DetectedDisks, Disk};
use crate::i18n::{tr, trf};

use super::enrichment::{cancel_enrichment, spawn_disk_enrichment};
use super::types::{DiskWidget, JobRecord};
use super::verify::attach_verify_details_button;

pub(super) fn create_disk_widget(disk: &Disk) -> DiskWidget {
    let (can_shred, reason) = disk.can_be_shredded();

    // ── Container carte ──────────────────────────────────────────────────────
    let main_container = GtkBox::new(Orientation::Vertical, 6);
    main_container.add_css_class("disk-card");
    main_container.set_margin_bottom(6);

    // ── Ligne 1 : checkbox + nom + badge type + [stop] + badge statut ────────
    let row1 = GtkBox::new(Orientation::Horizontal, 8);
    row1.set_valign(gtk4::Align::Center);

    let checkbox = CheckButton::new();
    checkbox.add_css_class("disk-checkbox");
    checkbox.set_sensitive(can_shred);
    checkbox.set_valign(gtk4::Align::Center);
    row1.append(&checkbox);

    let name_label = Label::new(Some(&disk.name));
    name_label.add_css_class("disk-name");
    name_label.set_xalign(0.0);
    name_label.set_valign(gtk4::Align::Center);
    row1.append(&name_label);

    // Cercle info SMART (info_button) — outline coloré selon l'état
    let info_button = Button::with_label("i");
    info_button.add_css_class("info");
    info_button.add_css_class("info-unknown");
    info_button.set_valign(gtk4::Align::Center);
    info_button.set_cursor_from_name(Some("pointer"));
    row1.append(&info_button);

    // Spacer
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    row1.append(&spacer);

    // Bouton Stop (caché au départ, petit)
    let stop_button = Button::with_label("⏹");
    stop_button.add_css_class("destructive-action");
    stop_button.set_sensitive(false);
    stop_button.set_visible(false);
    stop_button.set_valign(gtk4::Align::Center);
    row1.append(&stop_button);

    // Badge statut — toujours visible
    let status_badge = Button::with_label(&if can_shred {
        tr("INACTIF")
    } else {
        tr("INDISPONIBLE")
    });
    status_badge.add_css_class("status-badge");
    status_badge.add_css_class(if can_shred {
        "status-idle"
    } else {
        "status-failed"
    });
    status_badge.set_sensitive(false);
    status_badge.set_can_focus(false);
    status_badge.set_valign(gtk4::Align::Center);
    row1.append(&status_badge);

    main_container.append(&row1);

    // ── Ligne 2 : sous-titre (type • taille modèle) ─────────────────────────
    let subtitle = disk_subtitle_text(disk, can_shred, &reason);
    let subtitle_label = Label::new(Some(&subtitle));
    subtitle_label.add_css_class("disk-subtitle");
    subtitle_label.set_xalign(0.0);
    subtitle_label.set_margin_start(28);
    main_container.append(&subtitle_label);

    // ── Ligne 3 : barre de progression + texte droit (cachée init) ───────────
    let progress_row = GtkBox::new(Orientation::Horizontal, 8);
    progress_row.set_margin_start(28);
    progress_row.set_visible(false);

    let progress_bar = ProgressBar::new();
    progress_bar.set_show_text(false);
    progress_bar.set_hexpand(true);
    progress_bar.set_valign(gtk4::Align::Center);
    progress_row.append(&progress_bar);

    let progress_text = Label::new(None);
    progress_text.add_css_class("progress-text");
    progress_text.set_xalign(1.0);
    progress_text.set_valign(gtk4::Align::Center);
    // Un libellé long (méthode, ETA) ne doit pas élargir la fenêtre au-delà
    // de l'écran : coupé par des points de suspension au besoin.
    progress_text.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    progress_text.set_max_width_chars(48);
    progress_row.append(&progress_text);

    main_container.append(&progress_row);

    // ── Ligne 4 : vérification (cachée init) ─────────────────────────────────
    let verify_label = Label::new(None);
    verify_label.add_css_class("verify-label");
    verify_label.set_margin_start(28);
    verify_label.set_xalign(0.0);
    verify_label.set_wrap(true);
    verify_label.set_visible(false);
    main_container.append(&verify_label);

    let verify_details_button = Button::with_label(&tr("Détails vérification"));
    verify_details_button.set_margin_start(28);
    verify_details_button.set_halign(gtk4::Align::Start);
    verify_details_button.set_visible(false);
    verify_details_button.set_sensitive(false);
    main_container.append(&verify_details_button);

    // Mise à jour de la bordure de carte selon la sélection
    let container_for_check = main_container.clone();
    checkbox.connect_toggled(move |cb| {
        if cb.is_active() {
            container_for_check.add_css_class("card-selected");
        } else {
            container_for_check.remove_css_class("card-selected");
        }
    });

    DiskWidget {
        disk: disk.clone(),
        checkbox,
        subtitle_label,
        progress_bar,
        progress_text,
        progress_row,
        status_badge,
        verify_label,
        verify_details_button,
        info_button,
        stop_button,
        container: main_container,
        stop_handler: Rc::new(RefCell::new(None)),
        is_ata_locked: Rc::new(Cell::new(disk.is_locked == Some(true))),
    }
}

/// Récupère les disques sélectionnés
pub(super) fn get_selected_disks(widgets: &[DiskWidget]) -> Vec<(DiskWidget, Disk)> {
    widgets
        .iter()
        .filter(|w| w.checkbox.is_active())
        .map(|w| (w.clone(), w.disk.clone()))
        .collect()
}

pub(super) fn disk_trace_subtitle(disk: &Disk) -> String {
    let mut text = format!("{} {}", disk.kind.label(), disk.size);
    if !disk.model.is_empty() && disk.model != "Unknown" {
        text.push(' ');
        text.push_str(&disk.model);
    }
    text
}

pub(super) fn disk_subtitle_text(disk: &Disk, can_shred: bool, reason: &str) -> String {
    let media_type = disk.kind.label();
    // Parenthèse « modèle - S/N » : le S/N complète le modèle quand il est connu.
    let model_sn = if disk.serial.is_empty() {
        disk.model.clone()
    } else {
        format!("{} - {}", disk.model, disk.serial)
    };
    let mut text = if can_shred {
        format!("{} • {} ({})", media_type, disk.size, model_sn)
    } else {
        format!("{} • {} ({}) — {}", media_type, disk.size, model_sn, reason)
    };
    if disk.is_usb {
        text.push_str(&tr(" — USB (écriture hôte)"));
    }
    if disk.is_locked == Some(true) {
        text.push_str(" — LOCKED");
    } else if disk.security_enabled == Some(true) {
        text.push_str(&tr(" — mot de passe ATA actif"));
    }
    if disk.is_frozen == Some(true) {
        text.push_str(" — FROZEN");
    }
    if let Some(recovery) = &disk.recovery {
        text.push_str(&if recovery.recovered() {
            tr(" — mot de passe ATA SecMet levé")
        } else {
            tr(" — mot de passe ATA SecMet non levé")
        });
    }
    text
}

pub(super) fn set_status_badge(badge: &Button, text: &str, css_class: &str) {
    for class in [
        "status-idle",
        "status-pending",
        "status-running",
        "status-done",
        "status-stopped",
        "status-failed",
    ] {
        badge.remove_css_class(class);
    }
    badge.add_css_class(css_class);
    badge.set_label(text);
}

pub(super) fn attach_disk_info_button(window: &ApplicationWindow, disk_widget: &DiskWidget) {
    let window = window.clone();
    let disk_path = disk_widget.disk.path.clone();
    let disk_name = disk_widget.disk.name.clone();
    let locked = disk_widget.is_ata_locked.clone();
    let button = disk_widget.info_button.clone();
    let button_for_click = button.clone();
    button.connect_clicked(move |_| {
        button_for_click.set_sensitive(false);
        crate::disk_info::ui::show_disk_info_window(&window, &disk_name, &disk_path, locked.get());
        button_for_click.set_sensitive(true);
    });
}

pub(super) fn attach_select_all_sync(
    widgets: Rc<RefCell<Vec<DiskWidget>>>,
    select_all: CheckButton,
    updating: Rc<Cell<bool>>,
    selected_total_label: Label,
) {
    for widget in widgets.borrow().iter() {
        let widgets = widgets.clone();
        let select_all = select_all.clone();
        let updating = updating.clone();
        let selected_total_label = selected_total_label.clone();
        widget.checkbox.connect_toggled(move |_| {
            update_select_all_state(
                &widgets.borrow(),
                &select_all,
                updating.as_ref(),
                &selected_total_label,
            );
        });
    }

    update_select_all_state(
        &widgets.borrow(),
        &select_all,
        updating.as_ref(),
        &selected_total_label,
    );
}

pub(super) fn update_select_all_state(
    widgets: &[DiskWidget],
    select_all: &CheckButton,
    updating: &Cell<bool>,
    selected_total_label: &Label,
) {
    let selectable: Vec<&DiskWidget> = widgets
        .iter()
        .filter(|w| w.checkbox.is_sensitive())
        .collect();

    if selectable.is_empty() {
        updating.set(true);
        select_all.set_active(false);
        updating.set(false);
        select_all.set_sensitive(false);
        selected_total_label.set_text(&tr("Total sélectionné : 0 Go"));
        return;
    }

    let all_selected = selectable.iter().all(|w| w.checkbox.is_active());
    updating.set(true);
    select_all.set_active(all_selected);
    updating.set(false);
    select_all.set_sensitive(true);

    let selected_bytes = widgets
        .iter()
        .filter(|w| w.checkbox.is_active())
        .map(|w| {
            if w.disk.size_bytes > 0 {
                Some(w.disk.size_bytes)
            } else {
                parse_size_to_bytes(&w.disk.size)
            }
        })
        .map(|b| b.unwrap_or(0))
        .sum::<u64>();

    let text = if selected_bytes >= 1_000_000_000_000 {
        trf(
            "Total sélectionné : {size} To",
            &[(
                "size",
                &format!("{:.2}", selected_bytes as f64 / 1_000_000_000_000.0),
            )],
        )
    } else {
        trf(
            "Total sélectionné : {size} Go",
            &[(
                "size",
                &format!("{:.0}", selected_bytes as f64 / 1_000_000_000.0),
            )],
        )
    };
    selected_total_label.set_text(&text);
}

/// Issue d'un chargement de la liste.
pub(super) enum LoadOutcome {
    /// Liste rendue (éventuellement vide) ; `root_traced` : voir
    /// [`crate::disk::Disk::root_traced`].
    Loaded { root_traced: Option<bool> },
    /// Session ouverte mais inventaire impossible ; l'erreur est affichée
    /// dans la liste.
    InventoryFailed(String),
    /// Session administrateur refusée ou impossible.
    AuthFailed(String),
}

thread_local! {
    static LOAD_GENERATION: Cell<u64> = const { Cell::new(0) };
}

fn clear_list(disk_list_box: &GtkBox) {
    while let Some(child) = disk_list_box.first_child() {
        disk_list_box.remove(&child);
    }
}

/// Message centré dans la liste vide, avec un indicateur d'activité si `busy`.
fn show_list_notice(disk_list_box: &GtkBox, text: &str, busy: bool) {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_halign(gtk4::Align::Center);
    row.set_margin_top(24);
    if busy {
        let spinner = gtk4::Spinner::new();
        spinner.start();
        row.append(&spinner);
    }
    let label = Label::new(Some(text));
    label.set_wrap(true);
    if !busy {
        label.add_css_class("empty-state-error");
    }
    row.append(&label);
    disk_list_box.append(&row);
}

/// Vide la liste puis la recharge sans bloquer l'interface : l'ouverture de
/// la session administrateur (invite Polkit, jusqu'à plusieurs minutes) et
/// l'inventaire tournent sur un fil dédié pendant que la fenêtre reste
/// réactive ; `on_done` reçoit l'issue une fois la liste rendue. Un
/// chargement relancé entre-temps rend le précédent caduc : son résultat est
/// ignoré et son `on_done` n'est pas appelé.
#[allow(clippy::too_many_arguments)]
pub(super) fn load_disks_into_list(
    disk_list_box: &GtkBox,
    disk_widgets: &Rc<RefCell<Vec<DiskWidget>>>,
    window: &ApplicationWindow,
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    smart_auth_ok: &Rc<Cell<bool>>,
    select_all_button: &CheckButton,
    select_all_updating: &Rc<Cell<bool>>,
    selected_total_label: &Label,
    operations_active: &Rc<Cell<bool>>,
    on_done: Box<dyn FnOnce(LoadOutcome)>,
) {
    cancel_enrichment();
    clear_list(disk_list_box);
    disk_widgets.borrow_mut().clear();
    job_states.borrow_mut().clear();
    select_all_updating.set(true);
    select_all_button.set_active(false);
    select_all_updating.set(false);
    select_all_button.set_sensitive(false);
    selected_total_label.set_text(&tr("Total sélectionné : 0 Go"));
    let notice = if crate::helper_client::is_active() {
        tr("Inventaire des disques en cours…")
    } else {
        tr("Authentification administrateur en cours…")
    };
    show_list_notice(disk_list_box, &notice, true);

    let generation = LOAD_GENERATION.with(|g| {
        g.set(g.get().wrapping_add(1));
        g.get()
    });
    let (tx, rx) = mpsc::channel::<Result<DetectedDisks, LoadOutcome>>();
    std::thread::spawn(move || {
        let result = match crate::helper_client::ensure() {
            Err(e) => Err(LoadOutcome::AuthFailed(e)),
            Ok(_) => detect_disks().map_err(LoadOutcome::InventoryFailed),
        };
        let _ = tx.send(result);
    });

    let disk_list_box = disk_list_box.clone();
    let disk_widgets = disk_widgets.clone();
    let window = window.clone();
    let job_states = job_states.clone();
    let smart_auth_ok = smart_auth_ok.clone();
    let select_all_button = select_all_button.clone();
    let select_all_updating = select_all_updating.clone();
    let selected_total_label = selected_total_label.clone();
    let operations_active = operations_active.clone();
    let mut on_done = Some(on_done);
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(LoadOutcome::AuthFailed(tr("Réponse du helper absente")))
            }
        };
        if LOAD_GENERATION.with(|g| g.get()) != generation {
            return glib::ControlFlow::Break;
        }
        let outcome = render_disks(
            &disk_list_box,
            &disk_widgets,
            &window,
            &job_states,
            &smart_auth_ok,
            &select_all_button,
            &select_all_updating,
            &selected_total_label,
            &operations_active,
            result,
        );
        if let Some(done) = on_done.take() {
            done(outcome);
        }
        glib::ControlFlow::Break
    });
}

/// Rend le résultat d'un chargement : cartes, enrichissement, ou message
/// d'erreur dans la liste.
#[allow(clippy::too_many_arguments)]
fn render_disks(
    disk_list_box: &GtkBox,
    disk_widgets: &Rc<RefCell<Vec<DiskWidget>>>,
    window: &ApplicationWindow,
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    smart_auth_ok: &Rc<Cell<bool>>,
    select_all_button: &CheckButton,
    select_all_updating: &Rc<Cell<bool>>,
    selected_total_label: &Label,
    operations_active: &Rc<Cell<bool>>,
    result: Result<DetectedDisks, LoadOutcome>,
) -> LoadOutcome {
    clear_list(disk_list_box);
    let DetectedDisks { disks, root_traced } = match result {
        Ok(detected) => detected,
        Err(outcome) => {
            let text = match &outcome {
                LoadOutcome::AuthFailed(e) => {
                    eprintln!("❌ Session administrateur: {e}");
                    trf(
                        "❌ Session administrateur impossible : {error}",
                        &[("error", e.as_str())],
                    )
                }
                LoadOutcome::InventoryFailed(e) => {
                    eprintln!("❌ Inventaire disques: {e}");
                    trf(
                        "❌ Inventaire impossible : {error}",
                        &[("error", e.as_str())],
                    )
                }
                LoadOutcome::Loaded { .. } => String::new(),
            };
            show_list_notice(disk_list_box, &text, false);
            select_all_button.set_sensitive(false);
            selected_total_label.set_text(&tr("Total sélectionné : 0 Go"));
            smart_auth_ok.set(false);
            return outcome;
        }
    };
    smart_auth_ok.set(true);
    if disks.is_empty() {
        let no_disk_label = Label::new(Some(&tr("❌ Aucun disque détecté")));
        no_disk_label.add_css_class("empty-state-error");
        disk_list_box.append(&no_disk_label);
        select_all_button.set_sensitive(false);
        select_all_button.set_active(false);
        selected_total_label.set_text(&tr("Total sélectionné : 0 Go"));
        smart_auth_ok.set(crate::helper_client::is_active());
    } else {
        for disk in disks {
            let disk_widget = create_disk_widget(&disk);
            attach_disk_info_button(window, &disk_widget);
            attach_verify_details_button(window, job_states, &disk_widget);
            disk_list_box.append(&disk_widget.container);
            disk_widgets.borrow_mut().push(disk_widget);
        }
        spawn_disk_enrichment(
            disk_widgets.clone(),
            smart_auth_ok.clone(),
            select_all_button.clone(),
            select_all_updating.clone(),
            selected_total_label.clone(),
            operations_active.clone(),
        )
        .unwrap_or_else(|e| {
            eprintln!("❌ Enrichissement disques: {e}");
            smart_auth_ok.set(false);
        });
        select_all_button.set_sensitive(true);
        attach_select_all_sync(
            disk_widgets.clone(),
            select_all_button.clone(),
            select_all_updating.clone(),
            selected_total_label.clone(),
        );
    }
    LoadOutcome::Loaded { root_traced }
}

/// Avis sur l'hôte sous la bannière : visible seulement quand la racine n'a
/// pas pu être rattachée à un disque local (média live, ZFS), cas où le
/// disque système n'est protégé que par le verrou exclusif du noyau.
pub(super) fn update_host_notice(label: &Label, root_traced: Option<bool>) {
    // Mode clé live (annexe M, §M.8) : l'avis est attendu, le bandeau de lot
    // le remplace.
    if root_traced == Some(false) && !super::lot_ui::enabled() {
        label.set_text(&tr(
            "⚠ Disque système non identifié : la racine n'a pas pu être rattachée à un disque \
             local (média live ou ZFS). Le disque système n'est protégé que par le verrou \
             exclusif du noyau : vérifiez la sélection avant de lancer un effacement.",
        ));
        label.set_visible(true);
    } else {
        label.set_visible(false);
    }
}

pub(super) fn parse_size_to_bytes(size: &str) -> Option<u64> {
    let normalized = size.trim().replace(',', ".");
    if normalized.is_empty() {
        return None;
    }

    let mut number = String::new();
    let mut unit = String::new();

    for ch in normalized.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            number.push(ch);
        } else if !ch.is_whitespace() {
            unit.push(ch);
        }
    }

    let value = number.parse::<f64>().ok()?;
    let unit = unit.to_uppercase();

    let multiplier = match unit.as_str() {
        "" | "B" => 1_f64,
        "K" | "KB" => 1_000_f64,
        "M" | "MB" => 1_000_000_f64,
        "G" | "GB" => 1_000_000_000_f64,
        "T" | "TB" => 1_000_000_000_000_f64,
        "P" | "PB" => 1_000_000_000_000_000_f64,
        "KI" | "KIB" => 1_024_f64,
        "MI" | "MIB" => 1_024_f64.powi(2),
        "GI" | "GIB" => 1_024_f64.powi(3),
        "TI" | "TIB" => 1_024_f64.powi(4),
        "PI" | "PIB" => 1_024_f64.powi(5),
        _ => return None,
    };

    Some((value * multiplier) as u64)
}
