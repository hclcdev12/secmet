//! Interface : on ajoute les lots (dossiers ou partitions d'export des
//! clés), on lit le bilan et les avertissements, on choisit la langue et
//! le dossier de destination, puis on regroupe. Lecture des lots et
//! écriture en arrière-plan (empreintes de fichiers sur clé USB).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use eframe::egui;
use secmet_attest::consolidation::{self, Discovered, Grouping, SourceLot, Written};
use secmet_attest::lang::Lang;

use crate::job;

const TITLE: &str = "SecMet Regroupement";
const GREEN: egui::Color32 = egui::Color32::from_rgb(46, 125, 50);
const RED: egui::Color32 = egui::Color32::from_rgb(198, 40, 40);
const AMBER: egui::Color32 = egui::Color32::from_rgb(191, 120, 0);

/// Lance l'interface.
pub fn run() -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([780.0, 600.0])
            .with_min_inner_size([600.0, 460.0]),
        #[cfg(windows)]
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        TITLE,
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_zoom_factor(1.1);
            Ok(Box::new(App::new()))
        }),
    )
    .map_err(|e| format!("interface graphique : {e}"))
}

/// Dossier proposé pour les regroupements : `Documents/SecMet-Regroupements`.
fn default_destination() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SecMet-Regroupements")
}

struct App {
    lots: Vec<SourceLot>,
    /// Dossiers refusés à l'ajout, avec la raison.
    refused: Vec<String>,
    /// Lecture de dossiers en cours.
    loading: Option<Receiver<Discovered>>,
    /// Regroupement calculé sur les lots actuels (ou l'erreur).
    grouping: Option<Result<Grouping, String>>,
    lang: Lang,
    destination: PathBuf,
    writing: Option<Receiver<Result<Written, String>>>,
    result: Option<Result<Written, String>>,
}

impl App {
    fn new() -> Self {
        Self {
            lots: Vec::new(),
            refused: Vec::new(),
            loading: None,
            grouping: None,
            lang: Lang::Fr,
            destination: default_destination(),
            writing: None,
            result: None,
        }
    }

    /// Lit des dossiers en arrière-plan.
    fn add(&mut self, dirs: Vec<PathBuf>) {
        if dirs.is_empty() || self.loading.is_some() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut all = Discovered::default();
            for d in dirs {
                let found = consolidation::discover(&d);
                all.lots.extend(found.lots);
                all.refused.extend(found.refused);
            }
            let _ = tx.send(all);
        });
        self.loading = Some(rx);
        self.result = None;
    }

    fn regroup(&mut self) {
        self.grouping = if self.lots.is_empty() {
            None
        } else {
            Some(consolidation::build(self.lots.clone()))
        };
    }

    fn poll(&mut self) -> bool {
        if let Some(rx) = &self.loading {
            if let Ok(found) = rx.try_recv() {
                self.loading = None;
                for lot in found.lots {
                    // Même lot ajouté deux fois depuis le même dossier : ignoré.
                    if !self
                        .lots
                        .iter()
                        .any(|l| l.manifest.signature == lot.manifest.signature && l.dir == lot.dir)
                    {
                        self.lots.push(lot);
                    }
                }
                self.refused.extend(found.refused);
                self.regroup();
            }
        }
        if let Some(rx) = &self.writing {
            if let Ok(r) = rx.try_recv() {
                self.writing = None;
                self.result = Some(r);
            }
        }
        self.loading.is_some() || self.writing.is_some()
    }

    fn start_write(&mut self) {
        let Some(Ok(grouping)) = &self.grouping else {
            return;
        };
        let (grouping, parent, lang) = (grouping.clone(), self.destination.clone(), self.lang);
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(job::write(&grouping, &parent, lang));
        });
        self.writing = Some(rx);
        self.result = None;
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        if self.poll() {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| {
                    if p.is_dir() {
                        p
                    } else {
                        p.parent().map(Path::to_path_buf).unwrap_or(p)
                    }
                })
                .collect()
        });
        self.add(dropped);

        egui::Panel::bottom("foot").show(root, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.weak(format!("{TITLE} {}", env!("CARGO_PKG_VERSION")));
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                self.ui_sources(ui);
                ui.add_space(10.0);
                self.ui_summary(ui);
                ui.add_space(10.0);
                self.ui_output(ui);
            });
        });
    }
}

impl App {
    fn ui_sources(&mut self, ui: &mut egui::Ui) {
        ui.heading("1. Lots à regrouper");
        ui.label(
            "Ajoutez le dossier de chaque lot clôturé (SECMET-EXP/<LOT>), ou directement \
             la partition SECMET-EXP d'une clé : tous ses lots sont pris. Vous pouvez aussi \
             déposer les dossiers dans cette fenêtre.",
        );
        ui.horizontal(|ui| {
            let busy = self.loading.is_some() || self.writing.is_some();
            if ui
                .add_enabled(!busy, egui::Button::new("Ajouter des dossiers…"))
                .clicked()
            {
                if let Some(dirs) = rfd::FileDialog::new()
                    .set_title("Lots clôturés ou partition SECMET-EXP")
                    .pick_folders()
                {
                    self.add(dirs);
                }
            }
            if ui
                .add_enabled(
                    !busy && (!self.lots.is_empty() || !self.refused.is_empty()),
                    egui::Button::new("Tout retirer"),
                )
                .clicked()
            {
                self.lots.clear();
                self.refused.clear();
                self.grouping = None;
                self.result = None;
            }
            if self.loading.is_some() {
                ui.spinner();
                ui.label("Lecture et vérification des lots…");
            }
        });

        if !self.lots.is_empty() {
            ui.add_space(6.0);
            let mut remove = None;
            egui::Grid::new("lots")
                .striped(true)
                .num_columns(6)
                .spacing([14.0, 4.0])
                .show(ui, |ui| {
                    for h in ["Lot", "Clé", "Opérateur", "Référence client", "Disques", ""] {
                        ui.label(egui::RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (i, l) in self.lots.iter().enumerate() {
                        ui.label(l.lot_id())
                            .on_hover_text(l.dir.display().to_string());
                        ui.monospace(l.key_id());
                        ui.label(&l.manifest.payload.operator);
                        ui.label(if l.reference().is_empty() {
                            "—"
                        } else {
                            l.reference()
                        });
                        ui.label(l.certificates.len().to_string());
                        if ui
                            .add_enabled(self.writing.is_none(), egui::Button::new("Retirer"))
                            .clicked()
                        {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
            if let Some(i) = remove {
                self.lots.remove(i);
                self.result = None;
                self.regroup();
            }
        }

        if !self.refused.is_empty() {
            ui.add_space(6.0);
            ui.colored_label(RED, egui::RichText::new("✖ Dossiers refusés").strong());
            for r in &self.refused {
                ui.colored_label(RED, r);
            }
            if ui.small_button("Effacer ces messages").clicked() {
                self.refused.clear();
            }
        }
    }

    fn ui_summary(&mut self, ui: &mut egui::Ui) {
        ui.heading("2. Bilan");
        match &self.grouping {
            None => {
                ui.weak("Aucun lot ajouté.");
            }
            Some(Err(e)) => {
                ui.colored_label(RED, format!("✖ Regroupement impossible : {e}"));
            }
            Some(Ok(g)) => {
                let t = g.totals();
                let lots = g.lots.len();
                let keys = g.key_ids().len();
                ui.label(format!(
                    "{} · {} · {}",
                    plural(lots, "lot", "lots"),
                    plural(keys, "clé de signature", "clés de signature"),
                    plural(t.disks, "disque", "disques"),
                ));
                ui.horizontal(|ui| {
                    ui.colored_label(GREEN, format!("Réussis : {}", t.success));
                    ui.colored_label(
                        if t.failed > 0 {
                            RED
                        } else {
                            ui.visuals().text_color()
                        },
                        format!("Échecs : {}", t.failed),
                    );
                    ui.label(format!("Repassés (non comptés) : {}", t.redone));
                    if t.destroy > 0 {
                        ui.colored_label(RED, format!("À détruire : {}", t.destroy));
                    }
                });
                let reference = g.reference();
                if !reference.is_empty() {
                    ui.label(format!("Référence client : {reference}"));
                }
                for w in &g.warnings {
                    ui.colored_label(AMBER, format!("⚠ {w}"));
                }
            }
        }
    }

    fn ui_output(&mut self, ui: &mut egui::Ui) {
        ui.heading("3. Regrouper");
        ui.horizontal(|ui| {
            ui.label("Langue du PDF :");
            for (lang, name) in [
                (Lang::Fr, "Français"),
                (Lang::En, "English"),
                (Lang::De, "Deutsch"),
            ] {
                ui.radio_value(&mut self.lang, lang, name);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Destination :");
            ui.monospace(self.destination.display().to_string());
            if ui
                .add_enabled(self.writing.is_none(), egui::Button::new("Changer…"))
                .clicked()
            {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Dossier de destination du regroupement")
                    .set_directory(self.destination.parent().unwrap_or(&self.destination))
                    .pick_folder()
                {
                    self.destination = dir;
                    self.result = None;
                }
            }
        });
        ui.weak(
            "Un dossier R-<date>-<code> y est créé : le PDF de synthèse, un index JSON et les \
             fichiers d'origine des lots (sources/), recopiés à l'identique. La synthèse n'est \
             pas signée : chaque lot reste prouvé par son manifeste signé.",
        );
        ui.add_space(6.0);
        let ready = matches!(self.grouping, Some(Ok(_)))
            && self.loading.is_none()
            && self.writing.is_none();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    ready,
                    egui::Button::new(egui::RichText::new("Regrouper").strong()),
                )
                .clicked()
            {
                self.start_write();
            }
            if self.writing.is_some() {
                ui.spinner();
                ui.label("Copie des fichiers et rédaction du PDF…");
            }
        });
        match &self.result {
            Some(Ok(w)) => {
                ui.add_space(6.0);
                ui.colored_label(
                    GREEN,
                    egui::RichText::new(format!("✔ Regroupement créé : {}", w.dir.display()))
                        .strong(),
                );
                ui.horizontal(|ui| {
                    if ui.button("Ouvrir le PDF").clicked() {
                        job::open(&w.pdf);
                    }
                    if ui.button("Ouvrir le dossier").clicked() {
                        job::open(&w.dir);
                    }
                });
            }
            Some(Err(e)) => {
                ui.add_space(6.0);
                ui.colored_label(RED, format!("✖ Échec du regroupement : {e}"));
            }
            None => {}
        }
    }
}

/// « 1 lot », « 3 lots ».
fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}
