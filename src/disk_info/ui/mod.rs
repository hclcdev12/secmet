//! UI GTK4 Infos disque (SMART) — layout type maquette SecMet.

pub mod attributes;
pub mod capabilities;
pub mod health_ring;
pub mod identity;
pub mod overview;
pub mod theme;

use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Box as GtkBox, Label, Orientation, Paned};

use crate::disk_info::fetch::fetch_disk_smart_report;
use crate::disk_info::smart_models::DiskSmartReport;
use crate::i18n::{tr, trf};

use capabilities::build_features_bar;
use identity::build_disk_header;
use overview::{build_left_panel, build_quick_grid};
use theme::apply_theme_class;

pub fn show_disk_info_window(
    parent: &ApplicationWindow,
    disk_name: &str,
    disk_path: &str,
    ata_locked: bool,
) {
    let window = ApplicationWindow::builder()
        .title(tr("Infos disque"))
        .default_width(940)
        .default_height(760)
        .transient_for(parent)
        .modal(false)
        .build();

    if let Some(app) = parent.application() {
        window.set_application(Some(&app));
    }

    window.add_css_class("secmet-smart-window");
    apply_theme_class(&window);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("secmet-smart-root");
    root.set_vexpand(true);

    let content = GtkBox::new(Orientation::Vertical, 0);
    content.set_vexpand(true);
    content.set_hexpand(true);
    root.append(&content);

    if ata_locked {
        let msg = Label::new(Some(&format!(
            "{disk_name} ({disk_path})\n\n{}",
            tr("Disque verrouillé (ATA Security LOCKED).\n\
                Analyse SMART non lancée : les lectures timeout / échouent sans le mot de passe.\n\
                Effacement impossible tant que le mot de passe utilisateur ou maître n'est pas connu.\n\
                Si vous le connaissez : bouton « Déverrouiller » en bas de la fenêtre.")
        )));
        msg.add_css_class("secmet-smart-loading");
        msg.set_margin_top(16);
        msg.set_margin_start(16);
        msg.set_margin_end(16);
        msg.set_xalign(0.0);
        msg.set_wrap(true);
        content.append(&msg);
        window.set_child(Some(&root));
        window.present();
        return;
    }

    let disk_path_owned = disk_path.to_string();
    let disk_name_owned = disk_name.to_string();
    load_report_into(&content, &disk_name_owned, &disk_path_owned);

    window.set_child(Some(&root));
    window.present();
}

fn load_report_into(content: &GtkBox, disk_name: &str, disk_path: &str) {
    while let Some(child) = content.first_child() {
        content.remove(&child);
    }

    let loading = Label::new(Some(&tr("Lecture SMART en cours…")));
    loading.add_css_class("secmet-smart-loading");
    loading.set_margin_top(16);
    loading.set_margin_start(16);
    loading.set_xalign(0.0);
    content.append(&loading);

    let (sender, receiver) = std::sync::mpsc::channel();
    let disk_path_owned = disk_path.to_string();
    let disk_name_owned = disk_name.to_string();
    std::thread::spawn(move || {
        let result = fetch_disk_smart_report(&disk_path_owned);
        let _ = sender.send((disk_name_owned, result));
    });

    let content_clone = content.clone();
    glib::timeout_add_local(
        std::time::Duration::from_millis(100),
        move || match receiver.try_recv() {
            Ok((_disk_name, result)) => {
                while let Some(child) = content_clone.first_child() {
                    content_clone.remove(&child);
                }
                match result {
                    Ok(report) => content_clone.append(&build_smart_page(&report)),
                    Err(err) => {
                        let error = Label::new(Some(&trf(
                            "Erreur SMART: {error}",
                            &[("error", &err.to_string())],
                        )));
                        error.add_css_class("error");
                        error.add_css_class("secmet-smart-loading");
                        error.set_margin_top(16);
                        error.set_margin_start(16);
                        error.set_xalign(0.0);
                        content_clone.append(&error);
                    }
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => glib::ControlFlow::Break,
        },
    );
}

fn build_smart_page(report: &DiskSmartReport) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_vexpand(true);
    page.set_hexpand(true);

    let paned = Paned::new(Orientation::Horizontal);
    paned.add_css_class("secmet-smart-paned");
    paned.set_vexpand(true);
    paned.set_hexpand(true);
    paned.set_resize_start_child(false);
    paned.set_shrink_start_child(false);
    paned.set_resize_end_child(true);
    paned.set_shrink_end_child(true);
    paned.set_position(260);

    paned.set_start_child(Some(&build_left_panel(report)));
    paned.set_end_child(Some(&build_right_panel(report)));

    page.append(&paned);
    page.append(&build_statusbar(report));
    page
}

fn build_right_panel(report: &DiskSmartReport) -> GtkBox {
    let col = GtkBox::new(Orientation::Vertical, 0);
    col.add_css_class("secmet-smart-right");
    col.set_vexpand(true);
    col.set_hexpand(true);

    col.append(&build_disk_header(report));
    col.append(&build_quick_grid(report));

    if let Some(features) = build_features_bar(report) {
        col.append(&features);
    }

    col.append(&attributes::build_attributes(report));
    col
}

fn build_statusbar(report: &DiskSmartReport) -> GtkBox {
    let bar = GtkBox::new(Orientation::Horizontal, 10);
    bar.add_css_class("secmet-smart-statusbar");

    let left = GtkBox::new(Orientation::Horizontal, 10);
    let smart_on = report.health_flags.smart_status_known
        && report
            .smart_passed
            .unwrap_or(report.health_flags.health_passed);
    let dot = Label::new(Some("●"));
    dot.add_css_class("secmet-smart-sb-dot");
    if smart_on {
        dot.add_css_class("secmet-smart-tone-ok");
    } else if report.health_flags.smart_status_known {
        dot.add_css_class("secmet-smart-tone-warn");
    } else {
        dot.add_css_class("secmet-smart-tone-unknown");
    }
    let status_txt = if smart_on {
        tr("S.M.A.R.T. Actif")
    } else if report.health_flags.smart_status_known {
        tr("S.M.A.R.T. Alerte")
    } else {
        tr("S.M.A.R.T. Indisponible")
    };
    let status = Label::new(Some(&status_txt));
    status.add_css_class("secmet-smart-sb-text");
    left.append(&dot);
    left.append(&status);

    let right = Label::new(Some(&tr("SecMet — Infos disque")));
    right.add_css_class("secmet-smart-sb-text");
    right.set_halign(gtk4::Align::End);
    right.set_hexpand(true);
    right.set_xalign(1.0);

    bar.append(&left);
    bar.append(&right);
    bar
}
