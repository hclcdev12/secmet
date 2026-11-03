//! Écrivain PDF minimal et mise en page du certificat.
//!
//! PDF 1.4, polices standard (Helvetica, Helvetica-Bold, Courier, encodage
//! WinAnsi : accents français inclus), métriques Helvetica embarquées pour
//! les alignements, logo JPEG incorporé (`DCTDecode`), flux de contenu non
//! compressés, table `xref` exacte. Aucune dépendance : ce qui est écrit est
//! entièrement connu, donc testable octet par octet.
//!
//! La mise en page reprend le certificat historique de SecMet : logo en haut
//! à gauche, titre à droite, bandeau d'état, cartes « Machine & équipement »
//! et « Médium de stockage », « Détails de l'opération », validation, puis
//! l'attestation signée avec son QR code, et le pied de page.

use crate::lang::{disposition_label, fill, reason_label, Lang, Texts};
use crate::model::{Attestation, Disposition, NistLevel, Outcome};
use crate::qr::{self, QrMatrix};
use crate::sign::SignedAttestation;
use crate::Result;

/// Largeur A4 en points.
pub const PAGE_W: f64 = 595.28;
/// Hauteur A4 en points.
pub const PAGE_H: f64 = 841.89;
const MARGIN: f64 = 42.0;
const CONTENT_W: f64 = PAGE_W - 2.0 * MARGIN;
const RIGHT: f64 = PAGE_W - MARGIN;
const GAP: f64 = 14.0;
const FOOTER_H: f64 = 44.0;
use crate::model::{truncate_chars, MAX_ITEM_CHARS, MAX_LIST_ITEMS};

/// Logo SecMet (JPEG, fond blanc), incorporé tel quel dans le PDF.
pub const LOGO_JPEG: &[u8] = include_bytes!("../resources/secmet_logo.jpg");

/// Police standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    /// Helvetica.
    Regular,
    /// Helvetica-Bold.
    Bold,
    /// Courier (largeur fixe : empreintes, signatures).
    Mono,
}

/// Largeurs Helvetica (AFM Adobe), codes 32 à 126, en millièmes d'em.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
/// Largeurs Helvetica-Bold, codes 32 à 126.
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
    611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
    278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// Lettre de base d'un caractère accentué latin, pour les métriques.
fn base_letter(c: char) -> char {
    match c {
        'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'î' | 'ï' | 'í' | 'ì' => 'i',
        'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
        'û' | 'ü' | 'ú' | 'ù' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        'ÿ' | 'ý' => 'y',
        'œ' => 'o',
        'Œ' => 'O',
        'À' | 'Â' | 'Ä' | 'Á' | 'Ã' | 'Å' => 'A',
        'É' | 'È' | 'Ê' | 'Ë' => 'E',
        'Î' | 'Ï' | 'Í' | 'Ì' => 'I',
        'Ô' | 'Ö' | 'Ó' | 'Ò' | 'Õ' => 'O',
        'Û' | 'Ü' | 'Ú' | 'Ù' => 'U',
        'Ç' => 'C',
        'Ñ' => 'N',
        '’' | '‘' => '\'',
        '“' | '”' => '"',
        '–' | '—' => '-',
        '…' => '.',
        '•' | '·' => '.',
        '€' => 'E',
        '°' => 'o',
        '«' | '»' => '"',
        other => other,
    }
}

impl Font {
    fn resource(self) -> &'static str {
        match self {
            Self::Regular => "/F1",
            Self::Bold => "/F2",
            Self::Mono => "/F3",
        }
    }

    /// Largeur d'un caractère en millièmes d'em.
    fn char_width(self, c: char) -> f64 {
        if self == Self::Mono {
            return 600.0;
        }
        let table = match self {
            Self::Bold => &HELVETICA_BOLD,
            _ => &HELVETICA,
        };
        let c = base_letter(c);
        let code = c as u32;
        if (32..=126).contains(&code) {
            f64::from(table[(code - 32) as usize])
        } else {
            // Caractère hors table (tiret cadratin, symboles) : moyenne.
            match c {
                '—' => 1000.0,
                '–' => 556.0,
                _ => 556.0,
            }
        }
    }

    /// Largeur d'un texte à `size` points.
    pub fn width(self, size: f64, text: &str) -> f64 {
        text.chars().map(|c| self.char_width(c)).sum::<f64>() * size / 1000.0
    }
}

/// Couleur RVB (0 à 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub f64, pub f64, pub f64);

const INK: Rgb = Rgb(0.12, 0.16, 0.22);
const TITLE: Rgb = Rgb(0.29, 0.33, 0.39);
const MUTED: Rgb = Rgb(0.42, 0.45, 0.50);
const RULE: Rgb = Rgb(0.89, 0.91, 0.93);
const CARD_HEAD: Rgb = Rgb(0.96, 0.97, 0.98);
const SOFT: Rgb = Rgb(0.97, 0.97, 0.98);
const GREEN: Rgb = Rgb(0.09, 0.64, 0.29);
const GREEN_BG: Rgb = Rgb(0.93, 0.99, 0.96);
const RED: Rgb = Rgb(0.86, 0.15, 0.15);
const RED_BG: Rgb = Rgb(0.995, 0.95, 0.95);
const AMBER: Rgb = Rgb(0.80, 0.50, 0.05);
const AMBER_BG: Rgb = Rgb(1.0, 0.97, 0.90);
const WHITE: Rgb = Rgb(1.0, 1.0, 1.0);

/// Encode en WinAnsi, avec échappement PDF de `(`, `)` et `\`.
pub fn encode_winansi(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 4);
    for c in text.chars() {
        let byte = match c {
            '\u{20}'..='\u{7e}' | '\u{a0}'..='\u{ff}' => c as u32 as u8,
            '€' => 0x80,
            'Œ' => 0x8c,
            'œ' => 0x9c,
            'Š' => 0x8a,
            'š' => 0x9a,
            'Ž' => 0x8e,
            'ž' => 0x9e,
            'Ÿ' => 0x9f,
            '™' => 0x99,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            '→' => b'>',
            '‖' => b'|',
            _ => b'?',
        };
        match byte {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(byte);
            }
            _ => out.push(byte),
        }
    }
    out
}

/// Découpe `text` en lignes tenant dans `width` points à `size` points,
/// d'après les métriques de la police.
pub fn wrap(text: &str, font: Font, size: f64, width: f64) -> Vec<String> {
    let fits = |s: &str| font.width(size, s) <= width;
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let mut word = word.to_string();
            // Mot plus large que la ligne (empreinte hexadécimale) : coupé.
            while !fits(&word) && word.chars().count() > 1 {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                let mut head = String::new();
                for c in word.chars() {
                    let mut candidate = head.clone();
                    candidate.push(c);
                    if !fits(&candidate) && !head.is_empty() {
                        break;
                    }
                    head.push(c);
                }
                word = word.chars().skip(head.chars().count()).collect();
                lines.push(head);
            }
            let candidate = if line.is_empty() {
                word.clone()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && !fits(&candidate) {
                lines.push(std::mem::take(&mut line));
                line = word;
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

/// Dimensions (largeur, hauteur) d'un JPEG, lues dans son marqueur SOF.
pub fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 4 || data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }
    let mut i = 2;
    while i + 4 <= data.len() {
        if data[i] != 0xff {
            return None;
        }
        let marker = data[i + 1];
        if marker == 0xd8 || (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([data[i + 2], data[i + 3]]));
        if matches!(
            marker,
            0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc9 | 0xca | 0xcb
        ) {
            if i + 9 > data.len() {
                return None;
            }
            let height = u32::from(u16::from_be_bytes([data[i + 5], data[i + 6]]));
            let width = u32::from(u16::from_be_bytes([data[i + 7], data[i + 8]]));
            return Some((width, height));
        }
        i += 2 + len;
    }
    None
}

/// Flux de contenu d'une page. Octets bruts : le texte est en WinAnsi,
/// jamais réinterprété comme de l'UTF-8.
#[derive(Debug, Default, Clone)]
pub struct Canvas {
    ops: Vec<u8>,
}

impl Canvas {
    fn op(&mut self, text: &str) {
        self.ops.extend_from_slice(text.as_bytes());
    }

    fn color(&mut self, c: Rgb, stroke: bool) {
        self.op(&format!(
            "{:.3} {:.3} {:.3} {}\n",
            c.0,
            c.1,
            c.2,
            if stroke { "RG" } else { "rg" }
        ));
    }

    /// Texte à (`x`, `y`) (origine en bas à gauche).
    pub fn text(&mut self, x: f64, y: f64, font: Font, size: f64, color: Rgb, text: &str) {
        self.color(color, false);
        self.op(&format!(
            "BT {} {:.1} Tf {:.2} {:.2} Td (",
            font.resource(),
            size,
            x,
            y
        ));
        self.ops.extend_from_slice(&encode_winansi(text));
        self.op(") Tj ET\n");
    }

    /// Texte aligné à droite sur `x_right`.
    pub fn text_right(
        &mut self,
        x_right: f64,
        y: f64,
        font: Font,
        size: f64,
        color: Rgb,
        text: &str,
    ) {
        let x = x_right - font.width(size, text);
        self.text(x, y, font, size, color, text);
    }

    /// Rectangle plein.
    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: Rgb) {
        self.color(color, false);
        self.op(&format!("{x:.2} {y:.2} {w:.2} {h:.2} re f\n"));
    }

    /// Rectangle bordé (sans remplissage).
    pub fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64, width: f64, color: Rgb) {
        self.color(color, true);
        self.op(&format!("{width:.2} w {x:.2} {y:.2} {w:.2} {h:.2} re S\n"));
    }

    /// Trait.
    pub fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, width: f64, color: Rgb) {
        self.color(color, true);
        self.op(&format!(
            "{width:.2} w {x1:.2} {y1:.2} m {x2:.2} {y2:.2} l S\n"
        ));
    }

    /// Image `/Im1` (le logo) dont le coin bas-gauche est (`x`, `y`).
    pub fn image(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.op(&format!("q {w:.2} 0 0 {h:.2} {x:.2} {y:.2} cm /Im1 Do Q\n"));
    }

    /// QR code dont le coin bas-gauche est (`x`, `y`), de côté `side`
    /// (zone de silence de 4 modules incluse).
    pub fn qr(&mut self, x: f64, y: f64, side: f64, matrix: &QrMatrix) {
        let modules = matrix.size + 8;
        let unit = side / modules as f64;
        self.fill_rect(x, y, side, side, WHITE);
        // Une plage de modules sombres contigus = un rectangle, une seule
        // couleur et un seul `f` pour tout le code : le flux reste compact.
        self.color(INK, false);
        for row in 0..matrix.size {
            let py = y + side - (row + 5) as f64 * unit;
            let mut col = 0;
            while col < matrix.size {
                if !matrix.dark(col, row) {
                    col += 1;
                    continue;
                }
                let start = col;
                while col < matrix.size && matrix.dark(col, row) {
                    col += 1;
                }
                let px = x + (start + 4) as f64 * unit;
                let w = (col - start) as f64 * unit;
                self.op(&format!("{px:.2} {py:.2} {w:.2} {unit:.2} re\n"));
            }
        }
        self.op("f\n");
    }

    /// Octets du flux.
    pub fn finish(self) -> Vec<u8> {
        self.ops
    }
}

/// Métadonnées du document.
#[derive(Debug, Clone, Default)]
pub struct DocumentInfo {
    /// Titre.
    pub title: String,
    /// Auteur (opérateur).
    pub author: String,
    /// `YYYY-MM-DD HH:MM:SS` ; converti en date PDF.
    pub created: String,
}

fn pdf_date(text: &str) -> String {
    let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 14 {
        format!("D:{}", &digits[..14])
    } else {
        String::new()
    }
}

/// Assemble un PDF à partir de pages déjà dessinées. Le logo est l'objet 7,
/// disponible dans chaque page sous `/Im1`.
pub fn assemble(pages: &[Canvas], info: &DocumentInfo) -> Vec<u8> {
    assemble_with_logo(pages, info, LOGO_JPEG)
}

/// Comme [`assemble`], avec un logo JPEG (baseline, DCTDecode) fourni.
pub fn assemble_with_logo(pages: &[Canvas], info: &DocumentInfo, logo_jpeg: &[u8]) -> Vec<u8> {
    let page_count = pages.len();
    let first_page_obj = 8;
    let mut bodies: Vec<Vec<u8>> = Vec::new();
    bodies.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: Vec<String> = (0..page_count)
        .map(|i| format!("{} 0 R", first_page_obj + 2 * i))
        .collect();
    bodies.push(
        format!(
            "<< /Type /Pages /Kids [{}] /Count {page_count} >>",
            kids.join(" ")
        )
        .into_bytes(),
    );
    for base in ["Helvetica", "Helvetica-Bold", "Courier"] {
        bodies.push(
            format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>"
            )
            .into_bytes(),
        );
    }
    let mut info_dict = Vec::new();
    info_dict.extend_from_slice(b"<< /Producer (SecMet secmet-attest ");
    info_dict.extend_from_slice(env!("CARGO_PKG_VERSION").as_bytes());
    info_dict.extend_from_slice(b") /Creator (SecMet)");
    for (key, value) in [("Title", &info.title), ("Author", &info.author)] {
        if !value.is_empty() {
            info_dict.extend_from_slice(format!(" /{key} ").as_bytes());
            info_dict.extend_from_slice(&pdf_text_string_utf16(value));
        }
    }
    let date = pdf_date(&info.created);
    if !date.is_empty() {
        info_dict
            .extend_from_slice(format!(" /CreationDate ({date}) /ModDate ({date})").as_bytes());
    }
    info_dict.extend_from_slice(b" >>");
    bodies.push(info_dict);
    // Objet 7 : le logo.
    let (lw, lh) = jpeg_dimensions(logo_jpeg).unwrap_or((1, 1));
    let mut logo = format!(
        "<< /Type /XObject /Subtype /Image /Width {lw} /Height {lh} /ColorSpace /DeviceRGB \
         /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
        logo_jpeg.len()
    )
    .into_bytes();
    logo.extend_from_slice(logo_jpeg);
    logo.extend_from_slice(b"\nendstream");
    bodies.push(logo);
    for (i, page) in pages.iter().enumerate() {
        let content_obj = first_page_obj + 2 * i + 1;
        bodies.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W:.2} {PAGE_H:.2}] \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> \
                 /XObject << /Im1 7 0 R >> >> \
                 /Contents {content_obj} 0 R >>"
            )
            .into_bytes(),
        );
        let stream = page.clone().finish();
        let mut body = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        body.extend_from_slice(&stream);
        body.extend_from_slice(b"\nendstream");
        bodies.push(body);
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = Vec::with_capacity(bodies.len());
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", bodies.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            bodies.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Un certificat prêt à dessiner.
#[derive(Debug, Clone)]
pub struct RenderedCertificate {
    /// Enveloppe signée.
    pub signed: SignedAttestation,
    /// Texte du QR.
    pub qr_text: String,
}

impl RenderedCertificate {
    /// Signe le texte QR d'une enveloppe.
    pub fn new(signed: SignedAttestation) -> Result<Self> {
        let qr_text = qr::encode_text(&signed)?;
        Ok(Self { signed, qr_text })
    }
}

// ── Composants de mise en page ───────────────────────────────────────────

fn or_dash(text: &str) -> String {
    if text.trim().is_empty() {
        "—".to_string()
    } else {
        text.to_string()
    }
}

/// `2026-10-08 11:04:12` → `08/10/2026 à 11:04:12` (selon la langue).
fn display_date(text: &str, t: &Texts) -> String {
    let d = text.trim();
    let b = d.as_bytes();
    // Les 19 premiers octets doivent être ASCII `YYYY-MM-DD HH:MM:SS` ;
    // tout autre contenu (octets multi-caractères compris) est rendu tel quel.
    let well_formed = b.len() >= 19
        && b[..19].iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b' ' || *c == b'T',
            13 | 16 => *c == b':',
            _ => c.is_ascii_digit(),
        });
    if well_formed {
        fill(
            t.date,
            &[
                ("day", &d[8..10]),
                ("month", &d[5..7]),
                ("year", &d[0..4]),
                ("time", &d[11..19]),
            ],
        )
    } else {
        or_dash(d)
    }
}

/// Chaîne de texte PDF en UTF-16BE avec BOM, entre chevrons (hex) : le
/// dictionnaire `Info` n'est pas en WinAnsi mais en PDFDocEncoding, et
/// l'UTF-16 évite toute ambiguïté pour les tirets, points de suspension, etc.
fn pdf_text_string_utf16(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + text.len() * 4);
    out.extend_from_slice(b"<FEFF");
    for unit in text.encode_utf16() {
        out.extend_from_slice(format!("{unit:04X}").as_bytes());
    }
    out.push(b'>');
    out
}

/// Limite le nombre d'éléments affichés d'une liste (le JSON garde tout).
fn bounded_list(items: &[String], max: usize, t: &Texts) -> String {
    if items.len() <= max {
        items.join(", ")
    } else {
        format!(
            "{} {}",
            items[..max].join(", "),
            fill(t.more_items, &[("n", &(items.len() - max).to_string())])
        )
    }
}

fn duration_label(seconds: Option<u64>) -> String {
    match seconds {
        Some(s) => format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60),
        None => "—".to_string(),
    }
}

/// En-tête : logo à gauche, titre et sous-titre à droite, filet.
/// Renvoie l'ordonnée sous l'en-tête.
fn header(canvas: &mut Canvas, logo: (u32, u32), title: &str, subtitle: &str) -> f64 {
    let top = PAGE_H - MARGIN;
    let (lw, lh) = logo;
    let logo_h = 44.0;
    // Largeur bornée : un logo très allongé ne doit pas chevaucher le titre.
    let logo_w = (logo_h * f64::from(lw.max(1)) / f64::from(lh.max(1))).min(CONTENT_W * 0.45);
    canvas.image(MARGIN, top - logo_h, logo_w, logo_h);
    canvas.text_right(RIGHT, top - 20.0, Font::Bold, 13.0, TITLE, title);
    canvas.text_right(RIGHT, top - 35.0, Font::Regular, 8.5, MUTED, subtitle);
    let rule_y = top - logo_h - 12.0;
    canvas.line(MARGIN, rule_y, RIGHT, rule_y, 1.6, INK);
    rule_y - GAP
}

/// Pied de page.
fn footer(canvas: &mut Canvas, t: &Texts, report_id: &str, year: &str, page: usize, total: usize) {
    let y = MARGIN + FOOTER_H - 10.0;
    canvas.line(MARGIN, y + 8.0, RIGHT, y + 8.0, 0.6, RULE);
    canvas.text(
        MARGIN,
        y - 4.0,
        Font::Regular,
        7.0,
        MUTED,
        &fill(t.footer_notice, &[("year", year)]),
    );
    canvas.text(
        MARGIN,
        y - 15.0,
        Font::Regular,
        7.0,
        MUTED,
        &fill(t.footer_report_id, &[("id", report_id)]),
    );
    canvas.text_right(
        RIGHT,
        y - 15.0,
        Font::Regular,
        7.0,
        MUTED,
        &fill(
            t.footer_page,
            &[("page", &page.to_string()), ("total", &total.to_string())],
        ),
    );
}

/// Bandeau d'état : fond teinté, barre à gauche, titre et sous-titre,
/// libellé et valeur alignés à droite. Renvoie l'ordonnée sous le bandeau.
#[allow(clippy::too_many_arguments)]
fn banner(
    canvas: &mut Canvas,
    top: f64,
    tone: (Rgb, Rgb),
    title: &str,
    subtitle: &str,
    right_label: &str,
    right_value: &str,
) -> f64 {
    let h = 52.0;
    let (fg, bg) = tone;
    canvas.fill_rect(MARGIN, top - h, CONTENT_W, h, bg);
    canvas.fill_rect(MARGIN, top - h, 4.0, h, fg);
    canvas.text(MARGIN + 16.0, top - 21.0, Font::Bold, 14.0, fg, title);
    let right_w = Font::Bold
        .width(9.0, right_value)
        .max(Font::Regular.width(8.0, right_label))
        + 16.0;
    for line in wrap(subtitle, Font::Regular, 8.5, CONTENT_W - 32.0 - right_w)
        .into_iter()
        .take(2)
        .enumerate()
    {
        canvas.text(
            MARGIN + 16.0,
            top - 36.0 - 11.0 * line.0 as f64,
            Font::Regular,
            8.5,
            fg,
            &line.1,
        );
    }
    canvas.text_right(
        RIGHT - 12.0,
        top - 20.0,
        Font::Regular,
        8.0,
        MUTED,
        right_label,
    );
    canvas.text_right(RIGHT - 12.0, top - 33.0, Font::Bold, 9.0, INK, right_value);
    top - h - GAP
}

/// Une ligne « libellé  valeur » d'une carte.
struct Row<'a> {
    label: &'a str,
    value: String,
    font: Font,
    color: Rgb,
}

impl<'a> Row<'a> {
    fn new(label: &'a str, value: impl Into<String>) -> Self {
        Self {
            label,
            value: value.into(),
            font: Font::Bold,
            color: INK,
        }
    }

    fn mono(label: &'a str, value: impl Into<String>) -> Self {
        Self {
            label,
            value: value.into(),
            font: Font::Mono,
            color: INK,
        }
    }

    fn colored(mut self, color: Rgb) -> Self {
        self.color = color;
        self
    }
}

const ROW_SIZE: f64 = 8.5;
const ROW_STEP: f64 = 14.5;
const CARD_HEAD_H: f64 = 24.0;
const CARD_PAD: f64 = 12.0;
const LABEL_W: f64 = 92.0;

/// Hauteur d'une colonne de lignes, valeurs repliées sur `value_w`.
fn rows_height(rows: &[Row<'_>], value_w: f64) -> f64 {
    rows.iter()
        .map(|r| {
            let lines = wrap(&r.value, r.font, ROW_SIZE, value_w).len().max(1) as f64;
            ROW_STEP + (lines - 1.0) * 11.0
        })
        .sum()
}

/// Dessine des lignes à partir de (`x`, `top`) ; renvoie la hauteur occupée.
fn draw_rows(canvas: &mut Canvas, x: f64, top: f64, width: f64, rows: &[Row<'_>]) -> f64 {
    let value_w = width - LABEL_W;
    let mut y = top - 10.0;
    for row in rows {
        canvas.text(x, y, Font::Regular, ROW_SIZE, MUTED, row.label);
        let lines = wrap(&row.value, row.font, ROW_SIZE, value_w);
        for (i, line) in lines.iter().enumerate() {
            canvas.text(
                x + LABEL_W,
                y - 11.0 * i as f64,
                row.font,
                ROW_SIZE,
                row.color,
                line,
            );
        }
        y -= ROW_STEP + (lines.len().max(1) as f64 - 1.0) * 11.0;
    }
    top - y - 10.0 + 4.0
}

/// Cadre d'une carte avec son bandeau de titre ; renvoie l'ordonnée du
/// haut de la zone de contenu.
fn card_frame(canvas: &mut Canvas, x: f64, top: f64, w: f64, h: f64, title: &str) -> f64 {
    canvas.fill_rect(x, top - h, w, h, WHITE);
    canvas.stroke_rect(x, top - h, w, h, 0.8, RULE);
    canvas.fill_rect(x, top - CARD_HEAD_H, w, CARD_HEAD_H, CARD_HEAD);
    canvas.line(x, top - CARD_HEAD_H, x + w, top - CARD_HEAD_H, 0.8, RULE);
    canvas.text(
        x + CARD_PAD,
        top - 16.0,
        Font::Bold,
        9.0,
        INK,
        &title.to_uppercase(),
    );
    top - CARD_HEAD_H - CARD_PAD
}

/// Carte à une colonne de lignes. Renvoie la hauteur totale.
fn card(
    canvas: &mut Canvas,
    x: f64,
    top: f64,
    w: f64,
    title: &str,
    rows: &[Row<'_>],
    min_h: f64,
) -> f64 {
    let inner_w = w - 2.0 * CARD_PAD;
    let h = (CARD_HEAD_H + CARD_PAD + rows_height(rows, inner_w - LABEL_W) + CARD_PAD).max(min_h);
    let content_top = card_frame(canvas, x, top, w, h, title);
    draw_rows(canvas, x + CARD_PAD, content_top, inner_w, rows);
    h
}

fn outcome_tone(outcome: Outcome) -> (Rgb, Rgb) {
    match outcome {
        Outcome::Success => (GREEN, GREEN_BG),
        Outcome::Failed => (RED, RED_BG),
        Outcome::Cancelled => (AMBER, AMBER_BG),
    }
}

fn outcome_title(outcome: Outcome, t: &Texts) -> &'static str {
    match outcome {
        Outcome::Success => t.outcome_success,
        Outcome::Failed => t.outcome_failed,
        Outcome::Cancelled => t.outcome_cancelled,
    }
}

/// Catégorie NIST telle qu'imprimée (PURGE et CLEAR sont des termes de la
/// norme, seul « non classé » se traduit).
fn level_label(level: NistLevel, t: &Texts) -> &'static str {
    match level {
        NistLevel::Purge => "PURGE",
        NistLevel::Clear => "CLEAR",
        NistLevel::NotClassified => t.not_classified,
    }
}

fn year_of(date: &str) -> String {
    date.get(..4)
        .filter(|y| y.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or("2026")
        .to_string()
}

/// Dessine une page de certificat.
pub fn certificate_page(
    cert: &RenderedCertificate,
    logo: (u32, u32),
    page: usize,
    total: usize,
    lang: Lang,
) -> Result<Canvas> {
    let t = lang.texts();
    let a: &Attestation = &cert.signed.payload;
    let mut canvas = Canvas::default();
    let mut y = header(&mut canvas, logo, t.title, t.subtitle);

    // Bandeau d'état.
    let level = a.erasure.nist_level;
    let mut subtitle = match a.erasure.outcome {
        Outcome::Success => fill(t.banner_success, &[("level", level_label(level, t))]),
        Outcome::Failed => t.banner_failed.to_string(),
        Outcome::Cancelled => t.banner_cancelled.to_string(),
    };
    if let Some(d) = a.erasure.disposition.filter(|d| d.destroy()) {
        subtitle.push(' ');
        subtitle.push_str(disposition_label(d, lang));
        subtitle.push('.');
    }
    y = banner(
        &mut canvas,
        y,
        outcome_tone(a.erasure.outcome),
        outcome_title(a.erasure.outcome, t),
        &subtitle,
        t.operation_date,
        &display_date(a.erasure.finished_at.as_deref().unwrap_or(&a.issued_at), t),
    );

    // Deux cartes côte à côte.
    let half = (CONTENT_W - GAP) / 2.0;
    let machine = [
        Row::new(t.manufacturer, or_dash(&a.bench.manufacturer)),
        Row::new(t.model, or_dash(&a.bench.model)),
        Row::new(t.serial, or_dash(&a.bench.serial)),
    ];
    let capacity = match a.disk.size_bytes {
        Some(bytes) => fill(
            t.capacity_bytes,
            &[
                ("capacity", &or_dash(&a.disk.capacity)),
                ("bytes", &bytes.to_string()),
            ],
        ),
        None => or_dash(&a.disk.capacity),
    };
    let media = if a.disk.transport.is_empty() {
        or_dash(&a.disk.media)
    } else {
        format!("{} ({})", or_dash(&a.disk.media), a.disk.transport)
    };
    let medium = [
        Row::new(t.kind, media),
        Row::new(t.model, or_dash(&a.disk.model)),
        Row::mono(t.serial, or_dash(&a.disk.serial)),
        Row::new(t.firmware, or_dash(&a.disk.firmware)),
        Row::new(t.capacity, capacity),
        Row::new(t.node, or_dash(&a.disk.name)),
    ];
    let inner = half - 2.0 * CARD_PAD - LABEL_W;
    let h = (CARD_HEAD_H + 2.0 * CARD_PAD + rows_height(&machine, inner))
        .max(CARD_HEAD_H + 2.0 * CARD_PAD + rows_height(&medium, inner));
    card(&mut canvas, MARGIN, y, half, t.card_machine, &machine, h);
    card(
        &mut canvas,
        MARGIN + half + GAP,
        y,
        half,
        t.card_medium,
        &medium,
        h,
    );
    y -= h + GAP;

    // Détails de l'opération : encadré méthode puis deux colonnes.
    let method_text = fill(
        t.method,
        &[
            ("label", &a.erasure.method_label),
            ("code", &a.erasure.method_code),
        ],
    );
    let method_lines = wrap(
        &method_text,
        Font::Bold,
        10.0,
        CONTENT_W - 2.0 * CARD_PAD - 24.0,
    );
    let method_h = 14.0 + 13.0 * method_lines.len() as f64;
    let (level_text, level_color) = match (a.erasure.outcome, level) {
        (Outcome::Success, NistLevel::Purge | NistLevel::Clear) => (level_label(level, t), GREEN),
        _ => (t.not_classified, RED),
    };
    let (result_text, result_color) = match a.erasure.outcome {
        Outcome::Success => (t.result_success, GREEN),
        Outcome::Failed => (t.result_failed, RED),
        Outcome::Cancelled => (t.result_cancelled, AMBER),
    };
    let left = [
        Row::new(t.operator, or_dash(&a.operator)),
        Row::new(t.software, or_dash(&a.software)),
        Row::new(t.policy, or_dash(&a.erasure.policy)),
        Row::new(t.duration, duration_label(a.erasure.duration_seconds)),
    ];
    let right = [
        Row::new(t.nist_category, level_text).colored(level_color),
        Row::new(t.result, result_text).colored(result_color),
        Row::new(
            t.start,
            display_date(a.erasure.started_at.as_deref().unwrap_or(""), t),
        ),
        Row::new(
            t.end,
            display_date(a.erasure.finished_at.as_deref().unwrap_or(""), t),
        ),
    ];
    let col_w = (CONTENT_W - 2.0 * CARD_PAD - GAP) / 2.0;
    // Pleine largeur sous les colonnes : sort du support (raisons longues)
    // puis réserves.
    let mut caveats: Vec<Row<'_>> = match a.erasure.disposition {
        Some(d) => {
            vec![Row::new(t.disposition, disposition_text(a, d, lang)).colored(disposition_color(d))]
        }
        None => Vec::new(),
    };
    if !a.erasure.caveats.is_empty() {
        let shown: Vec<String> = a
            .erasure
            .caveats
            .iter()
            .take(MAX_LIST_ITEMS)
            .map(|c| truncate_chars(c, MAX_ITEM_CHARS))
            .collect();
        let mut text = shown.join(" · ");
        if a.erasure.caveats.len() > MAX_LIST_ITEMS {
            text.push_str(" · ");
            text.push_str(&fill(
                t.more_items,
                &[("n", &(a.erasure.caveats.len() - MAX_LIST_ITEMS).to_string())],
            ));
        }
        caveats.push(Row::new(t.caveats, text));
    }
    let cols_h = rows_height(&left, col_w - LABEL_W).max(rows_height(&right, col_w - LABEL_W));
    let caveats_h = rows_height(&caveats, CONTENT_W - 2.0 * CARD_PAD - LABEL_W);
    let h = CARD_HEAD_H + CARD_PAD + method_h + 8.0 + cols_h + caveats_h + CARD_PAD;
    let content_top = card_frame(&mut canvas, MARGIN, y, CONTENT_W, h, t.card_details);
    canvas.fill_rect(
        MARGIN + CARD_PAD,
        content_top - method_h,
        CONTENT_W - 2.0 * CARD_PAD,
        method_h,
        SOFT,
    );
    canvas.fill_rect(
        MARGIN + CARD_PAD,
        content_top - method_h,
        3.0,
        method_h,
        INK,
    );
    for (i, line) in method_lines.iter().enumerate() {
        canvas.text(
            MARGIN + CARD_PAD + 14.0,
            content_top - 13.0 - 13.0 * i as f64,
            Font::Bold,
            10.0,
            INK,
            line,
        );
    }
    let cols_top = content_top - method_h - 8.0;
    draw_rows(&mut canvas, MARGIN + CARD_PAD, cols_top, col_w, &left);
    draw_rows(
        &mut canvas,
        MARGIN + CARD_PAD + col_w + GAP,
        cols_top,
        col_w,
        &right,
    );
    if !caveats.is_empty() {
        draw_rows(
            &mut canvas,
            MARGIN + CARD_PAD,
            cols_top - cols_h,
            CONTENT_W - 2.0 * CARD_PAD,
            &caveats,
        );
    }
    y -= h + GAP;

    // Validation par relecture et attestation signée, QR sur toute la hauteur.
    let bottom = MARGIN + FOOTER_H + 4.0;
    let available = (y - bottom).max(CARD_HEAD_H + 80.0);
    let qr_side = (available - CARD_HEAD_H - 2.0 * CARD_PAD).clamp(80.0, 124.0);
    let qr_x = RIGHT - CARD_PAD - qr_side;
    let text_w = CONTENT_W - 3.0 * CARD_PAD - qr_side;
    let mut rows = vec![
        Row::new(
            t.evidence,
            match a.validation.kind.as_str() {
                "canaries" => t.evidence_canaries.to_string(),
                "read-probes" => t.evidence_probes.to_string(),
                other => or_dash(other),
            },
        ),
        Row::new(
            t.blocks_read,
            format!("{} / {}", a.validation.samples, a.validation.anomalies),
        ),
        Row::new(
            t.residual,
            if a.validation.residual_signatures.is_empty() {
                t.none.to_string()
            } else {
                bounded_list(&a.validation.residual_signatures, MAX_LIST_ITEMS, t)
            },
        ),
        Row::mono(t.evidence_digest, a.validation.evidence_sha256.clone()),
    ];
    if let Some(seed) = &a.validation.seed {
        rows.push(Row::mono(t.canary_seed, seed.clone()));
    }
    rows.push(Row::new(
        t.independent_check,
        or_dash(&a.validation.verify_status),
    ));
    if let Some(report) = &a.report_sha256 {
        rows.push(Row::mono(t.technical_report, report.clone()));
    }
    rows.push(Row::new(t.signing_key, signing_key_value(&cert.signed, t)));
    rows.push(Row::mono(t.public_key, cert.signed.public_key.clone()));
    rows.push(Row::mono(t.signature, cert.signed.signature.clone()));
    rows.push(Row::new(t.verification, t.verification_value));
    let mut text_h = rows_height(&rows, text_w - LABEL_W);
    // Page chargée (sort du support et réserves longs) : la phrase fixe de
    // vérification cède la place plutôt que de chevaucher le pied de page.
    if CARD_HEAD_H + 2.0 * CARD_PAD + text_h > available {
        rows.pop();
        text_h = rows_height(&rows, text_w - LABEL_W);
    }
    let h = (CARD_HEAD_H + CARD_PAD + text_h + CARD_PAD)
        .max(CARD_HEAD_H + qr_side + 2.0 * CARD_PAD)
        .min(available);
    let content_top = card_frame(&mut canvas, MARGIN, y, CONTENT_W, h, t.card_validation);
    draw_rows(&mut canvas, MARGIN + CARD_PAD, content_top, text_w, &rows);
    let matrix = qr::matrix(&cert.qr_text)?;
    let qr_y = (y - h) + (h - CARD_HEAD_H - qr_side) / 2.0;
    canvas.qr(qr_x, qr_y, qr_side, &matrix);

    footer(
        &mut canvas,
        t,
        &a.certificate_id,
        &year_of(&a.issued_at),
        page,
        total,
    );
    Ok(canvas)
}

/// Largeurs des colonnes du tableau de lot : n°, disque, type, modèle,
/// série, capacité, méthode, durée, résultat.
const LOT_WIDTHS: [f64; 9] = [16.0, 40.0, 44.0, 96.0, 82.0, 50.0, 103.0, 38.0, 42.0];

/// Titres des colonnes du tableau de lot.
fn lot_titles(t: &Texts) -> [&'static str; 9] {
    let c = t.lot_columns;
    ["#", c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]
}

/// Couleur d'un sort du support.
const fn disposition_color(d: Disposition) -> Rgb {
    match d {
        Disposition::Reusable => GREEN,
        Disposition::DestroyRequired => RED,
        Disposition::DestroyRecommended | Disposition::Retry => AMBER,
    }
}

/// Libellé du sort suivi de ses raisons.
fn disposition_text(a: &Attestation, d: Disposition, lang: Lang) -> String {
    let mut text = disposition_label(d, lang).to_string();
    if !a.erasure.disposition_reasons.is_empty() {
        let reasons: Vec<String> = a
            .erasure
            .disposition_reasons
            .iter()
            .map(|c| reason_label(c, lang))
            .collect();
        text.push_str(" — ");
        text.push_str(&reasons.join(" · "));
    }
    text
}

/// Supports du lot à détruire physiquement.
fn destroy_count(certs: &[RenderedCertificate]) -> usize {
    certs
        .iter()
        .filter(|c| {
            c.signed
                .payload
                .erasure
                .disposition
                .is_some_and(Disposition::destroy)
        })
        .count()
}

/// Largeurs des colonnes du bordereau : n°, disque, modèle, série, sort, raisons.
const DESTROY_WIDTHS: [f64; 6] = [16.0, 40.0, 110.0, 96.0, 88.0, 161.0];

fn destroy_titles(t: &Texts) -> [&'static str; 6] {
    let c = t.lot_columns;
    let d = t.lot_destroy_columns;
    ["#", c[0], c[2], c[3], d[0], d[1]]
}

/// Lignes du bordereau, numérotées comme dans le tableau du lot.
fn destruction_rows(certs: &[RenderedCertificate], lang: Lang) -> Vec<LotRow> {
    certs
        .iter()
        .enumerate()
        .filter_map(|(i, cert)| {
            let a = &cert.signed.payload;
            let d = a.erasure.disposition.filter(|d| d.destroy())?;
            let reasons: Vec<String> = a
                .erasure
                .disposition_reasons
                .iter()
                .map(|c| reason_label(c, lang))
                .collect();
            let values = [
                (i + 1).to_string(),
                or_dash(&a.disk.name),
                or_dash(&a.disk.model),
                or_dash(&a.disk.serial),
                disposition_label(d, lang).to_string(),
                if reasons.is_empty() {
                    or_dash("")
                } else {
                    reasons.join(" · ")
                },
            ];
            let cells: Vec<Vec<String>> = values
                .iter()
                .zip(DESTROY_WIDTHS.iter())
                .enumerate()
                .map(|(col, (value, w))| {
                    let font = if col == 3 { Font::Mono } else { Font::Regular };
                    wrap(value, font, LOT_CELL_SIZE, w - 2.0 * LOT_CELL_PAD)
                })
                .collect();
            let lines = cells.iter().map(Vec::len).max().unwrap_or(1).max(1) as f64;
            Some(LotRow {
                cells,
                result_color: disposition_color(d),
                height: lines * LOT_LINE + 2.0 * LOT_CELL_PAD,
            })
        })
        .collect()
}

/// Hauteur disponible pour les lignes du bordereau sur une page.
fn destruction_budget() -> f64 {
    let header_and_banner = 44.0 + 12.0 + GAP + 52.0 + GAP;
    PAGE_H - 2.0 * MARGIN - header_and_banner - FOOTER_H - LOT_HEAD_H
}

/// Découpe des lignes en pages : `(début, fin)` par page ; aucune page sans ligne.
fn chunk_rows(rows: &[LotRow], budget: f64) -> Vec<(usize, usize)> {
    let mut pages = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    for (i, row) in rows.iter().enumerate() {
        if used + row.height > budget && i > start {
            pages.push((start, i));
            start = i;
            used = 0.0;
        }
        used += row.height;
    }
    if !rows.is_empty() {
        pages.push((start, rows.len()));
    }
    pages
}

/// Pages du bordereau de destruction physique.
#[allow(clippy::too_many_arguments)]
fn destruction_pages(
    certs: &[RenderedCertificate],
    rows: &[LotRow],
    chunks: &[(usize, usize)],
    logo: (u32, u32),
    lot_id: &str,
    first_page: usize,
    total_pages: usize,
    lang: Lang,
) -> Vec<Canvas> {
    let t = lang.texts();
    let first = &certs[0].signed.payload;
    chunks
        .iter()
        .enumerate()
        .map(|(k, (start, end))| {
            let mut canvas = Canvas::default();
            let mut y = header(
                &mut canvas,
                logo,
                t.lot_destroy_title,
                t.lot_destroy_subtitle,
            );
            y = banner(
                &mut canvas,
                y,
                (RED, RED_BG),
                &fill(t.lot_destroy_banner, &[("n", &rows.len().to_string())]),
                t.lot_destroy_detail,
                t.lot_id,
                lot_id,
            );
            draw_table(
                &mut canvas,
                y,
                &destroy_titles(t),
                &DESTROY_WIDTHS,
                &rows[*start..*end],
                4,
                3,
            );
            footer(
                &mut canvas,
                t,
                lot_id,
                &year_of(&first.issued_at),
                first_page + k + 1,
                total_pages,
            );
            canvas
        })
        .collect()
}

/// Valeur de la ligne « clé de signature ».
fn signing_key_value(signed: &SignedAttestation, t: &Texts) -> String {
    fill(
        t.signing_key_value,
        &[("alg", &signed.alg), ("id", &signed.key_id)],
    )
}
const LOT_CELL_SIZE: f64 = 7.5;
const LOT_CELL_PAD: f64 = 4.0;
const LOT_LINE: f64 = 9.5;
const LOT_HEAD_H: f64 = 20.0;

/// Cellules d'une ligne du tableau, déjà repliées, et leur couleur.
struct LotRow {
    cells: Vec<Vec<String>>,
    result_color: Rgb,
    height: f64,
}

/// Machines distinctes du lot (numéros de série DMI non vides).
fn machines_count(certs: &[RenderedCertificate]) -> usize {
    let serials: std::collections::HashSet<&str> = certs
        .iter()
        .map(|c| c.signed.payload.bench.serial.as_str())
        .filter(|s| !s.is_empty())
        .collect();
    serials.len()
}

/// Règle d'identité (Annexe M) : plusieurs machines dans le lot → le
/// tableau porte la série de la machine sous chaque disque.
fn several_machines(certs: &[RenderedCertificate]) -> bool {
    machines_count(certs) > 1
}

fn lot_row(n: usize, cert: &RenderedCertificate, t: &Texts, show_machine: bool) -> LotRow {
    let a = &cert.signed.payload;
    let (result, mut color) = match a.erasure.outcome {
        Outcome::Success => (t.lot_success, GREEN),
        Outcome::Failed => (t.lot_failed, RED),
        Outcome::Cancelled => (t.lot_cancelled, AMBER),
    };
    let destroy = a.erasure.disposition.filter(|d| d.destroy());
    if let Some(d) = destroy {
        color = disposition_color(d);
    }
    let method = if a.erasure.outcome == Outcome::Success {
        format!(
            "{} — NIST {}",
            a.erasure.method_label,
            level_label(a.erasure.nist_level, t)
        )
    } else {
        a.erasure.method_label.clone()
    };
    let values = [
        n.to_string(),
        or_dash(&a.disk.name),
        or_dash(&a.disk.media),
        or_dash(&a.disk.model),
        or_dash(&a.disk.serial),
        or_dash(&a.disk.capacity),
        method,
        duration_label(a.erasure.duration_seconds),
        result.to_string(),
    ];
    let mut cells: Vec<Vec<String>> = values
        .iter()
        .zip(LOT_WIDTHS.iter())
        .enumerate()
        .map(|(col, (value, w))| {
            let font = if col == 4 { Font::Mono } else { Font::Regular };
            wrap(value, font, LOT_CELL_SIZE, w - 2.0 * LOT_CELL_PAD)
        })
        .collect();
    if show_machine && !a.bench.serial.is_empty() {
        let machine = format!("{} {}", t.lot_machine_prefix, a.bench.serial);
        cells[3].extend(wrap(
            &machine,
            Font::Regular,
            LOT_CELL_SIZE,
            LOT_WIDTHS[3] - 2.0 * LOT_CELL_PAD,
        ));
    }
    if destroy.is_some() {
        cells[8].push(t.lot_destroy_tag.to_string());
    }
    let lines = cells.iter().map(Vec::len).max().unwrap_or(1).max(1) as f64;
    LotRow {
        cells,
        result_color: color,
        height: lines * LOT_LINE + 2.0 * LOT_CELL_PAD,
    }
}

/// Hauteur disponible pour les lignes du tableau sur la première page
/// (sous les cartes) et sur les pages suivantes.
fn lot_table_budget(first_page: bool) -> f64 {
    let header_and_banner = 44.0 + 12.0 + GAP + 52.0 + GAP;
    let cards = if first_page {
        CARD_HEAD_H + 2.0 * CARD_PAD + 4.0 * ROW_STEP + 11.0 + GAP
    } else {
        0.0
    };
    let attestation_card = CARD_HEAD_H + 2.0 * CARD_PAD + 3.0 * ROW_STEP + 11.0 + GAP;
    PAGE_H - 2.0 * MARGIN - header_and_banner - cards - attestation_card - FOOTER_H - LOT_HEAD_H
}

/// Découpe les lignes du lot en pages : `(début, fin)` par page.
pub fn lot_pages(certs: &[RenderedCertificate], lang: Lang) -> Vec<(usize, usize)> {
    let t = lang.texts();
    let mut pages = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    let mut budget = lot_table_budget(true);
    for (i, cert) in certs.iter().enumerate() {
        let h = lot_row(i + 1, cert, t, several_machines(certs)).height;
        if used + h > budget && i > start {
            pages.push((start, i));
            start = i;
            used = 0.0;
            budget = lot_table_budget(false);
        }
        used += h;
    }
    pages.push((start, certs.len()));
    pages
}

fn draw_lot_table(canvas: &mut Canvas, t: &Texts, top: f64, rows: &[LotRow]) -> f64 {
    draw_table(canvas, top, &lot_titles(t), &LOT_WIDTHS, rows, 8, 4)
}

/// Tableau à colonnes fixes : `result_col` en gras de la couleur de la
/// ligne, `mono_col` en chasse fixe (numéros de série).
fn draw_table(
    canvas: &mut Canvas,
    top: f64,
    titles: &[&str],
    widths: &[f64],
    rows: &[LotRow],
    result_col: usize,
    mono_col: usize,
) -> f64 {
    let total_h = LOT_HEAD_H + rows.iter().map(|r| r.height).sum::<f64>();
    canvas.fill_rect(MARGIN, top - total_h, CONTENT_W, total_h, WHITE);
    canvas.stroke_rect(MARGIN, top - total_h, CONTENT_W, total_h, 0.8, RULE);
    canvas.fill_rect(MARGIN, top - LOT_HEAD_H, CONTENT_W, LOT_HEAD_H, CARD_HEAD);
    canvas.line(MARGIN, top - LOT_HEAD_H, RIGHT, top - LOT_HEAD_H, 0.8, RULE);
    let mut x = MARGIN;
    for (title, w) in titles.iter().zip(widths) {
        canvas.text(x + LOT_CELL_PAD, top - 13.0, Font::Bold, 7.0, MUTED, title);
        x += w;
        if x < RIGHT - 1.0 {
            canvas.line(x, top - total_h, x, top, 0.5, RULE);
        }
    }
    let mut y = top - LOT_HEAD_H;
    for row in rows {
        let mut x = MARGIN;
        for (col, (cell, w)) in row.cells.iter().zip(widths.iter()).enumerate() {
            let (font, color) = if col == result_col {
                (Font::Bold, row.result_color)
            } else if col == mono_col {
                (Font::Mono, INK)
            } else {
                (Font::Regular, INK)
            };
            for (k, line) in cell.iter().enumerate() {
                canvas.text(
                    x + LOT_CELL_PAD,
                    y - LOT_CELL_PAD - 7.0 - LOT_LINE * k as f64,
                    font,
                    LOT_CELL_SIZE,
                    color,
                    line,
                );
            }
            x += w;
        }
        y -= row.height;
        canvas.line(MARGIN, y, RIGHT, y, 0.5, RULE);
    }
    total_h
}

/// Période couverte par les opérations du lot.
fn lot_period(certs: &[RenderedCertificate], t: &Texts) -> String {
    let starts: Vec<&str> = certs
        .iter()
        .filter_map(|c| c.signed.payload.erasure.started_at.as_deref())
        .collect();
    let ends: Vec<&str> = certs
        .iter()
        .filter_map(|c| c.signed.payload.erasure.finished_at.as_deref())
        .collect();
    match (starts.iter().min(), ends.iter().max()) {
        (Some(s), Some(e)) => format!("{} – {}", display_date(s, t), display_date(e, t)),
        _ => "—".to_string(),
    }
}

/// Pages de synthèse d'un lot : en-tête, bandeau, cartes « Machine &
/// équipement » et « Session », tableau des disques (paginé), attestations.
pub fn summary_pages(
    certs: &[RenderedCertificate],
    logo: (u32, u32),
    lot_id: &str,
    total_pages: usize,
    lang: Lang,
) -> Vec<Canvas> {
    let t = lang.texts();
    let success = certs
        .iter()
        .filter(|c| c.signed.payload.erasure.outcome == Outcome::Success)
        .count();
    let failed = certs.len() - success;
    let destroy = destroy_count(certs);
    let first = &certs[0].signed.payload;
    let rows: Vec<LotRow> = certs
        .iter()
        .enumerate()
        .map(|(i, c)| lot_row(i + 1, c, t, several_machines(certs)))
        .collect();
    let mut pages = Vec::new();
    let counts = [
        ("n", certs.len().to_string()),
        ("success", success.to_string()),
        ("failed", failed.to_string()),
    ];
    let counts: Vec<(&str, &str)> = counts.iter().map(|(k, v)| (*k, v.as_str())).collect();
    for (page_index, (start, end)) in lot_pages(certs, lang).into_iter().enumerate() {
        let mut canvas = Canvas::default();
        let mut y = header(&mut canvas, logo, t.lot_title, t.lot_subtitle);
        let (tone, title, subtitle) = if failed == 0 {
            ((GREEN, GREEN_BG), t.lot_all_ok, t.lot_all_ok_detail)
        } else if success == 0 {
            ((RED, RED_BG), t.lot_all_failed, t.lot_all_failed_detail)
        } else {
            ((AMBER, AMBER_BG), t.lot_mixed, t.lot_mixed_detail)
        };
        let mut subtitle = subtitle.to_string();
        if destroy > 0 {
            subtitle.push(' ');
            subtitle.push_str(&fill(t.lot_destroy_count, &[("n", &destroy.to_string())]));
        }
        y = banner(
            &mut canvas,
            y,
            tone,
            &fill(title, &counts),
            &subtitle,
            t.generated_on,
            &display_date(&first.issued_at, t),
        );
        if page_index == 0 {
            let half = (CONTENT_W - GAP) / 2.0;
            // Plusieurs machines : leur nombre, pas l'identité de la première.
            let machines = machines_count(certs);
            let machine: Vec<Row> = if machines > 1 {
                vec![Row::new(t.machines_processed, machines.to_string())]
            } else {
                vec![
                    Row::new(t.manufacturer, or_dash(&first.bench.manufacturer)),
                    Row::new(t.model, or_dash(&first.bench.model)),
                    Row::new(t.serial, or_dash(&first.bench.serial)),
                ]
            };
            let session = [
                Row::new(t.operator, or_dash(&first.operator)),
                Row::new(t.software, or_dash(&first.software)),
                Row::new(t.session_period, lot_period(certs, t)),
                Row::new(t.lot_id, lot_id.to_string()),
            ];
            let inner = half - 2.0 * CARD_PAD - LABEL_W;
            let h = (CARD_HEAD_H + 2.0 * CARD_PAD + rows_height(&machine, inner))
                .max(CARD_HEAD_H + 2.0 * CARD_PAD + rows_height(&session, inner));
            card(&mut canvas, MARGIN, y, half, t.card_machine, &machine, h);
            card(
                &mut canvas,
                MARGIN + half + GAP,
                y,
                half,
                t.card_session,
                &session,
                h,
            );
            y -= h + GAP;
        }
        let table_h = draw_lot_table(&mut canvas, t, y, &rows[start..end]);
        y -= table_h + GAP;

        // Attestations : calées au-dessus du pied de page.
        let key = &certs[0].signed;
        let attestation = [
            Row::new(
                t.attestations,
                fill(
                    t.attestations_value,
                    &[("n", &certs.len().to_string()), ("lot", lot_id)],
                ),
            ),
            Row::new(t.signing_key, signing_key_value(key, t)),
            Row::mono(t.public_key, key.public_key.clone()),
        ];
        let inner_w = CONTENT_W - 2.0 * CARD_PAD;
        let h = CARD_HEAD_H + 2.0 * CARD_PAD + rows_height(&attestation, inner_w - LABEL_W);
        let top = (MARGIN + FOOTER_H + 4.0 + h).min(y);
        card(
            &mut canvas,
            MARGIN,
            top,
            CONTENT_W,
            t.card_attestations,
            &attestation,
            h,
        );

        footer(
            &mut canvas,
            t,
            lot_id,
            &year_of(&first.issued_at),
            page_index + 1,
            total_pages,
        );
        pages.push(canvas);
    }
    pages
}

/// Rend un PDF complet en français : une page par certificat, précédée
/// d'une synthèse quand il y en a plusieurs.
pub fn render(certs: &[RenderedCertificate], lot_id: Option<&str>) -> Result<Vec<u8>> {
    render_with_logo(certs, lot_id, LOGO_JPEG, Lang::Fr)
}

/// Comme [`render`], avec un logo JPEG personnalisé et la langue de
/// rédaction. Un JPEG illisible (dimensions introuvables) retombe sur le
/// logo SecMet.
pub fn render_with_logo(
    certs: &[RenderedCertificate],
    lot_id: Option<&str>,
    logo_jpeg: &[u8],
    lang: Lang,
) -> Result<Vec<u8>> {
    render_impl(certs, lot_id, logo_jpeg, lang, false)
}

/// PDF d'un lot clôturé (Annexe M) : la synthèse de lot (identifiant,
/// tableau) est toujours présente, même pour une seule machine, car c'est
/// elle qui porte l'identifiant du lot et renvoie au manifeste.
pub fn render_lot(
    certs: &[RenderedCertificate],
    lot_id: &str,
    logo_jpeg: &[u8],
    lang: Lang,
) -> Result<Vec<u8>> {
    render_impl(certs, Some(lot_id), logo_jpeg, lang, true)
}

fn render_impl(
    certs: &[RenderedCertificate],
    lot_id: Option<&str>,
    logo_jpeg: &[u8],
    lang: Lang,
    force_summary: bool,
) -> Result<Vec<u8>> {
    let t = lang.texts();
    let (logo_jpeg, logo) = match jpeg_dimensions(logo_jpeg) {
        Some(dims) => (logo_jpeg, dims),
        None => (LOGO_JPEG, jpeg_dimensions(LOGO_JPEG).unwrap_or((1200, 342))),
    };
    if certs.is_empty() {
        return Err(crate::AttestError::Invalid(
            "aucun certificat à rendre".into(),
        ));
    }
    let summary_count = if force_summary || certs.len() > 1 {
        lot_pages(certs, lang).len()
    } else {
        0
    };
    // Bordereau de destruction : avec la synthèse seulement, pour les
    // supports à sortir du circuit.
    let slip_rows = if summary_count > 0 {
        destruction_rows(certs, lang)
    } else {
        Vec::new()
    };
    let slip_chunks = chunk_rows(&slip_rows, destruction_budget());
    let total = summary_count + slip_chunks.len() + certs.len();
    let mut pages = Vec::with_capacity(total);
    if summary_count > 0 {
        let lot = lot_id
            .map(str::to_string)
            .unwrap_or_else(|| format!("{}-lot", certs[0].signed.payload.session_id));
        pages.extend(summary_pages(certs, logo, &lot, total, lang));
        pages.extend(destruction_pages(
            certs,
            &slip_rows,
            &slip_chunks,
            logo,
            &lot,
            summary_count,
            total,
            lang,
        ));
    }
    let before = summary_count + slip_chunks.len();
    for (i, cert) in certs.iter().enumerate() {
        pages.push(certificate_page(cert, logo, before + i + 1, total, lang)?);
    }
    let first = &certs[0].signed.payload;
    let info = DocumentInfo {
        title: match lot_id {
            Some(lot) => fill(t.doc_title_lot, &[("lot", lot)]),
            None if certs.len() == 1 => fill(t.doc_title_single, &[("id", &first.certificate_id)]),
            None => fill(t.doc_title_session, &[("id", &first.session_id)]),
        },
        author: first.operator.clone(),
        created: first.issued_at.clone(),
    };
    Ok(assemble_with_logo(&pages, &info, logo_jpeg))
}

/// Nombre de pages d'un PDF produit par cette crate.
pub fn page_count(pdf: &[u8]) -> usize {
    let text = String::from_utf8_lossy(pdf);
    text.matches("/Type /Page /Parent").count()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::model::tests::sample;
    use crate::model::NistLevel;
    use crate::sign::SigningIdentity;

    fn contains(pdf: &[u8], needle: &[u8]) -> bool {
        pdf.windows(needle.len()).any(|w| w == needle)
    }

    fn rendered(n: usize) -> Vec<RenderedCertificate> {
        let id = SigningIdentity::from_seed(&[3u8; 32]);
        (0..n)
            .map(|i| {
                let mut a = sample();
                a.certificate_id = format!("SEC-TEST-{i}");
                a.disk.serial = format!("SN{i:04}");
                if i % 2 == 1 {
                    a.erasure.outcome = Outcome::Failed;
                    a.erasure.nist_level = NistLevel::NotClassified;
                }
                RenderedCertificate::new(id.sign(&a).unwrap()).unwrap()
            })
            .collect()
    }

    /// Vérifie la table xref octet par octet (les flux contiennent du WinAnsi,
    /// donc tout est compté en octets, jamais en caractères).
    fn check_structure(pdf: &[u8]) {
        assert!(pdf.starts_with(b"%PDF-1.4\n"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let find_last = |needle: &[u8]| {
            pdf.windows(needle.len())
                .rposition(|w| w == needle)
                .unwrap()
        };
        let start = find_last(b"startxref\n") + "startxref\n".len();
        let line = |from: usize| -> String {
            let end = pdf[from..].iter().position(|&b| b == b'\n').unwrap();
            String::from_utf8_lossy(&pdf[from..from + end]).into_owned()
        };
        let offset: usize = line(start).parse().unwrap();
        assert!(
            pdf[offset..].starts_with(b"xref\n"),
            "startxref pointe sur xref"
        );
        let header = line(offset + 5);
        let count: usize = header.split(' ').nth(1).unwrap().parse().unwrap();
        let mut cursor = offset + 5 + header.len() + 1 + 20; // après l'entrée 0
        for i in 1..count {
            let entry = line(cursor);
            let obj_offset: usize = entry[..10].parse().unwrap();
            let expected = format!("{i} 0 obj\n");
            assert!(
                pdf[obj_offset..].starts_with(expected.as_bytes()),
                "objet {i} à l'offset {obj_offset}"
            );
            cursor += 20;
        }
        // Chaque /Length annoncé correspond au flux réel.
        let mut from = 0;
        while let Some(pos) = pdf[from..].windows(10).position(|w| w == b"<< /Length") {
            let at = from + pos;
            let len: usize = line(at + 11).split(' ').next().unwrap().parse().unwrap();
            let stream_start =
                at + pdf[at..].windows(7).position(|w| w == b"stream\n").unwrap() + 7;
            assert!(
                pdf[stream_start + len..].starts_with(b"\nendstream"),
                "Length exact"
            );
            from = stream_start + len;
        }
    }

    #[test]
    fn winansi_encoding_and_escaping() {
        assert_eq!(encode_winansi("é(à)\\"), b"\xe9\\(\xe0\\)\\\\".to_vec());
        assert_eq!(encode_winansi("—€…→"), vec![0x97, 0x80, 0x85, b'>']);
        assert_eq!(encode_winansi("漢"), b"?".to_vec());
    }

    #[test]
    fn wrapping_respects_width_and_breaks_long_tokens() {
        let lines = wrap("un deux trois quatre cinq six", Font::Regular, 10.0, 80.0);
        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|l| Font::Regular.width(10.0, l) <= 80.0));
        let hex = "a".repeat(128);
        let lines = wrap(&hex, Font::Mono, 7.5, 200.0);
        assert_eq!(lines.iter().map(|l| l.len()).sum::<usize>(), 128);
        assert!(lines.len() >= 3);
        assert!(lines.iter().all(|l| Font::Mono.width(7.5, l) <= 200.0));
        assert_eq!(wrap("", Font::Regular, 10.0, 100.0), vec![String::new()]);
    }

    #[test]
    fn metrics_follow_the_afm_tables_and_jpeg_header_is_parsed() {
        assert!((Font::Regular.width(10.0, "A") - 6.67).abs() < 0.01);
        assert!((Font::Bold.width(10.0, "A") - 7.22).abs() < 0.01);
        assert!((Font::Mono.width(10.0, "AB") - 12.0).abs() < 0.01);
        assert_eq!(
            Font::Regular.width(10.0, "é"),
            Font::Regular.width(10.0, "e")
        );
        assert!(Font::Regular.width(10.0, "SecMet") > Font::Regular.width(10.0, "Sec"));
        let (w, h) = jpeg_dimensions(LOGO_JPEG).unwrap();
        assert!(w > h && w >= 600, "{w}x{h}");
        assert_eq!(jpeg_dimensions(b"not a jpeg"), None);
        assert_eq!(
            display_date("2026-10-08 11:04:12", Lang::Fr.texts()),
            "08/10/2026 \u{e0} 11:04:12"
        );
        assert_eq!(display_date("", Lang::Fr.texts()), "\u{2014}");
    }

    #[test]
    fn single_certificate_is_one_well_formed_page() {
        let certs = rendered(1);
        let pdf = render(&certs, None).unwrap();
        check_structure(&pdf);
        assert_eq!(page_count(&pdf), 1);
        let text = String::from_utf8_lossy(&pdf);
        // Info en UTF-16BE (BOM FEFF) : « C » = 0043, « J » = 004A.
        assert!(text.contains("/Title <FEFF0043006500720074"));
        assert!(text.contains("/Author <FEFF004A00650061006E"));
        assert!(text.contains("/CreationDate (D:20261008113000)"));
        assert!(text.contains("SN0000"));
        assert!(text.contains("PURGE"));
        assert!(text.contains("/F3"), "Courier pour les empreintes");
        assert!(text.contains(" re f\n"), "modules du QR");
        assert!(
            pdf.len() > 20_000 && pdf.len() < 400_000,
            "{} octets",
            pdf.len()
        );
    }

    #[test]
    fn a_lot_gets_a_summary_page_then_one_page_per_disk() {
        let certs = rendered(5);
        let pdf = render(&certs, Some("SEC-TEST-lot")).unwrap();
        check_structure(&pdf);
        assert_eq!(page_count(&pdf), 6);
        let text = String::from_utf8_lossy(&pdf);
        assert!(
            contains(&pdf, b"3 succ\xe8s, 2 \xe9chec\\(s\\)"),
            "bandeau du lot (parenth\u{e8}ses \u{e9}chapp\u{e9}es)"
        );
        // Le tiret cadratin est U+2014 en UTF-16, plus un octet WinAnsi.
        assert!(text.contains("2014"), "tiret cadratin encodé en UTF-16");
        assert!(
            !text.contains("/Title ("),
            "Info en chaîne hexadécimale UTF-16"
        );
        let big = rendered(60);
        let pages = lot_pages(&big, Lang::Fr);
        assert!(pages.len() >= 2, "{pages:?}");
        assert_eq!(pages[0].0, 0);
        assert_eq!(pages.last().unwrap().1, 60);
        assert!(pages.windows(2).all(|w| w[0].1 == w[1].0));
        let pdf = render(&big, None).unwrap();
        assert_eq!(page_count(&pdf), pages.len() + 60);
        assert_eq!(lot_pages(&rendered(2), Lang::Fr).len(), 1);
        assert!(render(&[], None).is_err());
    }

    /// Un JPEG minimal (SOF0 300×80) pour le logo personnalisé : seul
    /// l'en-tête est inspecté par le générateur.
    fn tiny_jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        jpeg.extend_from_slice(&height.to_be_bytes());
        jpeg.extend_from_slice(&width.to_be_bytes());
        jpeg.extend_from_slice(&[0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        jpeg.extend_from_slice(b"CUSTOM-LOGO-PAYLOAD");
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        jpeg
    }

    #[test]
    fn display_date_never_slices_inside_a_character() {
        assert_eq!(
            display_date("2026-10-08 11:04:12", Lang::Fr.texts()),
            "08/10/2026 à 11:04:12"
        );
        assert_eq!(
            display_date("2026-10-0é9 11:30:00", Lang::Fr.texts()),
            "2026-10-0é9 11:30:00"
        );
        assert_eq!(
            display_date("éééééééééééééééééééé", Lang::Fr.texts()),
            "éééééééééééééééééééé"
        );
        assert_eq!(display_date("", Lang::Fr.texts()), "—");
        let many: Vec<String> = (0..20).map(|i| format!("sig{i}")).collect();
        let text = bounded_list(&many, 8, Lang::Fr.texts());
        assert!(text.starts_with("sig0, sig1") && text.contains("+12 autres"));
        assert_eq!(truncate_chars("abcdef", 4), "abc…");
    }

    /// Les libellés ne sont pas repliés : chacun doit tenir dans sa colonne,
    /// dans toutes les langues.
    /// Annexe M : un lot de plusieurs machines ne montre pas l'identité de
    /// la première comme si c'était celle du lot.
    #[test]
    fn lot_of_several_machines_shows_count_and_per_row_serials() {
        let id = SigningIdentity::from_seed(&[5u8; 32]);
        let mut a = sample();
        a.bench.serial = "LAPTOP-AAA".into();
        a.bench.manufacturer = "Dell".into();
        let mut b = sample();
        b.certificate_id = "SEC-TEST-0001-sdb".into();
        b.bench.serial = "LAPTOP-BBB".into();
        b.bench.manufacturer = "Lenovo".into();
        let certs: Vec<RenderedCertificate> = [a.clone(), b]
            .iter()
            .map(|p| RenderedCertificate::new(id.sign(p).unwrap()).unwrap())
            .collect();
        let pdf = render(&certs, Some("SEC-TEST-0001-lot")).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(
            text.contains("Machines trait"),
            "nombre de machines attendu"
        );
        // `wrap` coupe « Machine S/N » et la série sur deux lignes.
        assert!(text.contains("Machine S/N"));
        assert!(text.contains("LAPTOP-AAA") && text.contains("LAPTOP-BBB"));

        // Une seule machine : identité complète, pas de ligne S/N dans le tableau.
        let single = vec![RenderedCertificate::new(id.sign(&a).unwrap()).unwrap()];
        let pdf = render(&single, Some("SEC-TEST-0001-lot")).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("Dell") && !text.contains("Machines trait"));
        assert!(!text.contains("Machine S/N"));
    }

    #[test]
    fn closed_lot_always_has_its_summary_even_for_one_machine() {
        let id = SigningIdentity::from_seed(&[6u8; 32]);
        let one = vec![RenderedCertificate::new(id.sign(&sample()).unwrap()).unwrap()];
        let grouped = render(&one, Some("SEC-TEST-0001-lot")).unwrap();
        let lot = render_lot(&one, "L-20261030-TEST", LOGO_JPEG, Lang::Fr).unwrap();
        // Mode groupé classique : un seul certificat → pas de synthèse.
        assert!(!String::from_utf8_lossy(&grouped).contains("SEC-TEST-0001-lot)"));
        // Lot clôturé : la synthèse porte l'identifiant du lot.
        assert!(String::from_utf8_lossy(&lot).contains("L-20261030-TEST"));
        assert!(lot.len() > grouped.len());
    }

    #[test]
    fn condemned_disks_get_a_destruction_slip_after_the_summary() {
        use crate::model::Disposition;
        let id = SigningIdentity::from_seed(&[7u8; 32]);
        let certs: Vec<RenderedCertificate> = (0..3)
            .map(|i| {
                let mut a = sample();
                a.certificate_id = format!("SEC-SLIP-{i}");
                a.disk.serial = format!("SLIP{i:04}");
                a.erasure.disposition = Some(match i {
                    0 => Disposition::Reusable,
                    1 => Disposition::DestroyRecommended,
                    _ => Disposition::DestroyRequired,
                });
                if i == 2 {
                    a.erasure.outcome = Outcome::Failed;
                    a.erasure.nist_level = NistLevel::NotClassified;
                    a.erasure.disposition_reasons = vec!["UnreadableSectors".into()];
                } else if i == 1 {
                    a.erasure.disposition_reasons = vec!["SmartFailing".into()];
                }
                RenderedCertificate::new(id.sign(&a).unwrap()).unwrap()
            })
            .collect();
        let pdf = render_lot(&certs, "L-20261103-SLIP", LOGO_JPEG, Lang::Fr).unwrap();
        check_structure(&pdf);
        // Synthèse + bordereau + trois certificats.
        assert_eq!(page_count(&pdf), 1 + 1 + 3);
        assert!(contains(&pdf, b"BORDEREAU DE DESTRUCTION PHYSIQUE"));
        assert!(contains(&pdf, b"2 support\\(s\\)"));
        assert!(contains(&pdf, b"SLIP0002"));
        // Sans support à détruire : pas de bordereau.
        let fine = rendered(2);
        let pdf = render_lot(&fine, "L-20261103-FINE", LOGO_JPEG, Lang::Fr).unwrap();
        assert_eq!(page_count(&pdf), 1 + 2);
        assert!(!contains(&pdf, b"BORDEREAU"));
        for lang in Lang::ALL {
            let pdf = render_lot(&certs, "L-20261103-SLIP", LOGO_JPEG, lang).unwrap();
            check_structure(&pdf);
            assert_eq!(page_count(&pdf), 5, "{lang:?}");
        }
    }

    #[test]
    fn every_language_fits_its_fixed_columns() {
        for lang in Lang::ALL {
            let t = lang.texts();
            for label in t.row_labels() {
                assert!(
                    Font::Regular.width(ROW_SIZE, label) <= LABEL_W - 4.0,
                    "{lang:?} : libellé trop large « {label} »"
                );
            }
            for (title, w) in lot_titles(t).into_iter().zip(LOT_WIDTHS) {
                assert!(
                    Font::Bold.width(7.0, title) + LOT_CELL_PAD <= w,
                    "{lang:?} : titre de colonne trop large « {title} »"
                );
            }
            for result in [t.lot_success, t.lot_failed] {
                assert!(
                    Font::Bold.width(LOT_CELL_SIZE, result) + 2.0 * LOT_CELL_PAD <= LOT_WIDTHS[8],
                    "{lang:?} : résultat replié « {result} »"
                );
            }
        }
    }

    #[test]
    fn the_pdf_is_written_in_the_requested_language() {
        let certs = rendered(3);
        let render_in = |lang| render_with_logo(&certs, Some("SEC-TEST-lot"), LOGO_JPEG, lang);
        let fr = render_in(Lang::Fr).unwrap();
        assert_eq!(fr, render(&certs, Some("SEC-TEST-lot")).unwrap());
        assert!(contains(&fr, b"CERTIFICAT D'EFFACEMENT"));

        let en = render_in(Lang::En).unwrap();
        check_structure(&en);
        assert_eq!(page_count(&en), page_count(&fr));
        for needle in [
            &b"ERASURE CERTIFICATE"[..],
            b"OPERATION DETAILS",
            b"Report ID: SEC-TEST-0",
            b"NOT CLASSIFIED",
            b"2026-10-08 at",
        ] {
            assert!(contains(&en, needle), "{}", String::from_utf8_lossy(needle));
        }
        assert!(!contains(&en, b"Op\xe9rateur"));

        let de = render_in(Lang::De).unwrap();
        check_structure(&de);
        for needle in [
            &b"L\xd6SCHZERTIFIKAT"[..],
            b"PR\xdcFUNG DURCH R\xdcCKLESEN",
            b"Berichts-ID: SEC-TEST-0",
            b"NICHT KLASSIFIZIERT",
            b"08.10.2026 um",
        ] {
            assert!(contains(&de, needle), "{}", String::from_utf8_lossy(needle));
        }
    }

    #[test]
    fn custom_logo_is_embedded_and_invalid_logo_falls_back() {
        let logo = tiny_jpeg(300, 80);
        assert_eq!(jpeg_dimensions(&logo), Some((300, 80)));
        let pdf = render_with_logo(&rendered(1), None, &logo, Lang::Fr).unwrap();
        assert!(contains(&pdf, b"CUSTOM-LOGO-PAYLOAD"));
        assert!(contains(&pdf, b"/Width 300 /Height 80"));
        assert_eq!(page_count(&pdf), 1);

        let fallback = render_with_logo(&rendered(1), None, b"not a jpeg", Lang::Fr).unwrap();
        assert!(!contains(&fallback, b"CUSTOM-LOGO-PAYLOAD"));
        let (w, h) = jpeg_dimensions(LOGO_JPEG).unwrap();
        assert!(contains(
            &fallback,
            format!("/Width {w} /Height {h}").as_bytes()
        ));
        assert_eq!(render(&rendered(1), None).unwrap(), fallback);
    }
}
