//! Bandeau « système de la clé altéré » (Annexe M §M.3). Le contrôle de
//! `secmet-image-check` tourne en tâche de fond au démarrage de la clé
//! (quelques secondes à une demi-minute) : le rapport est relu toutes les
//! 3 s jusqu'à son verdict. Bandeau rouge seulement si l'image est altérée ;
//! les autres cas (non signée, non vérifiée) ne portent qu'une réserve sur
//! les certificats. L'effacement n'est pas bloqué.

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::Label;
use std::time::Duration;

use crate::i18n::trf;
use crate::system_integrity;

/// Relectures du rapport au plus (3 s × 100 : 5 minutes).
const MAX_POLLS: u32 = 100;

/// Étiquette invisible, rendue visible si le système de la clé est altéré.
pub(super) fn notice() -> Label {
    let label = Label::new(None);
    label.add_css_class("integrity-alert");
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.set_visible(false);
    if refresh(&label) {
        return label;
    }
    let weak = label.downgrade();
    let mut polls = 0u32;
    glib::timeout_add_local(Duration::from_secs(3), move || {
        polls += 1;
        match weak.upgrade() {
            Some(label) if !refresh(&label) && polls < MAX_POLLS => glib::ControlFlow::Continue,
            _ => glib::ControlFlow::Break,
        }
    });
    label
}

/// Met le bandeau à jour ; vrai quand le verdict est rendu (plus rien à
/// relire).
fn refresh(label: &Label) -> bool {
    if let Some(report) = system_integrity::altered_report() {
        let problem = report
            .problems
            .first()
            .cloned()
            .unwrap_or_else(|| "?".to_string());
        label.set_text(&trf(
            "Le système de cette clé USB ne correspond pas à l'image publiée : il a été modifié ou s'est corrompu ({problem}). Les certificats porteront une réserve. Réécrivez la clé avec SecMet Clé USB.",
            &[("problem", &problem)],
        ));
        label.set_visible(true);
        return true;
    }
    system_integrity::settled()
}
