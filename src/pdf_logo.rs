//! Logo personnalisé des certificats PDF.
//!
//! L'image choisie dans les réglages est convertie une fois en JPEG baseline
//! (seul format que le générateur PDF embarque, via DCTDecode), aplatie sur
//! fond blanc et bornée en largeur, puis enregistrée sous
//! `~/.config/secmet/pdf-logo.jpg`. Les générations suivantes la lisent.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use image::{ImageFormat, ImageReader, Rgb, RgbImage};

use crate::settings;

/// Largeur maximale du JPEG produit (pixels).
pub const MAX_WIDTH: u32 = 1200;
/// Qualité JPEG.
const JPEG_QUALITY: u8 = 90;
/// Taille maximale du fichier source accepté (octets).
pub const MAX_SOURCE_BYTES: u64 = 20 << 20;
/// Côté maximal accepté au décodage (pixels).
const MAX_DECODE_SIDE: u32 = 8192;
/// Mémoire maximale allouée par le décodeur (octets).
const MAX_DECODE_ALLOC: u64 = 128 << 20;

/// Emplacement du logo converti.
pub fn logo_path() -> Option<PathBuf> {
    settings::settings_path().map(|p| p.with_file_name("pdf-logo.jpg"))
}

/// Convertit `source` en JPEG et renvoie les octets. Tout format lisible par
/// `image` est accepté (PNG, JPEG, GIF, BMP, WebP, TIFF) ; la transparence
/// est aplatie sur blanc.
pub fn convert(source: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::metadata(source).map_err(|e| format!("{} : {e}", source.display()))?;
    if meta.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "image trop volumineuse ({} Mo, maximum {} Mo)",
            meta.len() >> 20,
            MAX_SOURCE_BYTES >> 20
        ));
    }
    let mut reader = ImageReader::open(source)
        .map_err(|e| format!("{} : {e}", source.display()))?
        .with_guessed_format()
        .map_err(|e| format!("format illisible : {e}"))?;
    // Bornes de décodage : une image déclarant des dimensions énormes
    // (bombe de décompression) est refusée avant toute allocation.
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|e| format!("image illisible : {e}"))?;
    let rgba = decoded.to_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 {
        return Err("image vide".to_string());
    }
    let mut flat = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
    for (x, y, px) in rgba.enumerate_pixels() {
        let a = u32::from(px[3]);
        let blend = |c: u8| ((u32::from(c) * a + 255 * (255 - a)) / 255) as u8;
        flat.put_pixel(x, y, Rgb([blend(px[0]), blend(px[1]), blend(px[2])]));
    }
    let flat = if w > MAX_WIDTH {
        let nh = (u64::from(h) * u64::from(MAX_WIDTH) / u64::from(w)).max(1) as u32;
        image::imageops::resize(&flat, MAX_WIDTH, nh, FilterType::Lanczos3)
    } else {
        flat
    };
    let mut out = Cursor::new(Vec::new());
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
    encoder
        .encode_image(&flat)
        .map_err(|e| format!("encodage JPEG : {e}"))?;
    let bytes = out.into_inner();
    if secmet_attest::pdf::jpeg_dimensions(&bytes).is_none() {
        return Err("le JPEG produit n'est pas exploitable par le générateur PDF".to_string());
    }
    Ok(bytes)
}

/// Convertit `source`, l'enregistre comme logo courant et met à jour les
/// réglages. Renvoie le chemin du JPEG.
pub fn install(source: &Path) -> Result<PathBuf, String> {
    let bytes = convert(source)?;
    let path = logo_path().ok_or_else(|| "répertoire de configuration introuvable".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{} : {e}", parent.display()))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, &bytes)
        .and_then(|()| fs::rename(&tmp, &path))
        .map_err(|e| format!("{} : {e}", path.display()))?;
    let mut current = settings::load();
    current.pdf_logo = Some(path.clone());
    settings::save(&current);
    Ok(path)
}

/// Revient au logo SecMet (le fichier converti est supprimé).
pub fn reset() {
    let mut current = settings::load();
    if let Some(path) = current.pdf_logo.take() {
        // Seul le fichier converti par l'application est supprimé : un
        // chemin édité à la main vers une image de l'utilisateur est laissé.
        if logo_path().is_some_and(|own| own == path) {
            let _ = fs::remove_file(path);
        }
    }
    settings::save(&current);
}

/// Le logo configuré, s'il existe et reste un JPEG valide ; sinon `None`
/// (le générateur utilise alors le logo SecMet).
pub fn current_jpeg() -> Option<Vec<u8>> {
    let path = settings::load().pdf_logo?;
    let bytes = fs::read(path).ok()?;
    secmet_attest::pdf::jpeg_dimensions(&bytes).map(|_| bytes)
}

/// Dimensions (largeur, hauteur) d'un JPEG, pour l'aperçu.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    secmet_attest::pdf::jpeg_dimensions(bytes)
}

/// `true` si `bytes` est un format d'image décodable.
pub fn is_supported(bytes: &[u8]) -> bool {
    image::guess_format(bytes)
        .map(|f| {
            matches!(
                f,
                ImageFormat::Png
                    | ImageFormat::Jpeg
                    | ImageFormat::Gif
                    | ImageFormat::Bmp
                    | ImageFormat::WebP
                    | ImageFormat::Tiff
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_with_alpha_is_flattened_resized_and_encoded_as_jpeg() {
        let dir = std::env::temp_dir().join(format!("secmet-logo-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("logo.png");
        let mut img = image::RgbaImage::from_pixel(2400, 600, image::Rgba([0, 0, 0, 0]));
        for x in 0..1200 {
            for y in 0..600 {
                img.put_pixel(x, y, image::Rgba([200, 30, 30, 255]));
            }
        }
        img.save(&src).unwrap();
        let bytes = convert(&src).unwrap();
        assert_eq!(dimensions(&bytes), Some((MAX_WIDTH, 300)));
        let back = image::load_from_memory(&bytes).unwrap().to_rgb8();
        // Zone transparente aplatie sur blanc, zone opaque conservée.
        let white = back.get_pixel(1150, 150);
        assert!(
            white[0] > 240 && white[1] > 240 && white[2] > 240,
            "{white:?}"
        );
        let red = back.get_pixel(50, 150);
        assert!(red[0] > 150 && red[1] < 80, "{red:?}");
        assert!(is_supported(&fs::read(&src).unwrap()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unreadable_file_is_an_error() {
        let dir = std::env::temp_dir().join(format!("secmet-logo-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("not-an-image.png");
        fs::write(&src, b"hello").unwrap();
        assert!(convert(&src).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
