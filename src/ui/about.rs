//! Fenêtre « À propos » : nom, version, titulaire du droit d'auteur,
//! licence GPL-3 (notice « sans aucune garantie » et texte intégral), lien
//! vers les sources, avis de licences tierces. C'est l'« Appropriate Legal
//! Notices » de la GPL-3 §5(d).
//!
//! Fenêtre maison plutôt que `gtk4::AboutDialog`, dont les libellés
//! intégrés suivent la traduction de GTK (`LANG`) et non la langue choisie
//! dans les Réglages.
//!
//! Titulaire, licence et adresse du dépôt viennent de
//! [`secmet_attest::legal`], seul endroit à changer avec le titulaire.

use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, Label, LinkButton, Notebook, Orientation,
    PolicyType, ScrolledWindow, TextView, Window, WrapMode,
};
use secmet_attest::legal;

use crate::app_identity::APP_NAME;
use crate::i18n::{tr, trf};

use super::app_theme::apply_color_scheme_class;

/// Texte intégral de la licence du dépôt (en-tête SecMet puis GPL-3).
pub(super) const LICENSE_TEXT: &str = include_str!("../../LICENSE");

/// Avis de licences tierces des crates de l'application et de ses outils
/// Linux (généré par `make licenses`, contrôlé par `make licenses-check`).
pub(super) const THIRD_PARTY_TEXT: &str = include_str!("../../packaging/licenses/secmet-linux.txt");

/// Ouvre la fenêtre « À propos », modale sur `parent`.
pub(super) fn show_about_dialog(parent: &ApplicationWindow) {
    let window = Window::builder()
        .title(tr("À propos de SecMet"))
        .modal(true)
        .transient_for(parent)
        .default_width(640)
        .default_height(560)
        .build();
    apply_color_scheme_class(&window);

    let root = GtkBox::new(Orientation::Vertical, 10);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);

    let notebook = Notebook::new();
    notebook.set_vexpand(true);
    notebook.append_page(&about_page(), Some(&Label::new(Some(&tr("À propos")))));
    notebook.append_page(&license_page(), Some(&Label::new(Some(&tr("Licence")))));
    notebook.append_page(
        &third_party_page(),
        Some(&Label::new(Some(&tr("Licences tierces")))),
    );
    root.append(&notebook);

    let close = Button::with_label(&tr("Fermer"));
    close.set_halign(Align::End);
    {
        let window = window.clone();
        close.connect_clicked(move |_| window.close());
    }
    root.append(&close);

    window.set_child(Some(&root));
    window.present();
}

fn wrapped_label(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.set_selectable(true);
    label
}

fn about_page() -> GtkBox {
    let version = env!("CARGO_PKG_VERSION");
    let page = GtkBox::new(Orientation::Vertical, 10);
    page.set_margin_top(18);
    page.set_margin_bottom(18);
    page.set_margin_start(18);
    page.set_margin_end(18);

    let name = Label::new(None);
    name.set_markup(&format!("<span size=\"x-large\"><b>{APP_NAME}</b></span>"));
    name.set_xalign(0.0);
    page.append(&name);
    page.append(&wrapped_label(&trf(
        "Version {version}",
        &[("version", version)],
    )));
    page.append(&wrapped_label(&tr(
        "Effacement sécurisé de disques avec certificat signé, méthodes alignées sur NIST SP 800-88.",
    )));
    page.append(&wrapped_label(&legal::copyright_short()));
    page.append(&wrapped_label(&trf(
        "Logiciel libre sous licence {license}.",
        &[("license", legal::LICENSE_NAME)],
    )));

    let source = LinkButton::with_label(
        &legal::source_url_for(version),
        &tr("Code source de cette version"),
    );
    source.set_halign(Align::Start);
    page.append(&source);
    let repo = LinkButton::with_label(legal::SOURCE_REPOSITORY, &tr("Dépôt des sources"));
    repo.set_halign(Align::Start);
    page.append(&repo);
    page
}

/// Zone de texte en lecture seule avec défilement.
fn text_view(text: &str) -> ScrolledWindow {
    let view = TextView::new();
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_monospace(true);
    view.set_wrap_mode(WrapMode::WordChar);
    view.set_left_margin(6);
    view.set_right_margin(6);
    view.buffer().set_text(text);
    let scroll = ScrolledWindow::new();
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&view));
    scroll
}

fn license_page() -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 8);
    page.set_margin_top(12);
    page.set_margin_bottom(12);
    page.set_margin_start(12);
    page.set_margin_end(12);
    page.append(&wrapped_label(&legal::copyright_line()));
    page.append(&wrapped_label(&tr(
        "Ce programme est un logiciel libre : vous pouvez le redistribuer et/ou le modifier selon les termes de la GNU General Public License publiée par la Free Software Foundation, soit la version 3 de la licence, soit (à votre choix) toute version ultérieure.\n\nCe programme est distribué dans l'espoir qu'il sera utile, mais SANS AUCUNE GARANTIE, sans même la garantie implicite de QUALITÉ MARCHANDE ou d'ADÉQUATION À UN USAGE PARTICULIER. Consultez la GNU General Public License pour plus de détails.",
    )));
    let note = wrapped_label(&tr(
        "Traduction officieuse : seule la version anglaise de la licence, reproduite ci-dessous, fait foi.",
    ));
    note.add_css_class("dim-label");
    page.append(&note);
    let marks = wrapped_label(&tr(
        "Le nom SecMet, le logo et les images du dossier content/ relèvent de leur propre licence (content/LICENSE, installée avec la documentation).",
    ));
    marks.add_css_class("dim-label");
    page.append(&marks);
    page.append(&text_view(LICENSE_TEXT));
    page
}

fn third_party_page() -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 8);
    page.set_margin_top(12);
    page.set_margin_bottom(12);
    page.set_margin_start(12);
    page.set_margin_end(12);
    page.append(&wrapped_label(&tr(
        "SecMet intègre des bibliothèques Rust distribuées sous leurs propres licences, reproduites ci-dessous.",
    )));
    page.append(&text_view(THIRD_PARTY_TEXT));
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_texts_are_present() {
        assert!(LICENSE_TEXT.contains("GNU GENERAL PUBLIC LICENSE"));
        assert!(LICENSE_TEXT.contains(legal::COPYRIGHT_HOLDER));
        assert!(!THIRD_PARTY_TEXT.trim().is_empty());
        assert!(THIRD_PARTY_TEXT.contains("gtk4"));
    }
}
