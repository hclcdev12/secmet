//! Fenêtre principale SecMet et câblage UI.
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton, HeaderBar, Label,
    Orientation, ScrolledWindow,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::app_identity::APP_ID;
use crate::app_identity::APP_NAME;
use crate::certificate;
use crate::disk::Disk;
use crate::i18n::{tr, trf};
use crate::logo;

use super::app_theme::{apply_color_scheme_class, setup_css, ui_prefers_dark_theme};
use super::auth::{authorize_for_action, run_startup_authorization};
use super::dialogs::{
    confirm_action, show_confirmation_dialog_async, show_defrozen_dialog, show_error_dialog,
    show_unlock_dialog,
};
use super::disk_list::{
    attach_select_all_sync, get_selected_disks, load_disks_into_list, update_host_notice,
    update_select_all_state, LoadOutcome,
};
use super::erase::launch_erase_operations;
use super::pdf_ui::show_pdf_generation_dialog;
use super::settings_ui::show_settings_dialog;
use super::testdisk_ui::{show_testdisk_dialog, update_testdisk_button_state, TestDiskController};
use super::types::{CondemnContext, DiskWidget, JobRecord, JobState};

/// Structure principale de l'application
pub struct SecMetApp {
    app: Application,
}

impl Default for SecMetApp {
    fn default() -> Self {
        Self::new()
    }
}

impl SecMetApp {
    pub fn new() -> Self {
        crate::i18n::set_current(crate::i18n::Lang::from_code(
            &crate::settings::load().language,
        ));
        let app = Application::builder().application_id(APP_ID).build();

        SecMetApp { app }
    }

    pub fn run(&self) {
        let app = self.app.clone();

        app.connect_activate(|app| {
            build_ui(app);
        });
        app.connect_shutdown(|_| {
            crate::helper_client::shutdown_app_session();
        });

        app.run();
    }
}

/// Construit l'interface utilisateur principale
pub(super) fn build_ui(app: &Application) {
    // Fenêtre principale
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(860)
        .default_height(620)
        .build();

    setup_css();
    apply_color_scheme_class(&window);
    if let Some(settings) = gtk4::Settings::default() {
        let window_for_theme = window.clone();
        settings.connect_gtk_application_prefer_dark_theme_notify(move |_| {
            apply_color_scheme_class(&window_for_theme);
        });
    }

    // ── HeaderBar ───────────────────────────────────────────────────────────
    let header = HeaderBar::new();
    let settings_button = Button::from_icon_name("emblem-system-symbolic");
    settings_button.set_tooltip_text(Some(&tr("Réglages")));
    // Point jaune par-dessus l'engrenage quand une mise à jour est connue.
    let settings_overlay = gtk4::Overlay::new();
    settings_overlay.set_child(Some(&settings_button));
    settings_overlay.add_overlay(&super::update_ui::indicator());
    header.pack_end(&settings_overlay);
    let settings_button_for_wiring = settings_button.clone();
    header.set_title_widget(Some(&{
        let title = trf("{app} – Effacement sécurisé", &[("app", APP_NAME)]);
        let lbl = Label::new(Some(&title));
        lbl.set_markup(&format!("<b>{title}</b>"));
        lbl
    }));

    window.set_titlebar(Some(&header));

    // ── Container principal ─────────────────────────────────────────────────
    let main_box = GtkBox::new(Orientation::Vertical, 10);
    main_box.set_margin_top(14);
    main_box.set_margin_bottom(14);
    main_box.set_margin_start(16);
    main_box.set_margin_end(16);

    // ── Bannière d'avertissement ────────────────────────────────────────────
    let warning_banner = GtkBox::new(Orientation::Horizontal, 10);
    warning_banner.add_css_class("warning-banner");

    let warning_icon = Label::new(Some("⚠"));
    warning_icon.add_css_class("warning-icon");
    warning_banner.append(&warning_icon);

    let warning_text = GtkBox::new(Orientation::Vertical, 2);
    let wl1 = Label::new(None);
    wl1.add_css_class("warning-text");
    wl1.set_markup(&tr("<b>ATTENTION :</b> Cette opération effacera <b>DÉFINITIVEMENT</b> et <b>IRRÉMÉDIABLEMENT</b> toutes les données des disques sélectionnés."));
    wl1.set_wrap(true);
    wl1.set_xalign(0.0);
    let wl2 = Label::new(Some(&tr(
        "Cette action ne peut PAS être annulée (sauf interruption immédiate du processus).",
    )));
    wl2.add_css_class("warning-text-secondary");
    wl2.set_xalign(0.0);
    wl2.set_wrap(true);
    warning_text.append(&wl1);
    warning_text.append(&wl2);
    warning_text.set_hexpand(true);
    warning_banner.append(&warning_text);

    warning_banner.set_hexpand(true);
    warning_banner.set_halign(Align::Fill);

    let brand_logo = logo::create_brand_picture(ui_prefers_dark_theme());

    let intro_row = GtkBox::new(Orientation::Horizontal, 12);
    intro_row.add_css_class("intro-row");
    intro_row.set_hexpand(true);
    intro_row.set_vexpand(false);
    intro_row.append(&brand_logo);
    intro_row.append(&warning_banner);

    main_box.append(&intro_row);

    // Avis sur l'hôte (racine non rattachée à un disque local), rempli par
    // l'inventaire ; invisible tant que rien n'est à signaler.
    let host_notice = Label::new(None);
    host_notice.add_css_class("host-notice");
    host_notice.set_wrap(true);
    host_notice.set_xalign(0.0);
    host_notice.set_visible(false);
    main_box.append(&host_notice);
    // Bandeau du lot actif (clé USB, annexe M).
    main_box.append(&super::lot_ui::banner(&window));

    // ── Ligne sélection globale ─────────────────────────────────────────────
    let select_all_button = CheckButton::with_label(&tr("Tout sélectionner"));
    select_all_button.add_css_class("disk-checkbox");

    let selected_total_label = Label::new(Some(&tr("Total sélectionné : 0 Go")));
    selected_total_label.add_css_class("total-label");
    selected_total_label.set_halign(gtk4::Align::End);
    selected_total_label.set_hexpand(true);

    let selection_row = GtkBox::new(Orientation::Horizontal, 10);
    selection_row.set_margin_top(4);
    selection_row.set_margin_bottom(4);
    selection_row.append(&select_all_button);
    selection_row.append(&selected_total_label);
    main_box.append(&selection_row);

    // Zone scrollable pour la liste des disques
    let scrolled_window = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .build();
    // Laisser la liste occuper l'espace vertical disponible (sans propagate_natural_height,
    // qui provoque des rectangles invalides quand le contenu grandit dynamiquement).
    scrolled_window.set_vexpand(true);

    let disk_list_box = GtkBox::new(Orientation::Vertical, 10);
    scrolled_window.set_child(Some(&disk_list_box));
    main_box.append(&scrolled_window);

    let smart_auth_ok = Rc::new(Cell::new(false));

    let loading_label = Label::new(Some(&tr("Authentification administrateur en cours…")));
    loading_label.set_wrap(true);
    disk_list_box.append(&loading_label);

    let disk_widgets = Rc::new(RefCell::new(Vec::new()));
    let select_all_updating = Rc::new(Cell::new(false));
    let job_states: Rc<RefCell<HashMap<String, JobRecord>>> = Rc::new(RefCell::new(HashMap::new()));

    // ── Bouton de lancement (bas droite) ────────────────────────────────────
    let start_button = Button::builder()
        .label(tr("Lancer la suppression sécurisée"))
        .build();
    start_button.add_css_class("destructive-action");

    let retry_failed_button = Button::with_label(&tr("Relancer les échoués"));
    retry_failed_button.set_sensitive(false);

    let defrozen_button = Button::with_label(&tr("Dé-frozen"));
    let unlock_button = Button::with_label(&tr("Déverrouiller"));
    unlock_button.set_tooltip_text(Some(&tr(
        "Déverrouiller un disque ATA Security avec un mot de passe connu",
    )));
    let operations_active: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    {
        let window = window.clone();
        let operations_active = operations_active.clone();
        settings_button_for_wiring
            .connect_clicked(move |_| show_settings_dialog(&window, operations_active.get()));
    }
    super::update_ui::start_background();

    let pdf_button = Button::with_label(&tr("Générer le PDF"));
    pdf_button.set_sensitive(false);

    let testdisk_button = Button::with_label("TestDisk");
    testdisk_button.set_sensitive(false);

    let refresh_button = Button::with_label(&tr("Rafraîchir"));
    // Activé une fois le premier inventaire rendu : un clic pendant
    // l'authentification lancerait un second chargement.
    refresh_button.set_sensitive(false);

    let global_status_label = Label::new(Some(""));
    // Barre de boutons : un message long (lot, PDF) ne doit pas élargir la
    // fenêtre au-delà de l'écran ; coupé par des points de suspension.
    global_status_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    global_status_label.set_max_width_chars(40);

    let bottom_row = GtkBox::new(Orientation::Horizontal, 8);
    bottom_row.set_margin_top(10);
    bottom_row.append(&refresh_button);
    bottom_row.append(&global_status_label);
    let spacer_bottom = GtkBox::new(Orientation::Horizontal, 0);
    spacer_bottom.set_hexpand(true);
    bottom_row.append(&spacer_bottom);
    bottom_row.append(&testdisk_button);
    bottom_row.append(&pdf_button);
    bottom_row.append(&retry_failed_button);
    bottom_row.append(&unlock_button);
    bottom_row.append(&defrozen_button);
    bottom_row.append(&start_button);

    main_box.append(&bottom_row);

    window.set_child(Some(&main_box));

    // Gestion du bouton rafraîchissement
    let disk_list_box_clone = disk_list_box.clone();
    let disk_widgets_clone2 = disk_widgets.clone();
    let select_all_clone = select_all_button.clone();
    let select_all_updating_clone = select_all_updating.clone();
    let window_for_refresh = window.clone();
    let job_states_for_refresh = job_states.clone();
    let smart_auth_ok_for_refresh = smart_auth_ok.clone();
    let retry_failed_button_for_refresh = retry_failed_button.clone();
    let selected_total_label_for_refresh = selected_total_label.clone();
    let pdf_button_for_refresh = pdf_button.clone();
    let testdisk_button_for_refresh = testdisk_button.clone();
    let refresh_button_for_refresh = refresh_button.clone();
    let host_notice_for_refresh = host_notice.clone();
    let global_status_for_pdf = global_status_label.clone();
    let operations_active_for_refresh = operations_active.clone();

    let window_for_pdf = window.clone();
    let smart_auth_ok_for_pdf = smart_auth_ok.clone();
    pdf_button.connect_clicked(move |_| {
        show_pdf_generation_dialog(
            &window_for_pdf,
            &global_status_for_pdf,
            &smart_auth_ok_for_pdf,
        );
    });

    let window_for_testdisk = window.clone();
    let smart_auth_ok_for_testdisk = smart_auth_ok.clone();
    let job_states_for_testdisk = job_states.clone();
    let disk_widgets_for_testdisk = disk_widgets.clone();
    let operations_active_for_testdisk = operations_active.clone();
    let testdisk_button_for_dialog = testdisk_button.clone();
    let testdisk_controller = TestDiskController::new();
    let testdisk_controller_for_dialog = testdisk_controller.clone();
    testdisk_button.connect_clicked(move |_| {
        show_testdisk_dialog(
            &window_for_testdisk,
            &disk_widgets_for_testdisk,
            &job_states_for_testdisk,
            &smart_auth_ok_for_testdisk,
            &testdisk_button_for_dialog,
            &operations_active_for_testdisk,
            &testdisk_controller_for_dialog,
        );
    });

    // Rafraîchissement : vidage de session + rechargement de la liste.
    // Facteur en closure clonable pour pouvoir passer par une confirmation
    // quand des résultats d'effacement sont affichés.
    // « Marquer à détruire » depuis une ligne de disque : même suite qu'une
    // fin d'effacement (session, lot, PDF, relance).
    let condemn_ctx = CondemnContext {
        window: window.clone(),
        disk_widgets: disk_widgets.clone(),
        job_states: job_states.clone(),
        global_status: global_status_label.clone(),
        pdf_button: pdf_button.clone(),
        retry_failed_button: retry_failed_button.clone(),
        operations_active: operations_active.clone(),
    };

    let run_refresh = {
        let condemn = condemn_ctx.clone();
        let pdf_button = pdf_button_for_refresh.clone();
        let testdisk_button = testdisk_button_for_refresh.clone();
        let refresh_button = refresh_button_for_refresh.clone();
        let host_notice = host_notice_for_refresh.clone();
        let retry_failed_button = retry_failed_button_for_refresh.clone();
        let window = window_for_refresh.clone();
        let smart_auth_ok = smart_auth_ok_for_refresh.clone();
        let disk_list_box = disk_list_box_clone.clone();
        let disk_widgets = disk_widgets_clone2.clone();
        let job_states = job_states_for_refresh.clone();
        let select_all = select_all_clone.clone();
        let select_all_updating = select_all_updating_clone.clone();
        let selected_total_label = selected_total_label_for_refresh.clone();
        let operations_active = operations_active_for_refresh.clone();
        move || {
            certificate::clear_session();
            pdf_button.set_sensitive(false);
            retry_failed_button.set_sensitive(false);
            // Ne PAS fermer l'agent : un seul prompt pour toute la vie de
            // l'app. Le chargement rouvre la session si elle est tombée, sur
            // un fil dédié : la fenêtre reste réactive pendant l'invite.
            refresh_button.set_sensitive(false);
            let refresh_button = refresh_button.clone();
            let host_notice = host_notice.clone();
            let window_done = window.clone();
            let testdisk_button = testdisk_button.clone();
            let disk_widgets_done = disk_widgets.clone();
            let operations_active_done = operations_active.clone();
            load_disks_into_list(
                &disk_list_box,
                &disk_widgets,
                &window,
                &job_states,
                &smart_auth_ok,
                &select_all,
                &select_all_updating,
                &selected_total_label,
                &operations_active,
                &condemn,
                Box::new(move |outcome| {
                    refresh_button.set_sensitive(true);
                    match &outcome {
                        LoadOutcome::AuthFailed(e) => {
                            show_error_dialog(&window_done, &tr("Autorisation requise"), e);
                        }
                        LoadOutcome::Loaded { root_traced } => {
                            update_host_notice(&host_notice, *root_traced);
                        }
                        LoadOutcome::InventoryFailed(_) => {}
                    }
                    update_testdisk_button_state(
                        &testdisk_button,
                        &disk_widgets_done,
                        &operations_active_done,
                    );
                }),
            );
        }
    };

    // Déverrouillage ATA avec un mot de passe connu, puis rafraîchissement.
    {
        let window = window_for_refresh.clone();
        let disk_widgets = disk_widgets_clone2.clone();
        let operations_active = operations_active_for_refresh.clone();
        let smart_auth_ok = smart_auth_ok_for_refresh.clone();
        let run_refresh = run_refresh.clone();
        unlock_button.connect_clicked(move |_| {
            if operations_active.get() {
                show_error_dialog(
                    &window,
                    &tr("Opération en cours"),
                    &tr("Terminez ou attendez la fin des effacements avant de déverrouiller un disque."),
                );
                return;
            }
            if !authorize_for_action(&window, &smart_auth_ok, &tr("Autorisation requise")) {
                return;
            }
            let locked: Vec<Disk> = disk_widgets
                .borrow()
                .iter()
                .filter(|w| w.disk.has_ata_password())
                .map(|w| w.disk.clone())
                .collect();
            if locked.is_empty() {
                show_error_dialog(
                    &window,
                    &tr("Aucun disque à déverrouiller"),
                    &tr("Aucun disque de la liste ne rapporte de mot de passe ATA Security."),
                );
                return;
            }
            let run_refresh = run_refresh.clone();
            show_unlock_dialog(&window, locked, operations_active.clone(), move || {
                run_refresh()
            });
        });
    }

    refresh_button.connect_clicked(move |_| {
        if certificate::session_has_results() {
            let run_refresh = run_refresh.clone();
            confirm_action(
                &window_for_refresh,
                &tr("Perdre les résultats affichés ?"),
                &tr("Des résultats d'effacement sont affichés.\n\
                     Rafraîchir va les effacer de cet ordinateur ; les certificats PDF \
                     déjà générés sont conservés.\n\nContinuer ?"),
                run_refresh,
            );
        } else {
            run_refresh();
        }
    });

    // Gestion du clic sur le bouton
    let disk_widgets_clone = disk_widgets.clone();
    let window_clone = window.clone();
    let global_status_clone = global_status_label.clone();
    let start_button_clone = start_button.clone();
    let retry_failed_button_clone = retry_failed_button.clone();
    let refresh_button_ref = Rc::new(refresh_button.clone());
    let select_all_ref = Rc::new(select_all_button.clone());
    let selected_total_label_for_start = Rc::new(selected_total_label.clone());
    let select_all_updating_for_start = select_all_updating.clone();
    let job_states_for_start = job_states.clone();
    let smart_auth_ok_for_start = smart_auth_ok.clone();
    let pdf_button_for_start = pdf_button.clone();
    let testdisk_button_for_start = testdisk_button.clone();
    let defrozen_button_for_start = defrozen_button.clone();
    let operations_active_for_start = operations_active.clone();

    start_button.connect_clicked(move |_| {
        let selected_disks = get_selected_disks(&disk_widgets_clone.borrow());

        if selected_disks.is_empty() {
            show_error_dialog(
                &window_clone,
                &tr("Aucun disque sélectionné"),
                &tr("Veuillez sélectionner au moins un disque."),
            );
            return;
        }

        // Vérifier la sécurité des disques
        for (disk, _) in &selected_disks {
            let (can_shred, reason) = disk.disk.can_be_shredded();
            if !can_shred {
                show_error_dialog(
                    &window_clone,
                    &trf(
                        "Impossible d'effacer {disk}",
                        &[("disk", disk.disk.name.as_str())],
                    ),
                    &reason,
                );
                return;
            }
        }

        // Lot actif : machine déjà traitée ? Puis confirmation finale avec
        // callback asynchrone (GTK4 proper way).
        let disks_for_lot: Vec<Disk> = selected_disks.iter().map(|(_, d)| d.clone()).collect();
        let window_for_confirm = window_clone.clone();
        let disk_widgets_for_confirm = disk_widgets_clone.clone();
        let global_status_for_confirm = global_status_clone.clone();
        let start_button_for_confirm = start_button_clone.clone();
        let retry_failed_for_confirm = retry_failed_button_clone.clone();
        let refresh_for_confirm = refresh_button_ref.as_ref().clone();
        let select_all_for_confirm = select_all_ref.as_ref().clone();
        let select_all_updating_for_confirm = select_all_updating_for_start.clone();
        let selected_total_for_confirm = selected_total_label_for_start.as_ref().clone();
        let job_states_for_confirm = job_states_for_start.clone();
        let smart_auth_ok_for_confirm = smart_auth_ok_for_start.clone();
        let pdf_button_for_confirm = pdf_button_for_start.clone();
        let testdisk_for_confirm = testdisk_button_for_start.clone();
        let defrozen_for_confirm = defrozen_button_for_start.clone();
        let operations_active_for_confirm = operations_active_for_start.clone();
        super::lot_ui::check_before_erase(
            &window_clone,
            &disks_for_lot,
            Box::new(move || {
                show_confirmation_dialog_async(
                    window_for_confirm,
                    &selected_disks,
                    disk_widgets_for_confirm,
                    global_status_for_confirm,
                    start_button_for_confirm,
                    retry_failed_for_confirm,
                    refresh_for_confirm,
                    select_all_for_confirm,
                    select_all_updating_for_confirm,
                    selected_total_for_confirm,
                    job_states_for_confirm,
                    smart_auth_ok_for_confirm,
                    pdf_button_for_confirm,
                    testdisk_for_confirm,
                    defrozen_for_confirm,
                    operations_active_for_confirm,
                );
            }),
        );
    });

    // Dé-frozen SATA
    let window_for_defrozen = window.clone();
    let disk_widgets_for_defrozen = disk_widgets.clone();
    let operations_active_for_defrozen = operations_active.clone();
    let smart_auth_ok_for_defrozen = smart_auth_ok.clone();
    defrozen_button.connect_clicked(move |_| {
        if operations_active_for_defrozen.get() {
            show_error_dialog(
                &window_for_defrozen,
                &tr("Opération en cours"),
                &tr("Terminez ou attendez la fin des effacements avant d'utiliser Dé-frozen."),
            );
            return;
        }
        if !authorize_for_action(
            &window_for_defrozen,
            &smart_auth_ok_for_defrozen,
            &tr("Autorisation requise"),
        ) {
            return;
        }
        let frozen: Vec<Disk> = disk_widgets_for_defrozen
            .borrow()
            .iter()
            .filter(|w| w.disk.is_frozen == Some(true))
            .map(|w| w.disk.clone())
            .collect();
        show_defrozen_dialog(
            &window_for_defrozen,
            frozen,
            operations_active_for_defrozen.clone(),
        );
    });

    // Relancer les échoués
    let window_for_retry = window.clone();
    let disk_widgets_for_retry = disk_widgets.clone();
    let global_status_for_retry = global_status_label.clone();
    let start_button_for_retry = start_button.clone();
    let refresh_button_for_retry = refresh_button.clone();
    let select_all_for_retry = select_all_button.clone();
    let selected_total_label_for_retry = selected_total_label.clone();
    let select_all_updating_for_retry = select_all_updating.clone();
    let job_states_for_retry = job_states.clone();
    let retry_failed_button_for_retry = retry_failed_button.clone();
    let pdf_button_for_retry = pdf_button.clone();
    let testdisk_button_for_retry = testdisk_button.clone();
    let defrozen_button_for_retry = defrozen_button.clone();
    let operations_active_for_retry = operations_active.clone();
    let smart_auth_ok_for_retry = smart_auth_ok.clone();
    retry_failed_button.connect_clicked(move |_| {
        let failed_disks = {
            let states = job_states_for_retry.borrow();
            if states.values().any(|r| r.status == JobState::Running) {
                show_error_dialog(
                    &window_for_retry,
                    &tr("Opérations en cours"),
                    &tr("Des opérations sont déjà en cours."),
                );
                return;
            }

            let mut failed = Vec::new();
            for widget in disk_widgets_for_retry.borrow().iter() {
                if let Some(record) = states.get(&widget.disk.name) {
                    if record.status == JobState::FailedOther {
                        failed.push((widget.clone(), widget.disk.clone()));
                    }
                }
            }
            failed
        };

        if failed_disks.is_empty() {
            show_error_dialog(
                &window_for_retry,
                &tr("Aucun échec"),
                &tr("Aucun disque n'est en échec."),
            );
            return;
        }

        {
            let mut states = job_states_for_retry.borrow_mut();
            for record in states.values_mut() {
                record.verify_state = None;
                record.verify_output = None;
                record.testdisk_state = None;
                record.testdisk_output = None;
                record.testdisk_summary = None;
            }
        }

        launch_erase_operations(
            window_for_retry.clone(),
            failed_disks,
            disk_widgets_for_retry.clone(),
            global_status_for_retry.clone(),
            start_button_for_retry.clone(),
            retry_failed_button_for_retry.clone(),
            refresh_button_for_retry.clone(),
            select_all_for_retry.clone(),
            select_all_updating_for_retry.clone(),
            selected_total_label_for_retry.clone(),
            job_states_for_retry.clone(),
            smart_auth_ok_for_retry.clone(),
            pdf_button_for_retry.clone(),
            testdisk_button_for_retry.clone(),
            defrozen_button_for_retry.clone(),
            operations_active_for_retry.clone(),
        );
    });

    // Gestion du "Tout sélectionner"
    let disk_widgets_clone3 = disk_widgets.clone();
    let select_all_for_handler = select_all_button.clone();
    let select_all_updating_for_handler = select_all_updating.clone();
    let selected_total_label_for_handler = selected_total_label.clone();
    select_all_button.connect_toggled(move |btn| {
        if select_all_updating_for_handler.get() {
            return;
        }

        let widgets = disk_widgets_clone3.borrow();
        let selectable: Vec<&DiskWidget> = widgets
            .iter()
            .filter(|w| w.checkbox.is_sensitive())
            .collect();

        if selectable.is_empty() {
            select_all_updating_for_handler.set(true);
            btn.set_active(false);
            select_all_updating_for_handler.set(false);
            btn.set_sensitive(false);
            return;
        }

        if btn.is_active() {
            for widget in selectable {
                widget.checkbox.set_active(true);
            }
        } else {
            for widget in selectable {
                widget.checkbox.set_active(false);
            }
        }

        update_select_all_state(
            &disk_widgets_clone3.borrow(),
            &select_all_for_handler,
            select_all_updating_for_handler.as_ref(),
            &selected_total_label_for_handler,
        );
    });

    attach_select_all_sync(
        disk_widgets.clone(),
        select_all_button.clone(),
        select_all_updating.clone(),
        selected_total_label.clone(),
    );

    // Fermeture pendant un effacement : confirmation explicite. Le helper
    // laisse de toute façon le job courant aller à son terme.
    let operations_active_for_close = operations_active.clone();
    window.connect_close_request(move |window| {
        if !operations_active_for_close.get() {
            return gtk4::glib::Propagation::Proceed;
        }
        let window_to_close = window.clone();
        confirm_action(
            window,
            &tr("Effacement en cours"),
            &tr("Un effacement est en cours.\n\n\
                 Si vous fermez SecMet maintenant, le suivi, la vérification et le certificat \
                 de cette session seront perdus. L'opération en cours continue jusqu'à son terme \
                 dans le disque (une commande firmware ne peut pas être interrompue).\n\n\
                 Fermer quand même ?"),
            move || window_to_close.destroy(),
        );
        gtk4::glib::Propagation::Stop
    });

    // Clé USB (annexe M) : une seule fenêtre, plein écran de travail.
    if crate::paths::dedicated_data_dir() {
        window.maximize();
    }
    window.present();

    run_startup_authorization(
        &window,
        &disk_list_box,
        &disk_widgets,
        &job_states,
        &smart_auth_ok,
        &select_all_button,
        &select_all_updating,
        &selected_total_label,
        &operations_active,
        &testdisk_button,
        &refresh_button,
        &host_notice,
        &condemn_ctx,
    );
}
