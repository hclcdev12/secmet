//! Génération et ouverture du certificat PDF.
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Button, ButtonsType, CheckButton, Dialog, DialogFlags, Entry,
    Label, MessageDialog, MessageType, Orientation, ResponseType, ScrolledWindow,
};
use std::cell::Cell;
use std::rc::Rc;

use crate::certificate;
use crate::i18n::{tr, trf};

use super::app_theme::apply_color_scheme_class;
use super::auth::authorize_for_action;
use super::dialogs::show_error_dialog;

pub(super) fn update_pdf_button_state(pdf_button: &Button) {
    pdf_button.set_sensitive(certificate::session_ready_for_pdf());
}

pub(super) fn show_pdf_generation_dialog(
    parent: &ApplicationWindow,
    global_status: &Label,
    smart_auth_ok: &Rc<Cell<bool>>,
) {
    let session = match certificate::current_session() {
        Some(s) if certificate::session_ready_for_pdf() => s,
        _ => {
            show_error_dialog(
                parent,
                &tr("Certificat PDF"),
                &tr("Aucune session terminée n'est disponible pour générer un certificat."),
            );
            return;
        }
    };
    // Mode lot : les certificats de la machine sont déjà produits et
    // enregistrés ; les régénérer écraserait les fichiers du lot.
    if let Some(count) = super::lot_ui::session_registered(&session.certificate_id) {
        super::lot_ui::show_already_registered_dialog(parent, count);
        return;
    }

    let dialog = Dialog::builder()
        .title(tr("Générer le certificat PDF"))
        .modal(true)
        .transient_for(parent)
        .build();
    apply_color_scheme_class(&dialog);

    let content = GtkBox::new(Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let operator_label = Label::new(Some(&tr("Nom de l'opérateur")));
    operator_label.set_halign(gtk4::Align::Start);
    content.append(&operator_label);

    let operator_entry = Entry::new();
    operator_entry.set_placeholder_text(Some(&tr("Votre nom")));
    operator_entry.set_text(&crate::settings::load().operator_name);
    operator_entry.set_hexpand(true);
    content.append(&operator_entry);

    let format_frame = GtkBox::new(Orientation::Vertical, 4);
    format_frame.add_css_class("secmet-section-frame");
    format_frame.set_margin_top(4);

    let format_label = Label::new(Some(&tr("Format du certificat")));
    format_label.set_halign(gtk4::Align::Start);
    format_label.set_margin_top(8);
    format_label.set_margin_start(10);
    format_label.set_margin_end(10);

    let format_box = GtkBox::new(Orientation::Vertical, 4);
    format_box.set_margin_start(10);
    format_box.set_margin_end(10);
    format_box.set_margin_bottom(8);
    let mode_per_disk = CheckButton::with_label(&tr("Une page par disque"));
    mode_per_disk.add_css_class("disk-checkbox");
    mode_per_disk.set_active(true);
    let mode_grouped = CheckButton::with_label(&tr("Certificat groupé (lot)"));
    mode_grouped.add_css_class("disk-checkbox");
    format_box.append(&mode_per_disk);
    format_box.append(&mode_grouped);
    format_frame.append(&format_label);
    format_frame.append(&format_box);
    content.append(&format_frame);

    let hint = Label::new(Some(&tr(
        "Cochez les disques à inclure. Individuel : une page par disque dans le PDF. \
         Groupé : un certificat unique avec tableau de lot (plusieurs pages si nécessaire).",
    )));
    hint.set_wrap(true);
    hint.set_margin_top(8);
    content.append(&hint);

    let list_frame = GtkBox::new(Orientation::Vertical, 0);
    list_frame.add_css_class("secmet-section-frame");
    list_frame.set_vexpand(true);

    let scroll = ScrolledWindow::builder()
        .min_content_height(180)
        .vexpand(true)
        .build();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    let list_box = GtkBox::new(Orientation::Vertical, 6);
    list_box.set_margin_top(8);
    list_box.set_margin_bottom(8);
    list_box.set_margin_start(10);
    list_box.set_margin_end(10);
    scroll.set_child(Some(&list_box));
    list_frame.append(&scroll);
    content.append(&list_frame);

    let mut checkboxes: Vec<(String, CheckButton)> = Vec::new();
    for rec in session.disks.iter().filter(|d| d.is_finished()) {
        let label = format!(
            "{} — {} ({}) — {}",
            rec.disk_name, rec.device_path, rec.media_type, rec.result_label
        );
        let cb = CheckButton::with_label(&label);
        cb.add_css_class("disk-checkbox");
        cb.set_active(true);
        list_box.append(&cb);
        checkboxes.push((rec.disk_name.clone(), cb));
    }

    if checkboxes.is_empty() {
        show_error_dialog(
            parent,
            &tr("Certificat PDF"),
            &tr("Aucun disque terminé dans cette session."),
        );
        return;
    }

    let button_row = GtkBox::new(Orientation::Horizontal, 8);
    button_row.set_margin_top(8);
    button_row.set_halign(gtk4::Align::End);
    let cancel_btn = Button::with_label(&tr("Annuler"));
    let generate_btn = Button::with_label(&tr("Générer le PDF"));
    generate_btn.add_css_class("suggested-action");
    button_row.append(&cancel_btn);
    button_row.append(&generate_btn);
    content.append(&button_row);

    dialog.set_default_size(520, 380);
    dialog.set_child(Some(&content));

    let parent_clone = parent.clone();
    let session_clone = session.clone();
    let global_status_clone = global_status.clone();
    let smart_auth_ok_for_generate = smart_auth_ok.clone();
    let dialog_for_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dialog_for_cancel.close();
    });

    let mode_per_disk_for_exclusive = mode_per_disk.clone();
    let mode_grouped_for_exclusive = mode_grouped.clone();
    mode_per_disk.connect_toggled(move |cb| {
        if cb.is_active() {
            mode_grouped_for_exclusive.set_active(false);
        }
    });
    mode_grouped.connect_toggled(move |cb| {
        if cb.is_active() {
            mode_per_disk_for_exclusive.set_active(false);
        }
    });

    let dialog_for_generate = dialog.clone();
    generate_btn.connect_clicked(move |_| {
        let operator_name = operator_entry.text().to_string();
        if operator_name.trim().is_empty() {
            show_error_dialog(
                &parent_clone,
                &tr("Certificat PDF"),
                &tr("Indiquez le nom de l'opérateur."),
            );
            return;
        }

        let selected: Vec<String> = checkboxes
            .iter()
            .filter(|(_, cb)| cb.is_active())
            .map(|(name, _)| name.clone())
            .collect();
        if selected.is_empty() {
            show_error_dialog(
                &parent_clone,
                &tr("Certificat PDF"),
                &tr("Sélectionnez au moins un disque."),
            );
            return;
        }

        if !authorize_for_action(
            &parent_clone,
            &smart_auth_ok_for_generate,
            &tr("Autorisation requise"),
        ) {
            return;
        }

        dialog_for_generate.close();
        global_status_clone.set_text(&tr("Génération du certificat PDF en cours…"));

        let pdf_mode = if mode_grouped.is_active() {
            certificate::CertificatePdfMode::Grouped
        } else {
            certificate::CertificatePdfMode::PerDisk
        };

        let (tx, rx) = std::sync::mpsc::channel();
        let operator_trimmed = operator_name.trim().to_string();
        let session_for_thread = session_clone.clone();
        std::thread::spawn(move || {
            let result = certificate::render_pdf_bundle(
                &session_for_thread,
                &selected,
                &operator_trimmed,
                pdf_mode,
            );
            let _ = tx.send((result, selected.len(), pdf_mode));
        });

        let parent_for_poll = parent_clone.clone();
        let global_status_for_poll = global_status_clone.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(150), move || {
            match rx.try_recv() {
                Ok((Ok(path), disk_count, mode)) => {
                    global_status_for_poll.set_text("");
                    show_pdf_created_dialog(&parent_for_poll, &path, disk_count, mode);
                    glib::ControlFlow::Break
                }
                Ok((Err(e), _, _)) => {
                    global_status_for_poll.set_text("");
                    show_error_dialog(&parent_for_poll, &tr("Échec génération PDF"), &e);
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    global_status_for_poll.set_text("");
                    glib::ControlFlow::Break
                }
            }
        });
    });

    dialog.present();
}

pub(super) fn show_pdf_created_dialog(
    parent: &ApplicationWindow,
    pdf_path: &std::path::Path,
    disk_count: usize,
    mode: certificate::CertificatePdfMode,
) {
    let args = [
        ("count", disk_count.to_string()),
        ("path", pdf_path.display().to_string()),
    ];
    let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let message = match mode {
        certificate::CertificatePdfMode::Grouped => trf(
            "Certificat PDF groupé créé ({count} disque(s) dans le lot).\n\n\
             Fichier :\n{path}\n\n\
             Un fichier JSON miroir unique pour ce lot est enregistré \
             dans le même dossier.",
            &args,
        ),
        certificate::CertificatePdfMode::PerDisk => trf(
            "Certificat PDF créé ({count} disque(s)).\n\n\
             Fichier :\n{path}\n\n\
             Des fichiers JSON miroir (métadonnées techniques) sont enregistrés \
             dans le même dossier.",
            &args,
        ),
    };
    let dialog = MessageDialog::new(
        Some(parent),
        DialogFlags::MODAL,
        MessageType::Info,
        ButtonsType::None,
        &message,
    );
    apply_color_scheme_class(&dialog);
    dialog.set_title(Some(&tr("Certificat PDF créé")));
    dialog.add_button(&tr("Fermer"), ResponseType::Close);
    dialog.add_button(&tr("Ouvrir le dossier"), ResponseType::Reject);
    dialog.add_button(&tr("Ouvrir le PDF"), ResponseType::Accept);
    dialog.set_default_response(ResponseType::Accept);

    let parent_for_errors = parent.clone();
    let pdf_path_owned = pdf_path.to_path_buf();
    dialog.connect_response(move |d, response| {
        match response {
            ResponseType::Accept => {
                if let Err(e) = certificate::open_pdf(&pdf_path_owned) {
                    show_error_dialog(
                        &parent_for_errors,
                        &tr("Ouverture du PDF"),
                        &trf(
                            "{error}\n\nChemin :\n{path}",
                            &[
                                ("error", &e),
                                ("path", &pdf_path_owned.display().to_string()),
                            ],
                        ),
                    );
                }
            }
            ResponseType::Reject => {
                if let Err(e) = certificate::open_certificates_folder() {
                    show_error_dialog(&parent_for_errors, &tr("Ouverture du dossier"), &e);
                }
            }
            _ => {}
        }
        d.close();
    });
    dialog.present();
}
