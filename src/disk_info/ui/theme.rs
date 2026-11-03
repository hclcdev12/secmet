//! Couleurs sémantiques alignées sur l'atelier SecMet (`resources/secmet.css`).
//!
//! Clair : `#15803d` / `#c2410c` / `#b91c1c` / `#60a5fa`
//! Sombre : `#4ade80` / `#fb923c` / `#f87171` / `#93c5fd`

use crate::disk_info::smart_models::{AttrStatus, HealthStatus};

pub fn prefers_dark_theme() -> bool {
    let Some(settings) = gtk4::Settings::default() else {
        return false;
    };
    if settings.is_gtk_application_prefer_dark_theme() {
        return true;
    }
    if let Some(theme) = settings.gtk_theme_name() {
        let t = theme.to_lowercase();
        if t.contains("dark") || t.contains("noir") {
            return true;
        }
    }
    false
}

pub fn apply_theme_class(widget: &impl gtk4::prelude::WidgetExt) {
    widget.remove_css_class("secmet-theme-dark");
    widget.remove_css_class("secmet-theme-light");
    if prefers_dark_theme() {
        widget.add_css_class("secmet-theme-dark");
    } else {
        widget.add_css_class("secmet-theme-light");
    }
}

/// Accents santé / attributs (même famille que `.verify-ok|warn|fail`).
pub fn health_color_hex(status: HealthStatus) -> &'static str {
    let dark = prefers_dark_theme();
    match status {
        HealthStatus::Excellent | HealthStatus::Good => {
            if dark {
                "#4ade80"
            } else {
                "#15803d"
            }
        }
        HealthStatus::Caution => {
            if dark {
                "#fb923c"
            } else {
                "#c2410c"
            }
        }
        HealthStatus::Bad => {
            if dark {
                "#f87171"
            } else {
                "#b91c1c"
            }
        }
        HealthStatus::Unknown => {
            if dark {
                "#a3a3a3"
            } else {
                "#525252"
            }
        }
    }
}

pub fn health_color_rgb(status: HealthStatus) -> (f64, f64, f64) {
    hex_to_rgb(health_color_hex(status))
}

/// Température OK → bleu pending atelier ; sinon même sémantique santé.
pub fn temp_color_hex(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Excellent | HealthStatus::Good => {
            if prefers_dark_theme() {
                "#93c5fd"
            } else {
                "#60a5fa"
            }
        }
        other => health_color_hex(other),
    }
}

pub fn temp_color_rgb(status: HealthStatus) -> (f64, f64, f64) {
    hex_to_rgb(temp_color_hex(status))
}

pub fn temp_color_rgb_muted(status: HealthStatus) -> (f64, f64, f64) {
    let (r, g, b) = temp_color_rgb(status);
    (r * 0.55, g * 0.55, b * 0.55)
}

pub fn attr_status_color_hex(status: AttrStatus) -> &'static str {
    match status {
        AttrStatus::Good => health_color_hex(HealthStatus::Good),
        AttrStatus::Caution => health_color_hex(HealthStatus::Caution),
        AttrStatus::Bad => health_color_hex(HealthStatus::Bad),
    }
}

pub fn health_css_modifier(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Excellent | HealthStatus::Good => "ok",
        HealthStatus::Caution => "warn",
        HealthStatus::Bad => "bad",
        HealthStatus::Unknown => "unknown",
    }
}

pub fn track_rgb() -> (f64, f64, f64, f64) {
    if prefers_dark_theme() {
        (1.0, 1.0, 1.0, 0.18)
    } else {
        (0.0, 0.0, 0.0, 0.14)
    }
}

fn hex_to_rgb(hex: &str) -> (f64, f64, f64) {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return (0.5, 0.5, 0.5);
    }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0) as f64 / 255.0;
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0) as f64 / 255.0;
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0) as f64 / 255.0;
    (r, g, b)
}
