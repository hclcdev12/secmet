//! Dialogues GTK (erreur, confirmation, dé-frozen, déverrouillage ATA).
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Button, ButtonsType, CheckButton, ComboBoxText, Dialog,
    DialogFlags, Label, MessageDialog, MessageType, Orientation, PasswordEntry, ResponseType,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::disk::Disk;
use crate::helper_client;
use crate::i18n::{tr, trf};
use crate::settings::{self, Settings, DEFAULT_HDD_PASSES};

use super::app_theme::apply_color_scheme_class;
use super::erase::{erase_method_description, launch_erase_operations_with_passes};
use super::types::{DiskWidget, JobRecord};

pub(super) fn show_defrozen_dialog(
    parent: &ApplicationWindow,
    frozen_disks: Vec<Disk>,
    operations_active: Rc<Cell<bool>>,
) {
    let dialog = Dialog::builder()
        .title(tr("Dé-frozen SATA"))
        .modal(true)
        .transient_for(parent)
        .build();
    apply_color_scheme_class(&dialog);

    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(16);
    content.set_margin_end(16);

    let instructions = Label::new(None);
    instructions.set_wrap(true);
    instructions.set_xalign(0.0);
    instructions.set_text(&tr(
        "Le BIOS/UEFI peut geler les commandes ATA Security (état « frozen ») au démarrage.\n\n\
         Procédure :\n\
         1. Vérifier la liste des disques gelés ci-dessous.\n\
         2. Cliquer « Mettre en veille » : la machine s'endort quelques secondes et se réveille d'elle-même ; le gel est levé sur la plupart des machines.\n\
         3. Cliquer « Rafraîchir » pour mettre à jour les cartes.\n\n\
         Si l'écran reste noir au réveil, maintenir le bouton d'alimentation pour éteindre, puis redémarrer : rien n'est perdu (le lot est conservé). Sans dégel, l'effacement par écriture (NIST Clear) reste possible ; sur un SSD qui supporte SANITIZE, le gel n'empêche pas le Purge.",
    ));
    content.append(&instructions);

    let list_label = Label::new(None);
    list_label.set_xalign(0.0);
    list_label.set_wrap(true);
    if frozen_disks.is_empty() {
        list_label.set_markup(&tr("<b>Aucun disque SATA gelé détecté.</b>"));
    } else {
        let lines: Vec<String> = frozen_disks
            .iter()
            .map(|d| format!("• {} ({}) — {}", d.name, d.path, d.model))
            .collect();
        list_label.set_text(&trf(
            "Disques gelés :\n{list}",
            &[("list", lines.join("\n").as_str())],
        ));
    }
    content.append(&list_label);

    let button_box = GtkBox::new(Orientation::Horizontal, 8);
    button_box.set_halign(gtk4::Align::End);
    button_box.set_margin_top(8);

    let close_btn = Button::with_label(&tr("Fermer"));
    let suspend_btn = Button::with_label(&tr("Mettre en veille"));
    suspend_btn.set_sensitive(!frozen_disks.is_empty() && !operations_active.get());

    button_box.append(&suspend_btn);
    button_box.append(&close_btn);
    content.append(&button_box);

    dialog.set_child(Some(&content));
    dialog.set_default_size(520, 360);

    let dialog_for_close = dialog.clone();
    close_btn.connect_clicked(move |_| {
        dialog_for_close.close();
    });

    let parent_for_err = parent.clone();
    let dialog_for_suspend = dialog.clone();
    let first_frozen = frozen_disks.first().map(|d| d.name.clone());
    suspend_btn.connect_clicked(move |btn| {
        let Some(name) = first_frozen.clone() else {
            return;
        };
        btn.set_sensitive(false);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let outcome = helper_client::current()
                .ok_or_else(|| tr("Session administrateur absente"))
                .and_then(|h| {
                    let scan =
                        helper_client::scan_device(&h, &name, helper_client::default_preferences())
                            .map_err(String::from)?;
                    helper_client::unfreeze(&h, &scan.token).map_err(String::from)
                });
            let _ = tx.send(outcome);
        });
        let parent = parent_for_err.clone();
        let dialog = dialog_for_suspend.clone();
        let btn = btn.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            match rx.try_recv() {
                Ok(Ok(outcome)) => {
                    dialog.close();
                    if outcome.still_frozen {
                        show_error_dialog(
                            &parent,
                            &tr("Dé-frozen"),
                            &tr("Le disque est encore gelé après la reprise.\n\
                                 Essayez un hot-plug SATA ou le mode AHCI dans le BIOS."),
                        );
                    }
                    glib::ControlFlow::Break
                }
                Ok(Err(e)) => {
                    btn.set_sensitive(true);
                    show_error_dialog(&parent, &tr("Échec mise en veille"), &e);
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            }
        });
    });

    dialog.present();
}

/// Déverrouillage d'un disque ATA avec un mot de passe connu de l'opérateur.
/// Le mot de passe part au helper, qui l'applique puis le lève ; il n'est
/// jamais journalisé. `on_done` est appelé après un déverrouillage réussi
/// (rafraîchissement de la liste).
pub(super) fn show_unlock_dialog<F>(
    parent: &ApplicationWindow,
    locked_disks: Vec<Disk>,
    operations_active: Rc<Cell<bool>>,
    on_done: F,
) where
    F: Fn() + 'static,
{
    let dialog = Dialog::builder()
        .title(tr("Déverrouiller un disque ATA"))
        .modal(true)
        .transient_for(parent)
        .build();
    apply_color_scheme_class(&dialog);

    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(16);
    content.set_margin_end(16);

    let instructions = Label::new(None);
    instructions.set_wrap(true);
    instructions.set_xalign(0.0);
    instructions.set_text(&tr(
        "Ce disque porte un mot de passe ATA Security (verrouillé, ou simplement actif).\n\n\
         Saisissez le mot de passe connu du propriétaire : le helper déverrouille le disque \
         puis lève ce mot de passe, afin de rendre l'effacement firmware possible. \
         Sans le bon mot de passe, le disque refuse et rien n'est modifié. Le compteur de \
         tentatives du disque peut se bloquer après cinq refus (cycle d'alimentation requis).",
    ));
    content.append(&instructions);

    let disk_combo = ComboBoxText::new();
    for disk in &locked_disks {
        disk_combo.append(
            Some(&disk.name),
            &format!("{} — {} ({})", disk.name, disk.model, disk.serial),
        );
    }
    disk_combo.set_active(Some(0));
    content.append(&disk_combo);

    let entry = PasswordEntry::new();
    entry.set_show_peek_icon(true);
    entry.set_placeholder_text(Some(&tr("Mot de passe ATA (1 à 32 caractères ASCII)")));
    content.append(&entry);

    let master_check = CheckButton::with_label(&tr("Il s'agit du mot de passe maître"));
    content.append(&master_check);

    let feedback = Label::new(None);
    feedback.set_xalign(0.0);
    feedback.set_wrap(true);
    content.append(&feedback);

    let button_box = GtkBox::new(Orientation::Horizontal, 8);
    button_box.set_halign(gtk4::Align::End);
    button_box.set_margin_top(8);
    let cancel_btn = Button::with_label(&tr("Annuler"));
    let unlock_btn = Button::with_label(&tr("Déverrouiller"));
    unlock_btn.add_css_class("suggested-action");
    unlock_btn.set_sensitive(!locked_disks.is_empty() && !operations_active.get());
    button_box.append(&cancel_btn);
    button_box.append(&unlock_btn);
    content.append(&button_box);

    dialog.set_child(Some(&content));
    dialog.set_default_size(560, 320);

    let dialog_for_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| dialog_for_cancel.close());

    let on_done = Rc::new(on_done);
    let dialog_for_unlock = dialog.clone();
    let entry_for_unlock = entry.clone();
    let combo_for_unlock = disk_combo.clone();
    let master_for_unlock = master_check.clone();
    let feedback_for_unlock = feedback.clone();
    unlock_btn.connect_clicked(move |btn| {
        let Some(name) = combo_for_unlock.active_id().map(|s| s.to_string()) else {
            return;
        };
        let password = entry_for_unlock.text().to_string();
        if password.is_empty() || password.len() > 32 || !password.is_ascii() {
            feedback_for_unlock
                .set_text(&tr("Le mot de passe doit faire 1 à 32 caractères ASCII."));
            return;
        }
        let master = master_for_unlock.is_active();
        btn.set_sensitive(false);
        feedback_for_unlock.set_text(&tr("Déverrouillage en cours…"));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let outcome = helper_client::current()
                .ok_or_else(|| tr("Session administrateur absente"))
                .and_then(|h| {
                    let scan =
                        helper_client::scan_device(&h, &name, helper_client::default_preferences())
                            .map_err(String::from)?;
                    helper_client::unlock(&h, &scan.token, &password, master).map_err(String::from)
                });
            let _ = tx.send(outcome);
        });
        let dialog = dialog_for_unlock.clone();
        let btn = btn.clone();
        let feedback = feedback_for_unlock.clone();
        let entry = entry_for_unlock.clone();
        let on_done = on_done.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            match rx.try_recv() {
                Ok(Ok(outcome)) => {
                    entry.set_text("");
                    if outcome.locked {
                        btn.set_sensitive(true);
                        feedback.set_text(&tr("Le disque est toujours verrouillé."));
                    } else {
                        dialog.close();
                        on_done();
                    }
                    glib::ControlFlow::Break
                }
                Ok(Err(e)) => {
                    btn.set_sensitive(true);
                    feedback.set_text(&trf("Refusé : {error}", &[("error", e.as_str())]));
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    btn.set_sensitive(true);
                    feedback.set_text(&tr("Réponse du helper absente."));
                    glib::ControlFlow::Break
                }
            }
        });
    });

    dialog.present();
}

pub(super) fn show_error_dialog(window: &ApplicationWindow, title: &str, message: &str) {
    let dialog = MessageDialog::new(
        Some(window),
        DialogFlags::MODAL,
        MessageType::Error,
        ButtonsType::Ok,
        message,
    );
    apply_color_scheme_class(&dialog);
    dialog.set_title(Some(title));
    let handled = Cell::new(false);
    dialog.connect_response(move |dialog, _| {
        // `close()` réémet une réponse (DeleteEvent) : une seule prise en compte.
        if handled.replace(true) {
            return;
        }
        dialog.close();
    });
    dialog.show();
}

/// Erreur bloquante : la fenêtre principale se ferme quand le dialogue est
/// acquitté, pas avant. Fermer la fenêtre d'abord détruisait le dialogue
/// (transitoire) avec elle : l'application se terminait sans que l'erreur
/// ait pu être lue.
pub(super) fn show_fatal_dialog(window: &ApplicationWindow, title: &str, message: &str) {
    let dialog = MessageDialog::new(
        Some(window),
        DialogFlags::MODAL,
        MessageType::Error,
        ButtonsType::Ok,
        message,
    );
    apply_color_scheme_class(&dialog);
    dialog.set_title(Some(title));
    let window = window.clone();
    let handled = Cell::new(false);
    dialog.connect_response(move |dialog, _| {
        if handled.replace(true) {
            return;
        }
        dialog.close();
        window.close();
    });
    dialog.show();
}

/// Question modale Oui/Non générique ; `on_confirm` est appelé si Oui.
/// La réponse par défaut est « Non » : Entrée n'accepte pas par accident.
pub(super) fn confirm_action<F>(
    parent: &ApplicationWindow,
    title: &str,
    message: &str,
    on_confirm: F,
) where
    F: FnOnce() + 'static,
{
    let dialog = MessageDialog::new(
        Some(parent),
        DialogFlags::MODAL,
        MessageType::Question,
        ButtonsType::YesNo,
        message,
    );
    apply_color_scheme_class(&dialog);
    dialog.set_title(Some(title));
    dialog.set_default_response(ResponseType::No);
    let on_confirm = RefCell::new(Some(on_confirm));
    let handled = Cell::new(false);
    dialog.connect_response(move |dialog, response| {
        if handled.replace(true) {
            return;
        }
        dialog.close();
        if response == ResponseType::Yes {
            if let Some(f) = on_confirm.borrow_mut().take() {
                f();
            }
        }
    });
    dialog.present();
}

/// Question modale Oui/Non avec traitement des deux réponses (refus compris).
/// La réponse par défaut est « Non » : Entrée n'accepte pas par accident.
pub(super) fn confirm_action_dual<F, G>(
    parent: &ApplicationWindow,
    title: &str,
    message: &str,
    on_confirm: F,
    on_decline: G,
) where
    F: FnOnce() + 'static,
    G: FnOnce() + 'static,
{
    let dialog = MessageDialog::new(
        Some(parent),
        DialogFlags::MODAL,
        MessageType::Question,
        ButtonsType::YesNo,
        message,
    );
    apply_color_scheme_class(&dialog);
    dialog.set_title(Some(title));
    dialog.set_default_response(ResponseType::No);
    let on_confirm: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(Some(Box::new(on_confirm)));
    let on_decline: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(Some(Box::new(on_decline)));
    let handled = Cell::new(false);
    dialog.connect_response(move |dialog, response| {
        // `close()` réémet DeleteEvent dans ce même gestionnaire : sans
        // garde, la seconde entrée prenait la branche « refus » après
        // l'acceptation (fermeture de l'application juste après
        // l'installation du composant).
        if handled.replace(true) {
            return;
        }
        dialog.close();
        let chosen = if response == ResponseType::Yes {
            on_confirm.borrow_mut().take()
        } else {
            on_decline.borrow_mut().take()
        };
        if let Some(f) = chosen {
            f();
        }
    });
    dialog.present();
}

#[allow(clippy::too_many_arguments)]
pub(super) fn show_confirmation_dialog_async(
    window: ApplicationWindow,
    selected_disks: &[(DiskWidget, Disk)],
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
    // Préférence mémorisée (pré-sélection du combo + récapitulatif HDD).
    let settings_loaded = settings::load();
    let current_passes = settings::clamp_hdd_passes(settings_loaded.hdd_passes);

    let disk_list = selected_disks
        .iter()
        .map(|(_, disk)| {
            format!(
                "  • {} ({}) — {}",
                disk.name,
                disk.path,
                erase_method_description(disk, current_passes)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let has_usb = selected_disks.iter().any(|(_, d)| d.is_usb);
    // Avec USB : la note se termine par un saut de ligne ; sinon, une ligne vide.
    let note = if has_usb {
        tr(
            "\n\nNote : un ou plusieurs disques sont en USB. Aucune commande firmware \
            n'est envoyée à travers un pont USB : écriture hôte (NIST Clear).\n",
        )
    } else {
        "\n".to_string()
    };

    // Le sélecteur de passes shred ne concerne que les HDD : les SSD utilisent
    // toujours la purge constructeur / overwrite (aucune passe aléatoire).
    let has_hdd = selected_disks.iter().any(|(_, d)| !d.is_ssd);

    let message = trf(
        "Vous êtes sur le point d'effacer DÉFINITIVEMENT les disques suivants :\n\n{disks}\n\
         {note}\n\
         Cette action est IRRÉVERSIBLE.\n\
         Toutes les données seront PERDUES.\n\n\
         Êtes-vous ABSOLUMENT SÛR de vouloir continuer ?",
        &[("disks", disk_list.as_str()), ("note", note.as_str())],
    );

    let dialog = MessageDialog::new(
        Some(&window),
        DialogFlags::MODAL,
        MessageType::Warning,
        ButtonsType::YesNo,
        &message,
    );
    apply_color_scheme_class(&dialog);
    dialog.set_title(Some(&tr("⚠️ CONFIRMATION FINALE")));
    // La réponse par défaut (Entrée, focus initial) est « Non » :
    // une frappe distraite ne doit jamais lancer une destruction.
    dialog.set_default_response(ResponseType::No);

    // Zone choix méthode HDD (préréglée sur la préférence mémorisée).
    let passes_combo = if has_hdd {
        let area = dialog
            .message_area()
            .downcast::<gtk4::Box>()
            .expect("message_area doit être une GtkBox");
        let row = GtkBox::new(Orientation::Vertical, 4);

        let combo = ComboBoxText::new();
        combo.append(
            Some("1"),
            &tr("Rapide — 1 passe + zéros (recommandé, NIST Clear)"),
        );
        combo.append(Some("3"), &tr("Historique — 3 passes aléatoires + zéros"));
        combo.set_active(Some(if current_passes >= 3 { 1 } else { 0 }));

        let hint = Label::new(None);
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        hint.set_markup(&tr(
            "<small>⏱ Rapide ≈ 2× moins long. Une seule passe suffit (NIST SP 800-88).\n\
             Les SSD passent par les commandes firmware (Purge) — ce choix ne les concerne pas.</small>",
        ));

        row.append(&combo);
        row.append(&hint);
        area.append(&row);
        Some(combo)
    } else {
        None
    };

    // Faire une copie des disques sélectionnés pour le callback
    let selected_disks_copy: Vec<(DiskWidget, Disk)> = selected_disks
        .iter()
        .map(|(w, d)| (w.clone(), d.clone()))
        .collect();

    // Connecter le callback de réponse
    dialog.connect_response(move |dialog, response| {
        if response == ResponseType::Yes {
            // Désactiver le bouton pendant l'opération
            start_button.set_sensitive(false);
            global_status.set_markup(&tr(
                "<span size='large' weight='bold'>🔄 Opération en cours...</span>",
            ));

            let chosen_passes = passes_combo
                .as_ref()
                .and_then(|c| c.active_id())
                .and_then(|id| id.parse::<u8>().ok())
                .map(settings::clamp_hdd_passes)
                .unwrap_or(DEFAULT_HDD_PASSES);
            if chosen_passes != current_passes {
                settings::save(&Settings {
                    hdd_passes: chosen_passes,
                    ..settings::load()
                });
            }

            launch_erase_operations_with_passes(
                window.clone(),
                selected_disks_copy.clone(),
                all_widgets.clone(),
                global_status.clone(),
                start_button.clone(),
                retry_failed_button.clone(),
                refresh_button.clone(),
                select_all_button.clone(),
                select_all_updating.clone(),
                selected_total_label.clone(),
                job_states.clone(),
                smart_auth_ok.clone(),
                pdf_button.clone(),
                testdisk_button.clone(),
                defrozen_button.clone(),
                operations_active.clone(),
                chosen_passes,
            );
        }
        // Fermer le dialogue
        dialog.close();
    });

    // Afficher le dialogue
    dialog.present();
}
