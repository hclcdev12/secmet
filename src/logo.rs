//! Logo SecMet embarqué pour l'interface GTK.

use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Orientation, Overflow, Picture};
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::app_identity::PKG_NAME;

const LOGO_SVG: &[u8] = include_bytes!("../content/SecMet_logo.svg");

/// Emprise fixe du logo (anciennes valeurs CSS ~180×44).
const LOGO_W: i32 = 180;
const LOGO_H: i32 = 44;

/// Logo de la fenêtre : blanc sur thème sombre, couleur d'origine
/// (#1d1d1b) sur thème clair, où le blanc serait illisible.
fn app_logo_bytes(dark: bool) -> Vec<u8> {
    let svg = std::str::from_utf8(LOGO_SVG).expect("logo SVG UTF-8");
    if !dark {
        return svg.as_bytes().to_vec();
    }
    svg.replace("#1d1d1b", "#ffffff")
        .replacen(
            "    </style>",
            "      path { fill: #ffffff; }\n    </style>",
            1,
        )
        .into_bytes()
}

fn logo_file_path(dark: bool) -> PathBuf {
    static DARK: OnceLock<PathBuf> = OnceLock::new();
    static LIGHT: OnceLock<PathBuf> = OnceLock::new();
    let (cell, name) = if dark {
        (&DARK, "logo-app-dark.svg")
    } else {
        (&LIGHT, "logo-app-light.svg")
    };
    cell.get_or_init(|| {
        let cache = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
        let path = cache.join(PKG_NAME).join(name);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, app_logo_bytes(dark));
        path
    })
    .clone()
}

/// Rasterise le SVG à LOGO_W×LOGO_H pour que la taille naturelle GTK soit fixe.
/// (Un Picture sur SVG brut garde une natural size ~758×216 → scale avec la fenêtre.)
fn logo_pixbuf(dark: bool) -> Option<Pixbuf> {
    Pixbuf::from_file_at_scale(logo_file_path(dark), LOGO_W, LOGO_H, true).ok()
}

/// Conteneur à taille fixe : indépendant de la bannière et de la largeur fenêtre.
/// `dark` : thème sombre (logo blanc) ou clair (logo d'origine).
pub fn create_brand_picture(dark: bool) -> GtkBox {
    let picture = match logo_pixbuf(dark) {
        Some(pixbuf) => Picture::for_pixbuf(&pixbuf),
        None => Picture::for_filename(logo_file_path(dark)),
    };
    picture.set_can_shrink(false);
    picture.set_keep_aspect_ratio(true);
    picture.set_hexpand(false);
    picture.set_vexpand(false);
    picture.set_halign(Align::Fill);
    picture.set_valign(Align::Fill);
    picture.set_size_request(LOGO_W, LOGO_H);
    picture.add_css_class("app-brand-logo");

    let wrap = GtkBox::new(Orientation::Horizontal, 0);
    wrap.set_size_request(LOGO_W, LOGO_H);
    wrap.set_hexpand(false);
    wrap.set_vexpand(false);
    wrap.set_halign(Align::Start);
    wrap.set_valign(Align::Center);
    wrap.set_overflow(Overflow::Hidden);
    wrap.set_margin_end(4);
    wrap.add_css_class("app-brand-logo-wrap");
    wrap.append(&picture);
    wrap
}

#[cfg(test)]
mod tests {
    use super::LOGO_SVG;

    #[test]
    fn embedded_logo_contains_brand_mark() {
        let content = std::str::from_utf8(LOGO_SVG).unwrap();
        assert!(content.contains("SecMet"));
        assert!(content.contains("<svg"));
    }

    #[test]
    fn app_logo_variant_is_white_on_dark_theme() {
        let bytes = super::app_logo_bytes(true);
        let content = std::str::from_utf8(&bytes).unwrap();
        assert!(content.contains("#ffffff"));
        assert!(!content.contains("#1d1d1b"));
        assert!(content.contains("path { fill: #ffffff; }"));
    }

    #[test]
    fn app_logo_keeps_original_colour_on_light_theme() {
        let bytes = super::app_logo_bytes(false);
        let content = std::str::from_utf8(&bytes).unwrap();
        assert!(content.contains("#1d1d1b"));
        assert!(!content.contains("#ffffff"));
    }
}
