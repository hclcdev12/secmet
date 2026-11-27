//! « À propos » de SecMet Regroupement : titulaire du droit d'auteur, licence
//! GPL-3 sans garantie, lien vers les sources, avis de licences tierces ;
//! et l'option `--licenses`, qui écrit ces avis sur la sortie standard.
//!
//! Avis tiers : sous Windows, il est **intégré au binaire**
//! (`packaging/licenses/consolidate-windows.txt`), si bien qu'un `.exe`
//! copié seul porte ses avis. Sous Linux, il est lu là où l'AppImage
//! l'installe (`usr/share/doc/secmet-consolidate/THIRD_PARTY_LICENSES.txt`,
//! à côté de `usr/bin/`).
//!
//! Titulaire, licence et adresse du dépôt : [`secmet_attest::legal`].

use std::borrow::Cow;
use std::ops::Range;

use eframe::egui;
use secmet_attest::legal;

/// Version de l'outil.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Texte intégral de la licence (en-tête SecMet puis GPL-3, en anglais).
pub const LICENSE_TEXT: &str = include_str!("../../../LICENSE");

/// Avis de licences tierces de l'exécutable Windows (crates, polices egui,
/// runtime MinGW), généré par `make licenses`.
#[cfg(windows)]
const THIRD_PARTY_EMBEDDED: &str =
    include_str!("../../../packaging/licenses/consolidate-windows.txt");

/// Nom du fichier d'avis installé (et proposé à l'enregistrement).
pub const THIRD_PARTY_FILE: &str = "THIRD_PARTY_LICENSES.txt";

/// Avis de licences tierces : intégré (Windows) ou installé (Linux).
pub fn third_party_text() -> Result<Cow<'static, str>, String> {
    #[cfg(windows)]
    {
        Ok(Cow::Borrowed(THIRD_PARTY_EMBEDDED))
    }
    #[cfg(not(windows))]
    {
        let exe = std::env::current_exe().map_err(|e| format!("exécutable introuvable : {e}"))?;
        let path = exe
            .parent()
            .and_then(|bin| bin.parent())
            .map(|usr| {
                usr.join("share/doc/secmet-consolidate")
                    .join(THIRD_PARTY_FILE)
            })
            .ok_or_else(|| format!("chemin inattendu : {}", exe.display()))?;
        std::fs::read_to_string(&path).map(Cow::Owned).map_err(|e| {
            format!(
                "avis des licences tierces illisible ({} : {e}) ; il est aussi dans les sources, packaging/licenses/consolidate-linux.txt",
                path.display()
            )
        })
    }
}

/// `--licenses` : mentions légales puis avis tiers sur la sortie standard.
pub fn print_licenses() -> std::process::ExitCode {
    println!("SecMet Regroupement {VERSION}");
    println!("{}", legal::copyright_line());
    println!("{}", legal::LICENSE_NOTICE_EN);
    println!("Source: {}", legal::source_url_for(VERSION));
    println!();
    match third_party_text() {
        Ok(text) => {
            print!("{text}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Texte découpé en lignes pour `ScrollArea::show_rows` : un seul `Label`
/// de plusieurs centaines de Ko serait remis en page à chaque image.
struct Lines {
    text: Cow<'static, str>,
    lines: Vec<Range<usize>>,
}

impl Lines {
    fn new(text: Cow<'static, str>) -> Self {
        let mut lines = Vec::new();
        let mut start = 0;
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                lines.push(start..i);
                start = i + 1;
            }
        }
        if start < text.len() {
            lines.push(start..text.len());
        }
        Lines { text, lines }
    }

    fn show(&self, ui: &mut egui::Ui, id: &str, max_height: f32) {
        let style = egui::TextStyle::Monospace;
        let row_height = ui.text_style_height(&style);
        egui::ScrollArea::both()
            .id_salt(id)
            .max_height(max_height)
            .auto_shrink([false, true])
            .show_rows(ui, row_height, self.lines.len(), |ui, rows| {
                for r in rows {
                    let line = self.lines.get(r).and_then(|l| self.text.get(l.clone()));
                    ui.label(egui::RichText::new(line.unwrap_or("")).text_style(style.clone()));
                }
            });
    }
}

/// État de la fenêtre « À propos » ouverte.
pub struct About {
    license: Lines,
    third_party: Result<Lines, String>,
    saved: Option<Result<String, String>>,
}

impl About {
    /// Ouvre la fenêtre (lit l'avis tiers une fois).
    pub fn new() -> Self {
        About {
            license: Lines::new(Cow::Borrowed(LICENSE_TEXT)),
            third_party: third_party_text().map(Lines::new),
            saved: None,
        }
    }

    /// Affiche la fenêtre modale ; `false` quand elle est fermée.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut close = false;
        // La fenêtre de l'outil est petite (600 × 460 au minimum) : tout le
        // corps défile, et chaque texte de licence a une hauteur bornée par
        // l'écran, pour que le haut de la fenêtre ne sorte jamais du cadre
        // quand on déplie les sections.
        let screen_h = ctx.content_rect().height();
        let body_max = (screen_h - 140.0).max(160.0);
        let text_max = (screen_h * 0.4).clamp(100.0, 260.0);
        let width = (ctx.content_rect().width() - 80.0).clamp(320.0, 560.0);
        let modal = egui::Modal::new(egui::Id::unique("secmet-consolidate-about")).show(ctx, |ui| {
            ui.set_width(width);
            ui.heading("À propos de SecMet Regroupement");
            egui::ScrollArea::vertical()
                .id_salt("about-body")
                .max_height(body_max)
                .auto_shrink([false, true])
                .show(ui, |ui| {
            ui.label(format!("Version {VERSION}"));
            ui.label("Regroupe les lots clôturés de plusieurs clés SecMet en un seul livrable : synthèse PDF, index et fichiers d'origine, sans rien ré-attester.");
            ui.add_space(6.0);
            ui.label(legal::copyright_short());
            ui.label(format!(
                "Logiciel libre sous licence {} ({}). Ce programme est distribué dans l'espoir qu'il sera utile, mais SANS AUCUNE GARANTIE, sans même la garantie implicite de QUALITÉ MARCHANDE ou d'ADÉQUATION À UN USAGE PARTICULIER. Seule la version anglaise de la licence fait foi.",
                legal::LICENSE_NAME,
                legal::LICENSE_SPDX
            ));
            ui.hyperlink_to("Code source de cette version", legal::source_url_for(VERSION));
            ui.add_space(6.0);
            egui::CollapsingHeader::new("Licence (texte intégral, en anglais)")
                .id_salt("about-license")
                .show(ui, |ui| self.license.show(ui, "license-text", text_max));
            egui::CollapsingHeader::new("Licences tierces")
                .id_salt("about-third-party")
                .show(ui, |ui| match &self.third_party {
                    Ok(lines) => {
                        ui.label("Bibliothèques Rust, polices et runtime intégrés à l'outil, sous leurs propres licences :");
                        lines.show(ui, "third-party-text", text_max);
                        if ui.button("Enregistrer…").clicked() {
                            self.saved = save(&lines.text);
                        }
                        match &self.saved {
                            Some(Ok(p)) => {
                                ui.label(format!("Enregistré : {p}"));
                            }
                            Some(Err(e)) => {
                                ui.colored_label(ui.visuals().error_fg_color, format!("✖ {e}"));
                            }
                            None => {}
                        }
                    }
                    Err(e) => {
                        ui.colored_label(ui.visuals().error_fg_color, format!("✖ {e}"));
                    }
                });
                });
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Fermer").clicked() {
                    close = true;
                }
            });
        });
        !(close || modal.should_close())
    }
}

/// « Enregistrer… » : choix du fichier puis écriture ; `None` si annulé.
fn save(text: &str) -> Option<Result<String, String>> {
    let path = rfd::FileDialog::new()
        .set_title("Enregistrer les licences tierces")
        .set_file_name(THIRD_PARTY_FILE)
        .add_filter("Texte", &["txt"])
        .save_file()?;
    Some(
        std::fs::write(&path, text)
            .map(|()| path.display().to_string())
            .map_err(|e| format!("{} : {e}", path.display())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn license_is_embedded() {
        assert!(LICENSE_TEXT.contains("GNU GENERAL PUBLIC LICENSE"));
        assert!(LICENSE_TEXT.contains(legal::COPYRIGHT_HOLDER));
    }

    #[test]
    fn lines_split_without_losing_text() {
        let l = Lines::new(Cow::Borrowed("a\nbc\n\nd"));
        let got: Vec<&str> = l.lines.iter().map(|r| &l.text[r.clone()]).collect();
        assert_eq!(got, ["a", "bc", "", "d"]);
    }

    /// Les avis générés existent et couvrent les polices d'egui (le
    /// `.exe` intègre la version Windows).
    #[test]
    fn generated_notices_cover_fonts() {
        for text in [
            include_str!("../../../packaging/licenses/consolidate-windows.txt"),
            include_str!("../../../packaging/licenses/consolidate-linux.txt"),
        ] {
            assert!(text.contains("SIL OPEN FONT LICENSE"));
            assert!(text.contains("Bitstream Vera"));
            assert!(text.contains("UBUNTU FONT LICENCE"));
            assert!(text.contains("eframe"));
        }
    }
}
