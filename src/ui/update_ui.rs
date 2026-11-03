//! Côté GTK de la vérification des mises à jour : point jaune par-dessus le
//! bouton Réglages, vérification en arrière-plan au démarrage puis toutes
//! les 24 h, relance manuelle depuis les Réglages.
//!
//! La vérification elle-même ([`crate::update::check_now`]) tourne sur un
//! fil dédié ; le fil GTK sonde un canal. La fenêtre peut être reconstruite
//! (changement de langue) : l'indicateur est ré-enregistré à chaque
//! construction, le minuteur n'est lancé qu'une fois.

use gtk4::prelude::*;
use gtk4::Box as GtkBox;
use std::cell::{Cell, RefCell};
use std::sync::mpsc;
use std::time::Duration;

use crate::settings;
use crate::update::{self, UpdateState};

thread_local! {
    static INDICATOR: RefCell<Option<glib::WeakRef<GtkBox>>> = const { RefCell::new(None) };
    static TIMER_STARTED: Cell<bool> = const { Cell::new(false) };
}

/// Pastille à poser en surimpression du bouton Réglages.
pub(super) fn indicator() -> GtkBox {
    let dot = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    dot.add_css_class("update-dot");
    dot.set_halign(gtk4::Align::End);
    dot.set_valign(gtk4::Align::Start);
    dot.set_can_target(false);
    dot.set_visible(false);
    INDICATOR.with(|slot| *slot.borrow_mut() = Some(dot.downgrade()));
    refresh_indicator();
    dot
}

/// Montre la pastille si une version plus récente est connue et que la
/// vérification automatique n'est pas désactivée.
pub(super) fn refresh_indicator() {
    let state = update::load_state();
    let show = settings::load().check_updates && state.available().is_some();
    INDICATOR.with(|slot| {
        if let Some(dot) = slot.borrow().as_ref().and_then(|weak| weak.upgrade()) {
            dot.set_visible(show);
            dot.set_tooltip_text(
                show.then(|| crate::i18n::tr("Une mise à jour est disponible"))
                    .as_deref(),
            );
        }
    });
}

/// Au démarrage : vérification si elle est due, puis un minuteur horaire
/// qui relance quand l'intervalle est écoulé (robuste à la veille).
pub(super) fn start_background() {
    let due =
        |state: &UpdateState| settings::load().check_updates && state.is_due(chrono::Utc::now());
    if due(&update::load_state()) {
        run_check(|_| {});
    }
    if TIMER_STARTED.with(|started| started.replace(true)) {
        return;
    }
    glib::timeout_add_seconds_local(3600, move || {
        if due(&update::load_state()) {
            run_check(|_| {});
        }
        glib::ControlFlow::Continue
    });
}

/// Lance une vérification sur un fil dédié ; `on_done` reçoit l'état sur le
/// fil GTK, après mise à jour de la pastille.
pub(super) fn run_check(on_done: impl FnOnce(UpdateState) + 'static) {
    let (tx, rx) = mpsc::channel::<UpdateState>();
    std::thread::spawn(move || {
        let _ = tx.send(update::check_now());
    });
    let mut on_done = Some(on_done);
    glib::timeout_add_local(Duration::from_millis(200), move || {
        let state = match rx.try_recv() {
            Ok(state) => state,
            Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => update::load_state(),
        };
        refresh_indicator();
        if let Some(done) = on_done.take() {
            done(state);
        }
        glib::ControlFlow::Break
    });
}
