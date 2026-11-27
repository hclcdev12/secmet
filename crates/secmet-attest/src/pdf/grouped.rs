//! PDF de synthèse d'un regroupement de lots ([`crate::consolidation`]) :
//! bilan global, tableau des lots, tableau de tous les disques, liste des
//! fichiers d'origine joints avec leurs empreintes. Quelques pages pour
//! une centaine de disques ; le détail par disque reste dans les PDF et
//! les attestations des lots, recopiés dans `sources/`.
//!
//! Document **non signé** : il le dit sur chaque page.

use super::{
    assemble_with_logo, display_date, draw_table, header, jpeg_dimensions, level_label, or_dash,
    wrap, Canvas, DocumentInfo, Font, LotRow, Rgb, AMBER, AMBER_BG, CARD_HEAD_H, CARD_PAD,
    CONTENT_W, FOOTER_H, GAP, GREEN, GREEN_BG, INK, LABEL_W, LOGO_JPEG, LOT_CELL_PAD,
    LOT_CELL_SIZE, LOT_HEAD_H, LOT_LINE, MARGIN, MUTED, RED, RED_BG, RIGHT, RULE,
};
use super::{banner, card, rows_height, Row};
use crate::consolidation::{EntryStatus, Grouping, Index};
use crate::lang::{fill, Lang, Texts};
use crate::model::Outcome;
use crate::Result;

/// Libellés du PDF de regroupement.
struct GroupTexts {
    title: &'static str,
    subtitle: &'static str,
    /// `{success}`, `{n}`.
    banner: &'static str,
    /// `{lots}`, `{keys}`.
    banner_detail: &'static str,
    card_grouping: &'static str,
    reference: &'static str,
    no_reference: &'static str,
    period: &'static str,
    operators: &'static str,
    lots: &'static str,
    keys: &'static str,
    card_totals: &'static str,
    disks: &'static str,
    success: &'static str,
    failed: &'static str,
    redone: &'static str,
    destroy: &'static str,
    lots_title: &'static str,
    lot_columns: [&'static str; 6],
    disks_title: &'static str,
    disk_columns: [&'static str; 7],
    redone_tag: &'static str,
    /// `{id}`.
    redone_by: &'static str,
    warnings_title: &'static str,
    files_title: &'static str,
    /// `{index}`.
    files_intro: &'static str,
    file_columns: [&'static str; 2],
    /// `{year}`.
    footer_notice: &'static str,
    /// `{id}`.
    footer_id: &'static str,
    /// `{id}`.
    doc_title: &'static str,
}

const FR: GroupTexts = GroupTexts {
    title: "REGROUPEMENT DE LOTS",
    subtitle: "Synthèse de plusieurs lots SecMet clôturés",
    banner: "Disques effacés : {success} sur {n}",
    banner_detail: "Lots regroupés : {lots} · clés de signature : {keys}. Synthèse non signée : \
                    chaque lot se prouve par son manifeste signé, joint dans sources/.",
    card_grouping: "Regroupement",
    reference: "Référence client",
    no_reference: "aucune ou différente selon les lots",
    period: "Période",
    operators: "Opérateurs",
    lots: "Lots",
    keys: "Clés de signature",
    card_totals: "Bilan",
    disks: "Disques (uniques)",
    success: "Réussis",
    failed: "Échecs",
    redone: "Repassés",
    destroy: "À détruire",
    lots_title: "Lots regroupés",
    lot_columns: ["LOT", "CLÉ", "OPÉRATEUR", "CLÔTURE", "DISQUES", "LOGICIEL"],
    disks_title: "Disques",
    disk_columns: [
        "MODÈLE",
        "SÉRIE",
        "TAILLE",
        "MÉTHODE",
        "RÉSULTAT",
        "LOT",
        "CERTIFICAT",
    ],
    redone_tag: "Repassé",
    redone_by: "remplacé par {id}",
    warnings_title: "Avertissements",
    files_title: "Fichiers joints",
    files_intro: "Fichiers d'origine des lots, recopiés à l'identique dans sources/. Le \
                  manifeste signé de chaque lot fixe l'empreinte de chacun de ses certificats ; \
                  il se vérifie avec secmet-verify --manifest. La liste complète des fichiers et \
                  de leurs empreintes est dans {index}.",
    file_columns: ["FICHIER", "SHA-256"],
    footer_notice: "SecMet © {year} — Synthèse de regroupement non signée : seuls les manifestes \
                    et certificats signés joints font foi.",
    footer_id: "Regroupement : {id}",
    doc_title: "Regroupement de lots {id}",
};

const EN: GroupTexts = GroupTexts {
    title: "GROUPED LOTS",
    subtitle: "Summary of several closed SecMet lots",
    banner: "Disks erased: {success} of {n}",
    banner_detail: "Lots grouped: {lots} · signing keys: {keys}. Unsigned summary: each lot \
                    is proven by its signed manifest, attached in sources/.",
    card_grouping: "Grouping",
    reference: "Customer reference",
    no_reference: "none, or different across lots",
    period: "Period",
    operators: "Operators",
    lots: "Lots",
    keys: "Signing keys",
    card_totals: "Totals",
    disks: "Disks (unique)",
    success: "Succeeded",
    failed: "Failures",
    redone: "Redone",
    destroy: "To destroy",
    lots_title: "Grouped lots",
    lot_columns: ["LOT", "KEY", "OPERATOR", "CLOSED", "DISKS", "SOFTWARE"],
    disks_title: "Disks",
    disk_columns: [
        "MODEL",
        "SERIAL",
        "SIZE",
        "METHOD",
        "RESULT",
        "LOT",
        "CERTIFICATE",
    ],
    redone_tag: "Redone",
    redone_by: "replaced by {id}",
    warnings_title: "Warnings",
    files_title: "Attached files",
    files_intro: "Original lot files, copied unchanged into sources/. Each lot's signed \
                  manifest fixes the digest of every one of its certificates; check it with \
                  secmet-verify --manifest. The full list of files and digests is in {index}.",
    file_columns: ["FILE", "SHA-256"],
    footer_notice: "SecMet © {year} — Unsigned grouping summary: only the attached signed \
                    manifests and certificates are authoritative.",
    footer_id: "Grouping: {id}",
    doc_title: "Grouped lots {id}",
};

const DE: GroupTexts = GroupTexts {
    title: "ZUSAMMENGEFASSTE LOSE",
    subtitle: "Übersicht mehrerer abgeschlossener SecMet-Lose",
    banner: "Gelöschte Datenträger: {success} von {n}",
    banner_detail: "Zusammengefasste Lose: {lots} · Signaturschlüssel: {keys}. Nicht signierte \
                    Übersicht: Jedes Los wird durch sein signiertes Manifest in sources/ belegt.",
    card_grouping: "Zusammenfassung",
    reference: "Kundenreferenz",
    no_reference: "keine oder je Los verschieden",
    period: "Zeitraum",
    operators: "Bediener",
    lots: "Lose",
    keys: "Schlüssel",
    card_totals: "Bilanz",
    disks: "Datenträger",
    success: "Erfolgreich",
    failed: "Fehlschläge",
    redone: "Wiederholt",
    destroy: "Zu vernichten",
    lots_title: "Zusammengefasste Lose",
    lot_columns: [
        "LOS",
        "SCHLÜSSEL",
        "BEDIENER",
        "ABSCHLUSS",
        "DISKS",
        "SOFTWARE",
    ],
    disks_title: "Datenträger",
    disk_columns: [
        "MODELL",
        "SERIE",
        "GRÖSSE",
        "METHODE",
        "ERGEBNIS",
        "LOS",
        "ZERTIFIKAT",
    ],
    redone_tag: "Wiederholt",
    redone_by: "ersetzt durch {id}",
    warnings_title: "Hinweise",
    files_title: "Beigefügte Dateien",
    files_intro: "Originaldateien der Lose, unverändert nach sources/ kopiert. Das signierte \
                  Manifest jedes Loses legt den Hashwert jedes seiner Zertifikate fest; geprüft \
                  wird es mit secmet-verify --manifest. Die vollständige Liste der Dateien und \
                  Hashwerte steht in {index}.",
    file_columns: ["DATEI", "SHA-256"],
    footer_notice: "SecMet © {year} — Nicht signierte Übersicht: Maßgeblich sind allein die \
                    beigefügten signierten Manifeste und Zertifikate.",
    footer_id: "Zusammenfassung: {id}",
    doc_title: "Zusammengefasste Lose {id}",
};

fn group_texts(lang: Lang) -> &'static GroupTexts {
    match lang {
        Lang::Fr => &FR,
        Lang::En => &EN,
        Lang::De => &DE,
    }
}

/// Lots : n°, lot, clé, opérateur, clôture, disques, logiciel.
const LOT_W: [f64; 7] = [22.0, 80.0, 80.0, 82.0, 90.0, 42.0, 115.0];
/// Disques : n°, modèle, série, taille, méthode, résultat, lot, certificat.
const DISK_W: [f64; 8] = [20.0, 80.0, 76.0, 40.0, 108.0, 46.0, 70.0, 71.0];
/// Fichiers : chemin, empreinte.
const FILE_W: [f64; 2] = [290.0, 221.0];
/// Aucune colonne « résultat » colorée.
const NO_COL: usize = usize::MAX;

/// Ligne de tableau : cellules repliées à leur largeur.
fn row(values: &[String], widths: &[f64], mono_col: usize, color: Rgb) -> LotRow {
    let cells: Vec<Vec<String>> = values
        .iter()
        .zip(widths)
        .enumerate()
        .map(|(col, (value, w))| {
            let font = if col == mono_col {
                Font::Mono
            } else {
                Font::Regular
            };
            wrap(value, font, LOT_CELL_SIZE, w - 2.0 * LOT_CELL_PAD)
        })
        .collect();
    let lines = cells.iter().map(Vec::len).max().unwrap_or(1).max(1) as f64;
    LotRow {
        cells,
        result_color: color,
        height: lines * LOT_LINE + 2.0 * LOT_CELL_PAD,
    }
}

/// Mise en pages : en-tête sur chaque page, tableaux coupés en bas de page
/// avec leurs titres de colonnes répétés.
struct Pager {
    pages: Vec<Canvas>,
    canvas: Canvas,
    y: f64,
    fresh: bool,
    logo: (u32, u32),
    title: &'static str,
    subtitle: &'static str,
}

const BOTTOM: f64 = MARGIN + FOOTER_H + 4.0;

impl Pager {
    fn new(logo: (u32, u32), title: &'static str, subtitle: &'static str) -> Self {
        let mut canvas = Canvas::default();
        let y = header(&mut canvas, logo, title, subtitle);
        Self {
            pages: Vec::new(),
            canvas,
            y,
            fresh: true,
            logo,
            title,
            subtitle,
        }
    }

    fn new_page(&mut self) {
        let done = std::mem::take(&mut self.canvas);
        self.pages.push(done);
        self.y = header(&mut self.canvas, self.logo, self.title, self.subtitle);
        self.fresh = true;
    }

    /// Titre de section ; page suivante s'il ne reste pas la place d'un
    /// début de tableau sous le titre.
    fn section(&mut self, title: &str) {
        if self.y - 16.0 - LOT_HEAD_H - 3.0 * LOT_LINE < BOTTOM && !self.fresh {
            self.new_page();
        }
        self.canvas
            .text(MARGIN, self.y - 10.0, Font::Bold, 10.0, INK, title);
        self.y -= 18.0;
        self.fresh = false;
    }

    /// Paragraphe en petit corps.
    fn paragraph(&mut self, text: &str, color: Rgb) {
        for line in wrap(text, Font::Regular, 8.0, CONTENT_W) {
            if self.y - 11.0 < BOTTOM {
                self.new_page();
            }
            self.canvas
                .text(MARGIN, self.y - 8.0, Font::Regular, 8.0, color, &line);
            self.y -= 11.0;
        }
        self.y -= GAP / 2.0;
        self.fresh = false;
    }

    fn table(
        &mut self,
        titles: &[&str],
        widths: &[f64],
        rows: &[LotRow],
        result_col: usize,
        mono_col: usize,
    ) {
        let mut start = 0;
        while start < rows.len() {
            let mut used = LOT_HEAD_H;
            let mut end = start;
            while end < rows.len() && self.y - used - rows[end].height >= BOTTOM {
                used += rows[end].height;
                end += 1;
            }
            if end == start {
                if self.fresh {
                    // Ligne plus haute qu'une page : dessinée quand même.
                    used += rows[end].height;
                    end += 1;
                } else {
                    self.new_page();
                    continue;
                }
            }
            draw_table(
                &mut self.canvas,
                self.y,
                titles,
                widths,
                &rows[start..end],
                result_col,
                mono_col,
            );
            self.y -= used + GAP;
            self.fresh = false;
            start = end;
            if start < rows.len() {
                self.new_page();
            }
        }
    }

    fn finish(mut self) -> Vec<Canvas> {
        let last = std::mem::take(&mut self.canvas);
        self.pages.push(last);
        self.pages
    }
}

/// Pied de page du regroupement : mention « non signé » sur chaque page.
fn footer(
    canvas: &mut Canvas,
    g: &GroupTexts,
    t: &Texts,
    id: &str,
    year: &str,
    page: usize,
    total: usize,
) {
    let y = MARGIN + FOOTER_H - 10.0;
    canvas.line(MARGIN, y + 8.0, RIGHT, y + 8.0, 0.6, RULE);
    canvas.text(
        MARGIN,
        y - 4.0,
        Font::Regular,
        7.0,
        MUTED,
        &fill(g.footer_notice, &[("year", year)]),
    );
    canvas.text(
        MARGIN,
        y - 15.0,
        Font::Regular,
        7.0,
        MUTED,
        &fill(g.footer_id, &[("id", id)]),
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

/// Rend le PDF de synthèse d'un regroupement. `logo_jpeg` illisible :
/// logo SecMet.
pub fn render_grouping(
    grouping: &Grouping,
    index: &Index,
    logo_jpeg: &[u8],
    lang: Lang,
) -> Result<Vec<u8>> {
    if grouping.entries.is_empty() {
        return Err(crate::AttestError::Invalid(
            "regroupement sans certificat".into(),
        ));
    }
    let t = lang.texts();
    let g = group_texts(lang);
    let (logo_jpeg, logo) = match jpeg_dimensions(logo_jpeg) {
        Some(dims) => (logo_jpeg, dims),
        None => (LOGO_JPEG, jpeg_dimensions(LOGO_JPEG).unwrap_or((1200, 342))),
    };
    let totals = grouping.totals();
    let mut pager = Pager::new(logo, g.title, g.subtitle);

    // Bandeau.
    let tone = if totals.failed == 0 {
        (GREEN, GREEN_BG)
    } else if totals.success == 0 {
        (RED, RED_BG)
    } else {
        (AMBER, AMBER_BG)
    };
    pager.y = banner(
        &mut pager.canvas,
        pager.y,
        tone,
        &fill(
            g.banner,
            &[
                ("success", &totals.success.to_string()),
                ("n", &totals.disks.to_string()),
            ],
        ),
        &fill(
            g.banner_detail,
            &[
                ("lots", &grouping.lots.len().to_string()),
                ("keys", &grouping.key_ids().len().to_string()),
            ],
        ),
        t.generated_on,
        &display_date(&index.generated_at, t),
    );

    // Cartes « Regroupement » et « Bilan ».
    let reference = grouping.reference();
    let period = match grouping.period() {
        Some((s, e)) => format!("{} – {}", display_date(&s, t), display_date(&e, t)),
        None => "—".into(),
    };
    let left = [
        Row::new(
            g.reference,
            if reference.is_empty() {
                g.no_reference.to_string()
            } else {
                reference
            },
        ),
        Row::new(g.period, period),
        Row::new(g.operators, or_dash(&grouping.operators().join(", "))),
        Row::new(g.lots, grouping.lots.len().to_string()),
        Row::new(g.keys, grouping.key_ids().len().to_string()),
    ];
    let mut right = vec![
        Row::new(g.disks, totals.disks.to_string()),
        Row::new(g.success, totals.success.to_string()).colored(GREEN),
        Row::new(g.failed, totals.failed.to_string()).colored(if totals.failed > 0 {
            RED
        } else {
            INK
        }),
        Row::new(g.redone, totals.redone.to_string()),
    ];
    if totals.destroy > 0 {
        right.push(Row::new(g.destroy, totals.destroy.to_string()).colored(RED));
    }
    let half = (CONTENT_W - GAP) / 2.0;
    let inner = half - 2.0 * CARD_PAD - LABEL_W;
    let h = (CARD_HEAD_H + 2.0 * CARD_PAD + rows_height(&left, inner))
        .max(CARD_HEAD_H + 2.0 * CARD_PAD + rows_height(&right, inner));
    card(
        &mut pager.canvas,
        MARGIN,
        pager.y,
        half,
        g.card_grouping,
        &left,
        h,
    );
    card(
        &mut pager.canvas,
        MARGIN + half + GAP,
        pager.y,
        half,
        g.card_totals,
        &right,
        h,
    );
    pager.y -= h + GAP;
    pager.fresh = false;

    if !grouping.warnings.is_empty() {
        pager.section(g.warnings_title);
        for w in &grouping.warnings {
            pager.paragraph(&format!("• {w}"), AMBER);
        }
    }

    // Lots.
    pager.section(g.lots_title);
    let lot_rows: Vec<LotRow> = grouping
        .lots
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let p = &l.manifest.payload;
            row(
                &[
                    (i + 1).to_string(),
                    l.lot_id().to_string(),
                    l.key_id().to_string(),
                    or_dash(&p.operator),
                    display_date(&p.closed_at, t),
                    l.certificates.len().to_string(),
                    or_dash(&p.software),
                ],
                &LOT_W,
                2,
                INK,
            )
        })
        .collect();
    let mut lot_titles = vec!["#"];
    lot_titles.extend(g.lot_columns);
    pager.table(&lot_titles, &LOT_W, &lot_rows, NO_COL, 2);

    // Disques.
    pager.section(g.disks_title);
    let disk_rows: Vec<LotRow> = grouping
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let a = &grouping.certificate(e).payload;
            let (result, color) = match (&e.status, a.erasure.outcome) {
                (EntryStatus::Redone { .. }, _) => (g.redone_tag, MUTED),
                (_, Outcome::Success) => (t.lot_success, GREEN),
                (_, Outcome::Failed) => (t.lot_failed, RED),
                (_, Outcome::Cancelled) => (t.lot_cancelled, AMBER),
            };
            let method = if a.erasure.outcome == Outcome::Success {
                format!(
                    "{} — NIST {}",
                    a.erasure.method_label,
                    level_label(a.erasure.nist_level, t)
                )
            } else {
                a.erasure.method_label.clone()
            };
            let mut certificate = a.certificate_id.clone();
            if let EntryStatus::Redone { by } = &e.status {
                certificate = format!("{certificate} ({})", fill(g.redone_by, &[("id", by)]));
            }
            row(
                &[
                    (i + 1).to_string(),
                    or_dash(&a.disk.model),
                    or_dash(&a.disk.serial),
                    or_dash(&a.disk.capacity),
                    method,
                    result.to_string(),
                    grouping.lots[e.lot].lot_id().to_string(),
                    certificate,
                ],
                &DISK_W,
                2,
                color,
            )
        })
        .collect();
    let mut disk_titles = vec!["#"];
    disk_titles.extend(g.disk_columns);
    pager.table(&disk_titles, &DISK_W, &disk_rows, 5, 2);

    // Fichiers joints.
    pager.section(g.files_title);
    pager.paragraph(
        &fill(
            g.files_intro,
            &[("index", &format!("{}.json", index.grouping_id))],
        ),
        MUTED,
    );
    // Par lot, les fichiers qui en portent la preuve : le manifeste signé
    // (il fixe l'empreinte de chaque certificat), le PDF du lot et la clé
    // publique. La liste complète est dans l'index JSON.
    let file_rows: Vec<LotRow> = index
        .lots
        .iter()
        .flat_map(|l| l.files.iter())
        .filter(|f| {
            f.path.ends_with(crate::consolidation::MANIFEST_SUFFIX)
                || f.path.ends_with("-lot.pdf")
                || f.path.ends_with(".pub")
        })
        .map(|f| row(&[f.path.clone(), f.sha256.clone()], &FILE_W, 1, INK))
        .collect();
    pager.table(&g.file_columns, &FILE_W, &file_rows, NO_COL, 1);

    let mut pages = pager.finish();
    let total = pages.len();
    let year: String = index.generated_at.chars().take(4).collect();
    for (i, canvas) in pages.iter_mut().enumerate() {
        footer(canvas, g, t, &index.grouping_id, &year, i + 1, total);
    }
    let info = DocumentInfo {
        title: fill(g.doc_title, &[("id", &index.grouping_id)]),
        author: grouping.operators().join(", "),
        created: index.generated_at.clone(),
    };
    Ok(assemble_with_logo(&pages, &info, logo_jpeg))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::consolidation::{build, load_lot, tests::write_lot};
    use crate::pdf::page_count;

    fn grouping(disks_per_lot: usize) -> (tempfile::TempDir, Grouping) {
        let tmp = tempfile::tempdir().unwrap();
        let mut lots = Vec::new();
        for k in 1..=5u8 {
            let serials: Vec<String> = (0..disks_per_lot).map(|i| format!("K{k}-S{i}")).collect();
            let disks: Vec<(&str, bool, &str)> = serials
                .iter()
                .map(|s| (s.as_str(), true, "2026-11-26 10:00:00"))
                .collect();
            let dir = write_lot(
                tmp.path(),
                k,
                &format!("L-20261126-K{k}"),
                "Mission 42",
                &disks,
            );
            lots.push(load_lot(&dir).unwrap());
        }
        (tmp, build(lots).unwrap())
    }

    #[test]
    fn hundred_laptops_on_five_keys_fit_in_a_few_pages() {
        let (_tmp, g) = grouping(20);
        let index = g.index("2026-11-27 09:00:00", "secmet-consolidate 0.5.1");
        let pdf = render_grouping(&g, &index, LOGO_JPEG, Lang::Fr).unwrap();
        let pages = page_count(&pdf);
        // Bilan, lots, 100 lignes de disques et ~210 fichiers : quelques
        // pages, très loin d'une page par disque.
        assert!((3..=12).contains(&pages), "{pages} pages");
        assert!(pdf.starts_with(b"%PDF-"));
        if let Some(out) = std::env::var_os("SECMET_GROUPED_PDF") {
            std::fs::write(out, &pdf).unwrap();
        }
        for lang in Lang::ALL {
            render_grouping(&g, &index, LOGO_JPEG, lang).unwrap();
        }
    }

    #[test]
    fn small_grouping_is_well_formed_and_says_unsigned() {
        let (_tmp, g) = grouping(1);
        let index = g.index("2026-11-27 09:00:00", "x");
        let pdf = render_grouping(&g, &index, b"pas un jpeg", Lang::Fr).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("non sign"));
        assert!(text.contains(&index.grouping_id));
    }
}
