//! Écriture d'un regroupement : identifiant daté, PDF dans la langue
//! choisie, ouverture des résultats.

use std::path::Path;

use secmet_attest::consolidation::{self, Grouping, Written};
use secmet_attest::lang::Lang;
use secmet_attest::pdf;

/// Logiciel inscrit dans l'index.
pub fn software() -> String {
    format!("secmet-consolidate {}", env!("CARGO_PKG_VERSION"))
}

/// Heure locale, `AAAA-MM-JJ HH:MM:SS`.
pub fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Écrit le regroupement dans `<parent>/R-…/`.
pub fn write(grouping: &Grouping, parent: &Path, lang: Lang) -> Result<Written, String> {
    std::fs::create_dir_all(parent).map_err(|e| format!("{} : {e}", parent.display()))?;
    consolidation::write(grouping, parent, &now(), &software(), |index| {
        pdf::grouped::render_grouping(grouping, index, pdf::LOGO_JPEG, lang)
    })
}

/// Ouvre un fichier ou un dossier avec le programme du bureau.
pub fn open(path: &Path) {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("explorer");
        c.arg(path);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        // Lancé depuis une AppImage : le programme du bureau ne doit pas
        // hériter de ses bibliothèques.
        if std::env::var_os("APPIMAGE").is_some() {
            for var in [
                "LD_LIBRARY_PATH",
                "LD_PRELOAD",
                "GTK_PATH",
                "GDK_PIXBUF_MODULE_FILE",
            ] {
                c.env_remove(var);
            }
        }
        c
    };
    let _ = cmd.spawn();
}
