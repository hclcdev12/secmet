//! Dialogue « Clé de signature absente » : la clé de signature des
//! certificats n'est jamais recréée en silence. Quand elle a disparu (une
//! `.pub` ou un historique témoigne qu'une clé a existé), ou qu'elle manque
//! sur la clé USB SecMet, l'opérateur décide : annuler, ou créer une
//! nouvelle clé dont il communiquera le nouvel identifiant à ses clients.

use std::cell::Cell;

use gtk4::prelude::*;
use gtk4::{ApplicationWindow, ButtonsType, DialogFlags, MessageDialog, MessageType, ResponseType};
use secmet_attest::keystore::{CreateMode, KeyState};

use crate::certificate;
use crate::i18n::{tr, trf};

use super::app_theme::apply_color_scheme_class;
use super::dialogs::show_error_dialog;

/// Texte du dialogue pour une clé perdue (marqueur `{key_id}`).
const LOST_TEXT: &str =
    "La clé de signature des certificats de ce poste a disparu (ancien key_id : {key_id}).\n\n\
             Les certificats déjà émis restent vérifiables avec l'ancienne clé publique, mais plus \
             aucun certificat ne peut être signé par elle. Aucun certificat n'est signé tant \
             qu'aucune clé n'est choisie.\n\n\
             Vous pouvez créer une nouvelle clé : elle aura un nouvel identifiant (key_id), \
             à communiquer à vos clients pour qu'ils l'inscrivent dans leur liste de confiance \
             (trusted-keys). Si l'ancienne clé a pu être copiée, demandez-leur aussi de retirer \
             l'ancien identifiant.\n\n\
             Créer une nouvelle clé de signature ?";
/// Texte du dialogue pour une clé USB sans clé de signature.
const MISSING_TEXT: &str =
    "Cette clé USB SecMet n'a pas de clé de signature des certificats (elle est \
             normalement créée au premier démarrage). Aucun certificat n'est signé tant \
             qu'aucune clé n'est créée.\n\n\
             Si cette clé USB a déjà signé des certificats, sa clé de signature a été perdue : \
             une nouvelle clé aura un nouvel identifiant (key_id), à communiquer à vos clients \
             pour qu'ils l'inscrivent dans leur liste de confiance (trusted-keys).\n\n\
             Créer une clé de signature ?";
/// Confirmation de création (marqueurs `{key_id}`, `{public_key}`).
const CREATED_TEXT: &str =
    "Nouvelle clé de signature créée.\n\nkey_id : {key_id}\nClé publique : {public_key}\n\n\
             Communiquez ce nouvel identifiant à vos clients : il figure aussi sur chaque \
             certificat signé avec cette clé.";

/// Au lancement : si la clé attend la décision de l'opérateur, le dialogue
/// s'ouvre une fois la fenêtre affichée. Rien sinon.
pub(super) fn check_at_startup(window: &ApplicationWindow) {
    if certificate::signing_key_needs_operator().is_none() {
        return;
    }
    let window = window.clone();
    gtk4::glib::idle_add_local_once(move || {
        if let Some(state) = certificate::signing_key_needs_operator() {
            show_missing_key_dialog(&window, &state, || {});
        }
    });
}

/// Avant une signature : `true` si la clé est prête (ou sera créée, toute
/// première clé d'un poste) ; sinon ouvre le dialogue, appelle
/// `on_created` si l'opérateur crée une clé, et renvoie `false`.
pub(super) fn ensure_signing_key<F>(window: &ApplicationWindow, on_created: F) -> bool
where
    F: FnOnce() + 'static,
{
    match certificate::signing_key_needs_operator() {
        None => true,
        Some(state) => {
            show_missing_key_dialog(window, &state, on_created);
            false
        }
    }
}

/// Texte du dialogue selon l'état.
fn explanation(state: &KeyState) -> String {
    match state {
        KeyState::Lost { .. } => trf(
            LOST_TEXT,
            &[(
                "key_id",
                &state.previous_key_id().unwrap_or_else(|| tr("inconnu")),
            )],
        ),
        _ => tr(MISSING_TEXT),
    }
}

fn show_missing_key_dialog<F>(window: &ApplicationWindow, state: &KeyState, on_created: F)
where
    F: FnOnce() + 'static,
{
    let dialog = MessageDialog::new(
        Some(window),
        DialogFlags::MODAL,
        MessageType::Warning,
        ButtonsType::None,
        explanation(state),
    );
    apply_color_scheme_class(&dialog);
    dialog.set_title(Some(&tr("Clé de signature absente")));
    dialog.add_button(&tr("Annuler"), ResponseType::Cancel);
    dialog.add_button(&tr("Créer une nouvelle clé"), ResponseType::Accept);
    dialog.set_default_response(ResponseType::Cancel);
    let mode = match state {
        KeyState::Missing => CreateMode::FirstKey,
        _ => CreateMode::Replace,
    };
    let on_created = std::cell::RefCell::new(Some(on_created));
    let handled = Cell::new(false);
    let window = window.clone();
    dialog.connect_response(move |dialog, response| {
        // `close()` réémet une réponse (DeleteEvent) : une seule prise en compte.
        if handled.replace(true) {
            return;
        }
        dialog.close();
        if response != ResponseType::Accept {
            return;
        }
        match certificate::create_signing_key(mode) {
            Ok(identity) => {
                show_created_dialog(&window, &identity.key_id(), &identity.public_key_hex());
                if let Some(f) = on_created.borrow_mut().take() {
                    f();
                }
            }
            Err(e) => show_error_dialog(&window, &tr("Clé de signature absente"), &e),
        }
    });
    dialog.present();
}

fn show_created_dialog(window: &ApplicationWindow, key_id: &str, public_key: &str) {
    let dialog = MessageDialog::new(
        Some(window),
        DialogFlags::MODAL,
        MessageType::Info,
        ButtonsType::Ok,
        trf(
            CREATED_TEXT,
            &[("key_id", key_id), ("public_key", public_key)],
        ),
    );
    apply_color_scheme_class(&dialog);
    dialog.set_title(Some(&tr("Clé de signature créée")));
    let handled = Cell::new(false);
    dialog.connect_response(move |dialog, _| {
        if handled.replace(true) {
            return;
        }
        dialog.close();
    });
    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{tr_in, Lang};

    #[test]
    fn dialog_texts_are_translated() {
        for key in [
            LOST_TEXT,
            MISSING_TEXT,
            CREATED_TEXT,
            "Clé de signature absente",
            "Clé de signature créée",
            "Créer une nouvelle clé",
            "inconnu",
            "Clé de signature absente : aucun certificat n'est signé. \
             Relancez SecMet pour créer une nouvelle clé.",
        ] {
            assert_ne!(tr_in(Lang::En, key), key, "EN : {key}");
            assert_ne!(tr_in(Lang::De, key), key, "DE : {key}");
        }
    }
}
