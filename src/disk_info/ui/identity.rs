//! En-tête disque + lignes d'identité (panneau gauche).

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation};

use crate::disk_info::smart_models::DiskSmartReport;
use crate::i18n::tr;

/// Header droit : modèle + sous-titre + badge santé.
pub fn build_disk_header(report: &DiskSmartReport) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.add_css_class("secmet-smart-disk-header");

    let icon = Label::new(Some("◉"));
    icon.add_css_class("secmet-smart-disk-icon");
    icon.set_halign(Align::Center);
    icon.set_valign(Align::Center);
    row.append(&icon);

    let titles = GtkBox::new(Orientation::Vertical, 2);
    titles.set_hexpand(true);

    let model = if report.identity.model_name.is_empty() {
        tr("Disque inconnu")
    } else {
        report.identity.model_name.clone()
    };
    let title = Label::new(Some(&model));
    title.add_css_class("secmet-smart-disk-title");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    titles.append(&title);

    let sub = Label::new(Some(&disk_subtitle(report)));
    sub.add_css_class("secmet-smart-disk-sub");
    sub.set_xalign(0.0);
    sub.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    titles.append(&sub);

    row.append(&titles);

    let badge_text = report.health.label().to_uppercase();
    let badge = Label::new(Some(&badge_text));
    badge.add_css_class("secmet-smart-status-badge");
    match report.health {
        crate::disk_info::HealthStatus::Excellent | crate::disk_info::HealthStatus::Good => {
            badge.add_css_class("secmet-smart-status-good");
        }
        crate::disk_info::HealthStatus::Caution => {
            badge.add_css_class("secmet-smart-status-warn");
        }
        crate::disk_info::HealthStatus::Bad => {
            badge.add_css_class("secmet-smart-status-bad");
        }
        crate::disk_info::HealthStatus::Unknown => {
            badge.add_css_class("secmet-smart-status-unknown");
        }
    }
    badge.set_valign(Align::Center);
    row.append(&badge);

    row
}

/// Liste di-rows : nœud, interface, capacité, firmware, série, type.
pub fn build_info_rows(report: &DiskSmartReport) -> GtkBox {
    let list = GtkBox::new(Orientation::Vertical, 6);
    list.add_css_class("secmet-smart-disk-info-list");

    add_di_row(
        &list,
        &tr("Nœud"),
        &display_or_dash(&report.identity.device_node),
    );
    add_di_row(
        &list,
        &tr("Interface"),
        &protocol_label(report.identity.protocol),
    );
    add_di_row(
        &list,
        &tr("Capacité"),
        &pretty_capacity(report.identity.capacity_bytes),
    );
    add_di_row(
        &list,
        &tr("Firmware"),
        &display_or_dash(&report.identity.firmware),
    );
    add_di_row(
        &list,
        &tr("Série"),
        &truncate_serial(&report.identity.serial_number),
    );
    add_di_row(&list, &tr("Type"), &device_type_label(report));

    list
}

fn add_di_row(list: &GtkBox, key: &str, value: &str) {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("secmet-smart-di-row");

    let k = Label::new(Some(&key.to_uppercase()));
    k.add_css_class("secmet-smart-di-key");
    k.set_xalign(0.0);
    k.set_halign(Align::Start);

    let v = Label::new(Some(value));
    v.add_css_class("secmet-smart-di-val");
    v.set_xalign(1.0);
    v.set_halign(Align::End);
    v.set_hexpand(true);
    v.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    v.set_selectable(true);

    row.append(&k);
    row.append(&v);
    list.append(&row);
}

fn protocol_label(protocol: crate::disk_info::DiskProtocol) -> String {
    use crate::disk_info::DiskProtocol;
    match protocol {
        DiskProtocol::Ata => "SATA".to_string(),
        DiskProtocol::Nvme => "NVMe".to_string(),
        DiskProtocol::Scsi => "SCSI".to_string(),
        DiskProtocol::Unknown => "----".to_string(),
    }
}

fn disk_subtitle(report: &DiskSmartReport) -> String {
    let cap = pretty_capacity(report.identity.capacity_bytes);
    let proto = protocol_label(report.identity.protocol);
    let kind = if report.identity.is_ssd_like() {
        "SSD"
    } else {
        "HDD"
    };
    if cap == "----" {
        format!("{proto} {kind}")
    } else {
        format!("{cap} {proto} {kind}")
    }
}

fn device_type_label(report: &DiskSmartReport) -> String {
    if !report.identity.device_type.is_empty() {
        let t = &report.identity.device_type;
        if report.identity.is_ssd_like() && !t.to_lowercase().contains("ssd") {
            return format!("{t} (SSD)");
        }
        return t.clone();
    }
    if report.identity.is_ssd_like() {
        "SSD".to_string()
    } else {
        "----".to_string()
    }
}

fn display_or_dash(s: &str) -> String {
    if s.trim().is_empty() {
        "----".to_string()
    } else {
        s.to_string()
    }
}

fn truncate_serial(s: &str) -> String {
    if s.is_empty() {
        return "----".to_string();
    }
    if s.chars().count() > 12 {
        let prefix: String = s.chars().take(8).collect();
        format!("{prefix}…")
    } else {
        s.to_string()
    }
}

pub fn pretty_capacity(bytes: Option<u64>) -> String {
    let Some(b) = bytes else {
        return "----".to_string();
    };
    let gb = b as f64 / 1_000_000_000_f64;
    let tb = b as f64 / 1_000_000_000_000_f64;
    if tb >= 1.0 {
        format!("{tb:.0} TB")
    } else {
        // Espace fine type maquette « 1 000 GB »
        let n = gb.round() as u64;
        format!("{} GB", format_grouped(n))
    }
}

fn format_grouped(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}
