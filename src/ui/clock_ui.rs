//! Contrôle de l'horloge au démarrage, en mode clé USB (Annexe M, §M.7) :
//! si l'heure du poste est antérieure à une référence connue, l'opérateur
//! garde l'horloge (réserve sur les certificats) ou saisit la date réelle,
//! appliquée par décalage sans toucher au système.

use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Box as GtkBox, Button, Dialog, Entry, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

use chrono::{DateTime, Local};

use crate::clock::{self, ClockStatus};
use crate::i18n::{tr, trf};
use crate::lot::{Lot, LotStatus};
use crate::paths;

use super::app_theme::apply_color_scheme_class;

type Done = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

/// Références fournies par les lots de la clé : dernière entrée du lot
/// ouvert, dernier lot clôturé.
fn lot_floors() -> Vec<(DateTime<Local>, &'static str)> {
    let mut floors = Vec::new();
    let mut scan = |root: &std::path::Path, open_only: bool| {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for lot in entries
            .filter_map(|e| e.ok())
            .filter_map(|e| Lot::load(&e.path()).ok())
        {
            let s = lot.state();
            let (reason, stamps): (&'static str, Vec<&String>) = match s.status {
                LotStatus::Open if open_only => (
                    "dernière entrée du lot",
                    std::iter::once(&s.opened_at)
                        .chain(s.entries.iter().map(|e| &e.started_at))
                        .chain(s.entries.iter().filter_map(|e| e.finished_at.as_ref()))
                        .collect(),
                ),
                LotStatus::Closed if !open_only => {
                    ("dernier lot clôturé", s.closed_at.iter().collect())
                }
                _ => continue,
            };
            if let Some(latest) = stamps.iter().filter_map(|t| clock::parse_stamp(t)).max() {
                floors.push((latest, reason));
            }
        }
    };
    scan(&paths::lots_dir(), true);
    scan(&paths::archive_dir(), false);
    floors
}

/// Contrôle puis `on_done` ; dialogue seulement si l'horloge est suspecte.
pub(super) fn startup(window: &ApplicationWindow, on_done: Box<dyn FnOnce()>) {
    if !paths::dedicated_data_dir() {
        on_done();
        return;
    }
    match clock::run_check(&lot_floors()) {
        ClockStatus::Suspect { floor, reason } => show_dialog(window, floor, &reason, on_done),
        _ => on_done(),
    }
}

fn show_dialog(
    window: &ApplicationWindow,
    floor: DateTime<Local>,
    reason: &str,
    on_done: Box<dyn FnOnce()>,
) {
    let dialog = Dialog::builder()
        .title(tr("Horloge du poste"))
        .modal(true)
        .transient_for(window)
        .build();
    apply_color_scheme_class(&dialog);
    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    content.set_margin_start(18);
    content.set_margin_end(18);
    let floor_text = floor.format(clock::STAMP_FORMAT).to_string();
    let text = Label::new(Some(&trf(
        "L'horloge du poste indique {now}, ce qui est antérieur à {floor} ({reason}). Les \
         dates des certificats et l'ordre du lot en dépendent. Vous pouvez saisir la date et \
         l'heure réelles : elles seront utilisées par SecMet, sans modifier l'horloge du \
         poste, et le certificat le mentionnera.",
        &[
            ("now", &clock::stamp()),
            ("floor", &floor_text),
            ("reason", &tr(reason)),
        ],
    )));
    text.set_wrap(true);
    text.set_xalign(0.0);
    content.append(&text);
    let entry_title = Label::new(Some(&tr("Date et heure réelles (AAAA-MM-JJ HH:MM)")));
    entry_title.set_xalign(0.0);
    content.append(&entry_title);
    let entry = Entry::new();
    entry.set_text(&floor.format(clock::INPUT_FORMAT).to_string());
    content.append(&entry);
    let status = Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);
    content.append(&status);
    let buttons = GtkBox::new(Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    let keep = Button::with_label(&tr("Garder l'horloge du poste"));
    let apply = Button::with_label(&tr("Utiliser cette date"));
    apply.add_css_class("suggested-action");
    buttons.append(&keep);
    buttons.append(&apply);
    content.append(&buttons);
    dialog.set_child(Some(&content));
    dialog.set_default_size(560, -1);

    let done: Done = Rc::new(RefCell::new(Some(on_done)));
    let finish = |done: &Done| {
        if let Some(f) = done.borrow_mut().take() {
            f();
        }
    };
    {
        let dialog = dialog.clone();
        let done = done.clone();
        keep.connect_clicked(move |_| {
            clock::confirm_suspect();
            dialog.close();
            finish(&done);
        });
    }
    {
        let dialog = dialog.clone();
        let done = done.clone();
        apply.connect_clicked(move |_| match clock::parse_input(&entry.text()) {
            None => status.set_text(&tr("Date illisible : AAAA-MM-JJ HH:MM attendu")),
            Some(target) if target < floor => status.set_text(&trf(
                "La date saisie est antérieure à {floor}.",
                &[("floor", &floor_text)],
            )),
            Some(target) => {
                clock::set_manual(target);
                dialog.close();
                finish(&done);
            }
        });
    }
    {
        let done = done.clone();
        dialog.connect_close_request(move |_| {
            // Fermer sans choisir vaut « garder l'horloge » ; la fermeture
            // déclenchée par un bouton ne doit pas écraser son choix.
            if matches!(clock::status(), ClockStatus::Suspect { .. }) {
                clock::confirm_suspect();
            }
            finish(&done);
            glib::Propagation::Proceed
        });
    }
    dialog.present();
}
