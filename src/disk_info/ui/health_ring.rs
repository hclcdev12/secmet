//! Anneau de santé Cairo (DrawingArea + Overlay).

use std::f64::consts::{PI, TAU};

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, DrawingArea, Label, Orientation, Overlay};

use crate::disk_info::smart_models::{DiskSmartReport, HealthPercent, HealthStatus};

use super::theme::{health_color_rgb, health_css_modifier, track_rgb};

const RING_SIZE: i32 = 150;
const RING_RADIUS: f64 = 61.5;
const RING_STROKE: f64 = 7.0;
const TEXT_MAX_WIDTH: i32 = 98;

/// Classe CSS du bouton « i » (liste disques) — même mapping que l'anneau.
pub fn health_info_button_class(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Excellent | HealthStatus::Good => "info-good",
        HealthStatus::Caution => "info-warn",
        HealthStatus::Bad => "info-bad",
        HealthStatus::Unknown => "info-unknown",
    }
}

fn health_word_size_class(label: &str) -> &'static str {
    match label.chars().count() {
        0..=4 => "secmet-smart-health-word",
        5..=7 => "secmet-smart-health-word-md",
        _ => "secmet-smart-health-word-sm",
    }
}

/// Anneau 150×150 + labels centrés (`label_fr` + wear / indice).
pub fn build_health_ring(report: &DiskSmartReport) -> Overlay {
    let overlay = Overlay::new();
    overlay.add_css_class("secmet-smart-health-ring-wrap");
    overlay.set_halign(Align::Center);
    overlay.set_size_request(RING_SIZE, RING_SIZE);

    let pct = report.health_percent.map(|p| p.value()).unwrap_or(0);
    let fill = if report.health_percent.is_some() {
        (pct as f64 / 100.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (fr, fg, fb) = if report.health_percent.is_some() {
        health_color_rgb(report.health)
    } else {
        let (r, g, b, _) = track_rgb();
        (r, g, b)
    };
    let has_fill = report.health_percent.is_some() && fill > 0.0;
    let (tr, tg, tb, ta) = track_rgb();

    let area = DrawingArea::new();
    area.set_content_width(RING_SIZE);
    area.set_content_height(RING_SIZE);
    area.add_css_class("secmet-smart-health-ring");
    area.set_draw_func(move |_, cr, width, height| {
        let cx = width as f64 / 2.0;
        let cy = height as f64 / 2.0;
        let r = RING_RADIUS.min(cx.min(cy) - RING_STROKE);

        cr.set_line_width(RING_STROKE);
        cr.set_line_cap(cairo::LineCap::Round);

        cr.set_source_rgba(tr, tg, tb, ta);
        cr.arc(cx, cy, r, 0.0, TAU);
        let _ = cr.stroke();

        if has_fill {
            cr.set_source_rgb(fr, fg, fb);
            let start = -PI / 2.0;
            let end = start + fill * TAU;
            cr.arc(cx, cy, r, start, end);
            let _ = cr.stroke();
        }
    });

    let labels = GtkBox::new(Orientation::Vertical, 0);
    labels.add_css_class("secmet-smart-health-ring-text");
    labels.set_halign(Align::Center);
    labels.set_valign(Align::Center);
    labels.set_can_target(false);
    labels.set_size_request(TEXT_MAX_WIDTH, -1);

    let tone = health_css_modifier(report.health);
    let word_text = report.health.label();
    let word = Label::new(Some(&word_text));
    word.add_css_class(health_word_size_class(&word_text));
    word.add_css_class(&format!("secmet-smart-tone-{tone}"));
    word.set_halign(Align::Center);
    labels.append(&word);

    if let Some(hp) = report.health_percent {
        match hp {
            HealthPercent::Wear(v) => {
                let row = GtkBox::new(Orientation::Horizontal, 0);
                row.set_halign(Align::Center);
                let pct_lbl = Label::new(Some(&v.to_string()));
                pct_lbl.add_css_class("secmet-smart-health-pct");
                pct_lbl.add_css_class(&format!("secmet-smart-tone-{tone}"));
                let unit = Label::new(Some("%"));
                unit.add_css_class("secmet-smart-health-pct-unit");
                row.append(&pct_lbl);
                row.append(&unit);
                labels.append(&row);
            }
            HealthPercent::SecMetScore(v) => {
                let pct_lbl = Label::new(Some(&v.to_string()));
                pct_lbl.add_css_class("secmet-smart-health-pct");
                pct_lbl.add_css_class(&format!("secmet-smart-tone-{tone}"));
                pct_lbl.set_halign(Align::Center);
                labels.append(&pct_lbl);

                let unit = Label::new(Some(&crate::i18n::tr("Indice")));
                unit.add_css_class("secmet-smart-health-pct-unit");
                unit.set_halign(Align::Center);
                labels.append(&unit);
            }
        }
    }

    overlay.set_child(Some(&area));
    overlay.add_overlay(&labels);
    overlay
}
