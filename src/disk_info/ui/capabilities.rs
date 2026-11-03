//! Barre de capacités — affichée uniquement si données disponibles.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

use crate::disk_info::smart_models::DiskSmartReport;

/// Badges features. Retourne `None` si aucune donnée exploitable
/// (ne fabrique pas TRIM/NCQ/AAM sans parsing dédié).
pub fn build_features_bar(report: &DiskSmartReport) -> Option<GtkBox> {
    let mut badges: Vec<(String, bool)> = Vec::new();

    if report.health_flags.smart_status_known {
        let on = report
            .smart_passed
            .unwrap_or(report.health_flags.health_passed);
        badges.push(("S.M.A.R.T.".to_string(), on));
    }

    if badges.is_empty() {
        return None;
    }

    let bar = GtkBox::new(Orientation::Horizontal, 8);
    bar.add_css_class("secmet-smart-features-bar");
    bar.set_hexpand(true);

    for (label, on) in badges {
        bar.append(&feat_badge(&label, on));
    }

    Some(bar)
}

fn feat_badge(text: &str, on: bool) -> GtkBox {
    let badge = GtkBox::new(Orientation::Horizontal, 4);
    badge.add_css_class("secmet-smart-feat-badge");
    if !on {
        badge.add_css_class("secmet-smart-feat-badge-off");
    }

    let mark = Label::new(Some(if on { "✓" } else { "✗" }));
    mark.add_css_class(if on {
        "secmet-smart-feat-mark-on"
    } else {
        "secmet-smart-feat-mark-off"
    });

    let lbl = Label::new(Some(text));
    lbl.add_css_class("secmet-smart-feat-label");

    badge.append(&mark);
    badge.append(&lbl);
    badge
}
