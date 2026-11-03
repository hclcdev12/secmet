//! Panneau gauche : santé, température, informations.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, DrawingArea, Label, Orientation};

use crate::disk_info::disk_temperature::{resolve_thresholds, temperature_status};
use crate::disk_info::smart_models::DiskSmartReport;
use crate::i18n::tr;

use super::health_ring::build_health_ring;
use super::identity::build_info_rows;
use super::theme::{health_css_modifier, temp_color_rgb, temp_color_rgb_muted, track_rgb};

/// Section gauche du paned (~260 px).
pub fn build_left_panel(report: &DiskSmartReport) -> GtkBox {
    let col = GtkBox::new(Orientation::Vertical, 18);
    col.add_css_class("secmet-smart-left");
    col.set_size_request(260, -1);

    col.append(&build_health_section(report));
    col.append(&build_temp_section(report));
    col.append(&build_informations_section(report));

    col
}

fn section_label(text: &str) -> Label {
    // GTK CSS n'applique pas text-transform : uppercase côté Rust (maquette).
    let l = Label::new(Some(&text.to_uppercase()));
    l.add_css_class("secmet-smart-section-label");
    l.set_xalign(0.0);
    l.set_halign(Align::Start);
    l.set_margin_bottom(8);
    l
}

fn build_health_section(report: &DiskSmartReport) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.add_css_class("secmet-smart-health-section");
    section.set_halign(Align::Fill);

    let label = section_label(&tr("État de santé"));
    label.set_halign(Align::Center);
    label.set_xalign(0.5);
    section.append(&label);
    section.append(&build_health_ring(report));
    section
}

fn build_temp_section(report: &DiskSmartReport) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.add_css_class("secmet-smart-temp-section");

    let label = section_label(&tr("Température"));
    label.set_halign(Align::Center);
    label.set_xalign(0.5);
    section.append(&label);

    let thresholds = resolve_thresholds(
        report.identity.protocol,
        &report.temperature,
        &report.smartctl_output_lines,
    );
    let temp_state = temperature_status(report.temperature.current_celsius, &thresholds);
    let tone = health_css_modifier(temp_state);

    let temp_text = report
        .temperature
        .current_celsius
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "--".to_string());

    let val_row = GtkBox::new(Orientation::Horizontal, 4);
    val_row.set_halign(Align::Center);
    let val = Label::new(Some(&temp_text));
    val.add_css_class("secmet-smart-temp-val");
    val.add_css_class(&format!("secmet-smart-tone-{tone}"));
    let unit = Label::new(Some("°C"));
    unit.add_css_class("secmet-smart-temp-unit");
    unit.set_valign(Align::Start);
    unit.set_margin_top(8);
    val_row.append(&val);
    val_row.append(&unit);
    section.append(&val_row);

    let current = report
        .temperature
        .current_celsius
        .unwrap_or(0)
        .clamp(0, 100) as f64;
    let warn = thresholds.warning_celsius.clamp(0, 100) as f64;
    let fill_frac = current / 100.0;
    let warn_frac = warn / 100.0;
    let (fr, fg, fb) = temp_color_rgb(temp_state);
    let (mr, mg, mb) = temp_color_rgb_muted(temp_state);
    let (tr, tg, tb, ta) = track_rgb();

    let bar = DrawingArea::new();
    bar.add_css_class("secmet-smart-temp-bar");
    bar.set_content_width(200);
    bar.set_content_height(4);
    bar.set_hexpand(true);
    bar.set_margin_top(10);
    bar.set_draw_func(move |_, cr, width, height| {
        let w = width as f64;
        let h = height as f64;

        cr.set_source_rgba(tr, tg, tb, ta.max(0.22));
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();

        let fill_w = (w * fill_frac).max(0.0);
        if fill_w > 0.0 {
            let pat = cairo::LinearGradient::new(0.0, 0.0, fill_w, 0.0);
            pat.add_color_stop_rgb(0.0, mr, mg, mb);
            pat.add_color_stop_rgb(1.0, fr, fg, fb);
            let _ = cr.set_source(&pat);
            cr.rectangle(0.0, 0.0, fill_w, h);
            let _ = cr.fill();
        }

        let tick_x = (w * warn_frac).clamp(0.0, w);
        cr.set_source_rgba(tr, tg, tb, 0.55);
        cr.set_line_width(1.0);
        cr.move_to(tick_x, 0.0);
        cr.line_to(tick_x, h);
        let _ = cr.stroke();
    });
    section.append(&bar);

    let range = GtkBox::new(Orientation::Horizontal, 0);
    range.add_css_class("secmet-smart-temp-range");
    range.set_margin_top(5);
    range.set_hexpand(true);

    let z = Label::new(Some("0°C"));
    z.add_css_class("secmet-smart-temp-range-label");
    z.set_halign(Align::Start);
    z.set_hexpand(true);
    z.set_xalign(0.0);

    let mid = Label::new(Some(&format!("{}°C", thresholds.warning_celsius)));
    mid.add_css_class("secmet-smart-temp-range-label");
    mid.set_halign(Align::Center);
    mid.set_hexpand(true);
    mid.set_xalign(0.5);

    let max = Label::new(Some("100°C"));
    max.add_css_class("secmet-smart-temp-range-label");
    max.set_halign(Align::End);
    max.set_hexpand(true);
    max.set_xalign(1.0);

    range.append(&z);
    range.append(&mid);
    range.append(&max);
    section.append(&range);

    section
}

fn build_informations_section(report: &DiskSmartReport) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.append(&section_label(&tr("Informations")));
    section.append(&build_info_rows(report));
    section
}

/// Grille 3×2 lectures / écritures / heures / démarrages / réalloués / pending.
pub fn build_quick_grid(report: &DiskSmartReport) -> GtkBox {
    let grid = GtkBox::new(Orientation::Vertical, 1);
    grid.add_css_class("secmet-smart-quick-grid");

    let row1 = GtkBox::new(Orientation::Horizontal, 1);
    row1.set_homogeneous(true);
    row1.append(&qg_cell(
        &tr("Lectures"),
        &report
            .total_reads_mb
            .map(pretty_mb)
            .unwrap_or_else(|| "----".to_string()),
        QgTone::Normal,
    ));
    row1.append(&qg_cell(
        &tr("Écritures"),
        &report
            .total_writes_mb
            .map(pretty_mb)
            .unwrap_or_else(|| "----".to_string()),
        QgTone::Normal,
    ));
    row1.append(&qg_cell(
        &tr("Heures"),
        &report
            .power_on_hours
            .map(format_int)
            .unwrap_or_else(|| "----".to_string()),
        QgTone::Normal,
    ));

    let realloc = attr_raw(report, 5);
    let pending = attr_raw(report, 197);

    let row2 = GtkBox::new(Orientation::Horizontal, 1);
    row2.set_homogeneous(true);
    row2.append(&qg_cell(
        &tr("Démarrages"),
        &report
            .power_cycle_count
            .map(format_int)
            .unwrap_or_else(|| "----".to_string()),
        QgTone::Normal,
    ));
    row2.append(&qg_cell(
        &tr("Réalloués"),
        realloc.as_deref().unwrap_or("----"),
        qg_tone(realloc.as_deref()),
    ));
    row2.append(&qg_cell(
        &tr("Pending"),
        pending.as_deref().unwrap_or("----"),
        qg_tone(pending.as_deref()),
    ));

    grid.append(&row1);
    grid.append(&row2);
    grid
}

#[derive(Clone, Copy)]
enum QgTone {
    Normal,
    Good,
    Warn,
}

fn qg_tone(raw: Option<&str>) -> QgTone {
    match raw {
        Some("0") => QgTone::Good,
        Some(v) if v != "----" && !v.is_empty() => QgTone::Warn,
        _ => QgTone::Normal,
    }
}

fn qg_cell(label: &str, value: &str, tone: QgTone) -> GtkBox {
    let cell = GtkBox::new(Orientation::Vertical, 2);
    cell.add_css_class("secmet-smart-qg-cell");
    cell.set_hexpand(true);

    let k = Label::new(Some(&label.to_uppercase()));
    k.add_css_class("secmet-smart-qg-label");
    k.set_xalign(0.0);

    let v = Label::new(Some(value));
    v.add_css_class("secmet-smart-qg-value");
    match tone {
        QgTone::Good => v.add_css_class("secmet-smart-qg-value-green"),
        QgTone::Warn => v.add_css_class("secmet-smart-qg-value-warn"),
        QgTone::Normal => {}
    }
    v.set_xalign(0.0);
    v.set_selectable(true);

    cell.append(&k);
    cell.append(&v);
    cell
}

fn attr_raw(report: &DiskSmartReport, id: u32) -> Option<String> {
    report
        .attributes
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.raw_string.clone())
}

fn pretty_mb(mb: i64) -> String {
    let gb = mb as f64 / 1_000_f64;
    let tb = mb as f64 / 1_000_000_f64;
    if tb >= 1.0 {
        format!("{tb:.0} TB")
    } else if gb >= 1.0 {
        format!("{gb:.0} GB")
    } else {
        format!("{mb} MB")
    }
}

fn format_int(v: u64) -> String {
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
