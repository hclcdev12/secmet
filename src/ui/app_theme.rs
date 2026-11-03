//! Thème / chrome GTK de l'application (distinct de `disk_info::ui::theme`).
use gtk4::prelude::*;
use gtk4::{Button, CssProvider};

pub(super) fn ui_prefers_dark_theme() -> bool {
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

/// GTK CSS n'accepte pas @media prefers-color-scheme : on pose une classe sur fenêtres/dialogues.
pub(super) fn apply_color_scheme_class(widget: &impl WidgetExt) {
    widget.remove_css_class("secmet-theme-dark");
    widget.remove_css_class("secmet-theme-light");
    if ui_prefers_dark_theme() {
        widget.add_css_class("secmet-theme-dark");
    } else {
        widget.add_css_class("secmet-theme-light");
    }
}

pub(super) fn setup_css() {
    const APP_CSS: &str = include_str!("../../resources/secmet.css");
    let provider = CssProvider::new();
    provider.load_from_data(APP_CSS);

    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub(super) fn set_info_button_style(button: &Button, style: &str) {
    for class in ["info-good", "info-warn", "info-bad", "info-unknown"] {
        button.remove_css_class(class);
    }
    button.add_css_class(style);
    button.set_label("i");
}
