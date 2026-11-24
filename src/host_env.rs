//! Environnement rendu aux programmes du bureau lancés depuis SecMet.
//!
//! Le lanceur de l'AppImage (crochet linuxdeploy-plugin-gtk) prépare
//! l'environnement pour la pile GTK embarquée : thème forcé, X11 forcé,
//! modules et données pris dans le montage de l'AppImage. Un lecteur PDF,
//! un gestionnaire de fichiers ou un navigateur qui en hérite charge ces
//! réglages avec ses propres bibliothèques : icônes cassées, mauvais thème,
//! voire refus de démarrer. Ces programmes reçoivent donc l'environnement
//! du bureau, débarrassé de ce que l'AppImage a ajouté.

use gtk4::gio::prelude::AppLaunchContextExt;
use std::process::Command;

/// Variables posées uniquement pour la pile GTK de l'AppImage.
const APPIMAGE_ONLY: &[&str] = &[
    "GTK_DATA_PREFIX",
    "GTK_THEME",
    "GDK_BACKEND",
    "GSETTINGS_SCHEMA_DIR",
    "GI_TYPELIB_PATH",
    "GTK_EXE_PREFIX",
    "GTK_PATH",
    "GTK_IM_MODULE_FILE",
    "GDK_PIXBUF_MODULE_FILE",
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
];

/// Listes de chemins où l'AppImage a pu insérer des entrées de son montage.
const PATH_LISTS: &[&str] = &["XDG_DATA_DIRS", "PATH", "LD_LIBRARY_PATH"];

/// Ce qu'il faut changer : `(variable, None)` à retirer, `(variable,
/// Some(valeur))` à remplacer. Vide hors AppImage.
pub fn host_overrides() -> Vec<(String, Option<String>)> {
    overrides_from(|k| std::env::var(k).ok())
}

fn overrides_from(get: impl Fn(&str) -> Option<String>) -> Vec<(String, Option<String>)> {
    let Some(appdir) = get("APPDIR").filter(|d| !d.is_empty()) else {
        return Vec::new();
    };
    let appdir = appdir.trim_end_matches('/').to_string();
    let mut out: Vec<(String, Option<String>)> = APPIMAGE_ONLY
        .iter()
        .filter(|k| get(k).is_some())
        .map(|k| (k.to_string(), None))
        .collect();
    for key in PATH_LISTS {
        let Some(value) = get(key) else { continue };
        let mut kept: Vec<&str> = Vec::new();
        for entry in value.split(':') {
            let inside = entry == appdir || entry.starts_with(&format!("{appdir}/"));
            if !entry.is_empty() && !inside && !kept.contains(&entry) {
                kept.push(entry);
            }
        }
        let cleaned = kept.join(":");
        if cleaned != value {
            out.push((key.to_string(), (!cleaned.is_empty()).then_some(cleaned)));
        }
    }
    out
}

/// `Command` pour un programme du bureau, environnement de l'AppImage retiré.
pub fn host_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    for (key, value) in host_overrides() {
        match value {
            Some(v) => cmd.env(key, v),
            None => cmd.env_remove(key),
        };
    }
    cmd
}

/// Contexte de lancement GIO avec le même environnement nettoyé.
pub fn host_launch_context() -> gtk4::gio::AppLaunchContext {
    let ctx = gtk4::gio::AppLaunchContext::new();
    for (key, value) in host_overrides() {
        match value {
            Some(v) => ctx.setenv(key, v),
            None => ctx.unsetenv(key),
        }
    }
    ctx
}

/// Ouvre `uri` avec l'application par défaut du bureau.
pub fn launch_default_for_uri(uri: &str) -> Result<(), gtk4::glib::Error> {
    gtk4::gio::AppInfo::launch_default_for_uri(uri, Some(&host_launch_context()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn nothing_changes_outside_an_appimage() {
        let get = env(&[("GTK_THEME", "Adwaita:dark"), ("PATH", "/usr/bin")]);
        assert!(overrides_from(get).is_empty());
    }

    #[test]
    fn appimage_variables_are_removed_and_lists_cleaned() {
        let get = env(&[
            ("APPDIR", "/tmp/.mount_SecMet1/"),
            ("GTK_THEME", "Adwaita:dark"),
            ("GDK_BACKEND", "x11"),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_SecMet1/usr/share:/usr/share:/usr/local/share:/usr/share",
            ),
            ("PATH", "/usr/bin:/bin"),
            ("LD_LIBRARY_PATH", "/tmp/.mount_SecMet1/usr/lib"),
        ]);
        let out: HashMap<_, _> = overrides_from(get).into_iter().collect();
        assert_eq!(out.get("GTK_THEME"), Some(&None));
        assert_eq!(out.get("GDK_BACKEND"), Some(&None));
        assert_eq!(out.get("APPDIR"), Some(&None));
        assert_eq!(
            out.get("XDG_DATA_DIRS"),
            Some(&Some("/usr/share:/usr/local/share".to_string()))
        );
        assert_eq!(out.get("LD_LIBRARY_PATH"), Some(&None));
        // PATH sans entrée de l'AppImage : inchangé.
        assert!(!out.contains_key("PATH"));
        // Variable absente : rien à retirer.
        assert!(!out.contains_key("GTK_PATH"));
    }

    #[test]
    fn a_sibling_directory_with_the_same_prefix_is_kept() {
        let get = env(&[
            ("APPDIR", "/tmp/.mount_SecMet1"),
            (
                "PATH",
                "/tmp/.mount_SecMet1/usr/bin:/tmp/.mount_SecMet12/bin",
            ),
        ]);
        let out: HashMap<_, _> = overrides_from(get).into_iter().collect();
        assert_eq!(
            out.get("PATH"),
            Some(&Some("/tmp/.mount_SecMet12/bin".to_string()))
        );
    }
}
