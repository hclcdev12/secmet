//! Table attributs SMART (style maquette SecMet).
//!
//! Un seul `Grid` porte l'en-tête ET toutes les lignes : GTK calcule les
//! largeurs de colonnes sur l'ensemble du tableau, l'alignement en-têtes /
//! valeurs est donc garanti même quand la fenêtre est redimensionnée. Les
//! colonnes `expand` (nom + raw) absorbent seules l'élargissement.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, EventControllerMotion, Grid, Label, Orientation, ScrolledWindow, Widget,
};

use crate::disk_info::smart_models::{AttrStatus, DiskProtocol, DiskSmartReport};
use crate::i18n::tr;

use super::theme::attr_status_color_hex;

/// Largeurs minimales (px) par colonne — repères maquette.
const COL_W_ID: i32 = 38;
const COL_W_NAME: i32 = 230;
const COL_W_NUM: i32 = 52;
const COL_W_VAL: i32 = 90;

struct ColSpec {
    header: String,
    min_width: i32,
    center: bool,
    /// Absorbe une part de l'élargissement de la fenêtre.
    expand: bool,
}

fn col_spec(header: &str, min_width: i32, center: bool, expand: bool) -> ColSpec {
    ColSpec {
        header: tr(header),
        min_width,
        center,
        expand,
    }
}

#[derive(Clone, Copy)]
enum CellCss {
    None,
    Good,
    Thresh,
    RawZero,
    RawData,
}

impl CellCss {
    fn class(self) -> Option<&'static str> {
        match self {
            CellCss::None => None,
            CellCss::Good => Some("secmet-smart-v-good"),
            CellCss::Thresh => Some("secmet-smart-v-thresh"),
            CellCss::RawZero => Some("secmet-smart-v-raw-zero"),
            CellCss::RawData => Some("secmet-smart-v-raw-data"),
        }
    }
}

struct DataCell {
    text: String,
    css: CellCss,
    /// Valeur potentiellement longue (raw) : ellipse au-delà d'une taille max.
    long: bool,
}

impl DataCell {
    fn new(text: String, css: CellCss) -> Self {
        Self {
            text,
            css,
            long: false,
        }
    }

    fn long(mut self) -> Self {
        self.long = true;
        self
    }
}

/// Ligne : `Some(id)` (ATA/NVMe, colonne ID) ou `None` (SCSI : point d'état
/// seul sous « État », le nom passant sous « Op »).
struct DataRow {
    status: AttrStatus,
    id: Option<String>,
    name: String,
    cells: Vec<DataCell>,
}

pub fn build_attributes(report: &DiskSmartReport) -> ScrolledWindow {
    let scroll = ScrolledWindow::new();
    scroll.add_css_class("secmet-smart-scroll");
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);

    if matches!(report.identity.protocol, DiskProtocol::Unknown) {
        let lbl = Label::new(Some(&tr(
            "Aucun attribut SMART disponible pour ce protocole.",
        )));
        lbl.add_css_class("secmet-smart-empty");
        lbl.set_xalign(0.0);
        lbl.set_margin_top(12);
        lbl.set_margin_start(12);
        let wrap = GtkBox::new(Orientation::Vertical, 0);
        wrap.append(&lbl);
        scroll.set_child(Some(&wrap));
        return scroll;
    }

    let (cols, rows) = table_content(report);
    scroll.set_child(Some(&build_table(&cols, &rows)));
    scroll
}

fn table_content(report: &DiskSmartReport) -> (Vec<ColSpec>, Vec<DataRow>) {
    match report.identity.protocol {
        DiskProtocol::Ata => {
            let cols = vec![
                col_spec("ID", COL_W_ID, true, false),
                col_spec("Attribute Name", COL_W_NAME, false, true),
                col_spec("Current", COL_W_NUM, true, false),
                col_spec("Worst", COL_W_NUM, true, false),
                col_spec("Thresh", COL_W_NUM, true, false),
                col_spec("Raw Values", COL_W_VAL, true, true),
            ];
            let rows = report
                .attributes
                .iter()
                .map(|attr| {
                    let cur_good = attr.value >= attr.threshold.max(1) || attr.threshold == 0;
                    let num_css = if cur_good {
                        CellCss::Good
                    } else {
                        CellCss::None
                    };
                    let raw_css = if attr.raw_string == "0" {
                        CellCss::RawZero
                    } else {
                        CellCss::RawData
                    };
                    DataRow {
                        status: attr.status,
                        id: Some(format!("{:02X}", attr.id)),
                        name: attr.display_name.clone(),
                        cells: vec![
                            DataCell::new(attr.value.to_string(), num_css),
                            DataCell::new(attr.worst.to_string(), num_css),
                            DataCell::new(attr.threshold.to_string(), CellCss::Thresh),
                            DataCell::new(attr.raw_string.clone(), raw_css).long(),
                        ],
                    }
                })
                .collect();
            (cols, rows)
        }
        DiskProtocol::Nvme => {
            let cols = vec![
                col_spec("ID", COL_W_ID, true, false),
                col_spec("Attribute Name", COL_W_NAME, false, true),
                col_spec("Raw Values", COL_W_VAL, true, true),
            ];
            let rows = report
                .nvme_entries
                .iter()
                .enumerate()
                .map(|(idx, e)| {
                    let raw = e.raw.to_string();
                    let raw_css = if raw == "0" {
                        CellCss::RawZero
                    } else {
                        CellCss::RawData
                    };
                    DataRow {
                        status: e.status,
                        id: Some(format!("{:02X}", idx + 1)),
                        name: e.display_name.clone(),
                        cells: vec![DataCell::new(raw, raw_css).long()],
                    }
                })
                .collect();
            (cols, rows)
        }
        DiskProtocol::Scsi => {
            let cols = vec![
                col_spec("État", COL_W_ID, true, false),
                col_spec("Op", COL_W_NAME, false, true),
                col_spec("Corrigées", COL_W_VAL, true, false),
                col_spec("Invocations", COL_W_VAL, true, false),
                col_spec("Go traités", COL_W_VAL, true, false),
                col_spec("Erreurs non corr.", COL_W_VAL, true, true),
            ];
            let rows = report
                .scsi_errors
                .iter()
                .map(|e| {
                    let good = e.total_uncorrected_errors == 0;
                    let num_css = if good { CellCss::Good } else { CellCss::None };
                    let last_css = if e.total_uncorrected_errors == 0 {
                        CellCss::RawZero
                    } else {
                        CellCss::RawData
                    };
                    DataRow {
                        status: e.status,
                        id: None,
                        name: capitalize(&e.operation),
                        cells: vec![
                            DataCell::new(e.total_errors_corrected.to_string(), num_css),
                            DataCell::new(e.correction_algorithm_invocations.to_string(), num_css),
                            DataCell::new(e.gigabytes_processed.clone(), CellCss::Thresh),
                            DataCell::new(e.total_uncorrected_errors.to_string(), last_css).long(),
                        ],
                    }
                })
                .collect();
            (cols, rows)
        }
        DiskProtocol::Unknown => (Vec::new(), Vec::new()),
    }
}

fn build_table(cols: &[ColSpec], rows: &[DataRow]) -> Grid {
    let grid = Grid::new();
    grid.set_column_spacing(0);
    grid.set_row_spacing(0);

    for (i, spec) in cols.iter().enumerate() {
        let l = Label::new(Some(&spec.header));
        l.add_css_class("secmet-smart-table-header");
        l.add_css_class("secmet-smart-table-head-cell");
        l.set_width_request(spec.min_width);
        l.set_hexpand(spec.expand);
        l.set_xalign(if spec.center { 0.5 } else { 0.0 });
        grid.attach(&l, i as i32, 0, 1, 1);
    }

    for (idx, row) in rows.iter().enumerate() {
        add_data_row(&grid, cols, (idx + 1) as i32, idx % 2 == 1, row);
    }

    grid
}

fn status_dot_markup(status: AttrStatus) -> String {
    format!(
        "<span foreground='{}' size='5120'>●</span>",
        attr_status_color_hex(status)
    )
}

/// Ligne : ID (ou point d'état) | (dot +) nom | valeurs numériques.
fn add_data_row(grid: &Grid, cols: &[ColSpec], row: i32, alt: bool, data: &DataRow) {
    // Toutes les cellules de la ligne, pour le survol commun.
    let row_cells: Rc<RefCell<Vec<Widget>>> = Rc::new(RefCell::new(Vec::new()));
    let mut col = 0i32;

    match &data.id {
        Some(id) => {
            let l = Label::new(Some(id));
            track_row_cell(&row_cells, &l);
            l.add_css_class("secmet-smart-table-row");
            if alt {
                l.add_css_class("secmet-smart-table-row-alt");
            }
            l.add_css_class("secmet-smart-table-cell");
            l.add_css_class("secmet-smart-td-id");
            l.set_xalign(0.5);
            grid.attach(&l, col, row, 1, 1);
            col += 1;
        }
        None => {
            let dot = Label::new(None);
            dot.set_markup(&status_dot_markup(data.status));
            track_row_cell(&row_cells, &dot);
            dot.add_css_class("secmet-smart-table-row");
            if alt {
                dot.add_css_class("secmet-smart-table-row-alt");
            }
            dot.add_css_class("secmet-smart-dot");
            dot.set_xalign(0.5);
            grid.attach(&dot, col, row, 1, 1);
            col += 1;
        }
    }

    // Colonne nom : la boîte remplit la cellule (le fond zebré couvre toute la
    // largeur, y compris la part absorbée par l'élargissement).
    let name_box = GtkBox::new(Orientation::Horizontal, 5);
    track_row_cell(&row_cells, &name_box);
    name_box.add_css_class("secmet-smart-table-row");
    if alt {
        name_box.add_css_class("secmet-smart-table-row-alt");
    }
    name_box.set_hexpand(cols.get(1).is_none_or(|c| c.expand));
    if data.id.is_some() {
        let dot = Label::new(None);
        dot.set_markup(&status_dot_markup(data.status));
        dot.add_css_class("secmet-smart-dot");
        name_box.append(&dot);
    }
    let name_l = Label::new(Some(&data.name));
    name_l.add_css_class("secmet-smart-table-cell");
    name_l.add_css_class("secmet-smart-td-name");
    name_l.set_xalign(0.0);
    name_l.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    name_l.set_hexpand(true);
    name_box.append(&name_l);
    grid.attach(&name_box, col, row, 1, 1);
    col += 1;

    for cell in &data.cells {
        let l = Label::new(Some(&cell.text));
        track_row_cell(&row_cells, &l);
        l.add_css_class("secmet-smart-table-row");
        if alt {
            l.add_css_class("secmet-smart-table-row-alt");
        }
        l.add_css_class("secmet-smart-table-cell");
        if let Some(c) = cell.css.class() {
            l.add_css_class(c);
        }
        l.set_xalign(0.5);
        l.set_selectable(true);
        if cell.long {
            l.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            l.set_max_width_chars(40);
        }
        l.set_hexpand(cols.get(col as usize).is_some_and(|c| c.expand));
        grid.attach(&l, col, row, 1, 1);
        col += 1;
    }
}

/// Enregistre la cellule dans sa ligne et branchant le survol partagé.
fn track_row_cell(row_cells: &Rc<RefCell<Vec<Widget>>>, w: &impl IsA<Widget>) {
    row_cells.borrow_mut().push(w.clone().upcast::<Widget>());

    let for_enter = row_cells.clone();
    let enter = EventControllerMotion::new();
    enter.connect_enter(move |_, _, _| set_row_hover(&for_enter, true));

    let for_leave = row_cells.clone();
    let leave = EventControllerMotion::new();
    leave.connect_leave(move |_| set_row_hover(&for_leave, false));

    w.add_controller(enter);
    w.add_controller(leave);
}

fn set_row_hover(row_cells: &Rc<RefCell<Vec<Widget>>>, on: bool) {
    for w in row_cells.borrow().iter() {
        if on {
            w.add_css_class("secmet-smart-row-hover");
        } else {
            w.remove_css_class("secmet-smart-row-hover");
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
