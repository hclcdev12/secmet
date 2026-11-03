//! Migration automatique de ~/.shred_graphique_logs vers ~/.secmet_logs.

use crate::app_identity::{LEGACY_LOGS_DIR, LOGS_DIR, PKG_NAME};
use std::fs;
use std::path::Path;

const REPLACEMENTS: &[(&str, &str)] = &[
    ("shred-graphique", PKG_NAME),
    ("shred_graphique", PKG_NAME),
    (".shred_graphique_logs", LOGS_DIR),
    ("/usr/lib/shred-graphique/", "/usr/lib/secmet/"),
];

pub fn run_user_data_migration() {
    // Dossier de données dédié (clé live) : rien à migrer depuis le compte.
    if crate::paths::dedicated_data_dir() {
        return;
    }
    let Some(home) = dirs::home_dir() else {
        return;
    };

    let legacy = home.join(LEGACY_LOGS_DIR);
    let target = home.join(LOGS_DIR);

    if !legacy.exists() {
        return;
    }

    let result = if !target.exists() {
        migrate_rename(&legacy, &target)
    } else {
        migrate_merge(&legacy, &target)
    };

    match result {
        Ok(()) => eprintln!("[{PKG_NAME}] Migration des logs : OK"),
        Err(e) => eprintln!("[{PKG_NAME}] Migration des logs : {e}"),
    }
}

fn migrate_rename(legacy: &Path, target: &Path) -> Result<(), String> {
    fs::rename(legacy, target).map_err(|e| format!("renommage {legacy:?} → {target:?}: {e}"))?;
    patch_json_files(target)?;
    Ok(())
}

fn migrate_merge(legacy: &Path, target: &Path) -> Result<(), String> {
    merge_dir_contents(legacy, target)?;
    let backup = legacy.with_extension("bak");
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }
    fs::rename(legacy, &backup).map_err(|e| format!("sauvegarde {legacy:?} → {backup:?}: {e}"))?;
    patch_json_files(target)?;
    Ok(())
}

fn merge_dir_contents(from: &Path, into: &Path) -> Result<(), String> {
    for entry in fs::read_dir(from).map_err(|e| format!("lecture {from:?}: {e}"))? {
        let entry = entry.map_err(|e| format!("entrée {from:?}: {e}"))?;
        let src = entry.path();
        let name = entry.file_name();
        let dest = into.join(&name);
        if src.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| format!("mkdir {dest:?}: {e}"))?;
            merge_dir_contents(&src, &dest)?;
        } else if !dest.exists() {
            fs::copy(&src, &dest).map_err(|e| format!("copie {src:?} → {dest:?}: {e}"))?;
        }
    }
    Ok(())
}

fn patch_json_files(root: &Path) -> Result<(), String> {
    patch_json_dir(root)
}

fn patch_json_dir(dir: &Path) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("lecture {dir:?}: {e}"))? {
        let entry = entry.map_err(|e| format!("entrée {dir:?}: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            patch_json_dir(&path)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            patch_json_file(&path)?;
        }
    }
    Ok(())
}

fn patch_json_file(path: &Path) -> Result<(), String> {
    let content = fs::read_to_string(path).map_err(|e| format!("lecture {path:?}: {e}"))?;
    let mut patched = content.clone();
    for (from, to) in REPLACEMENTS {
        patched = patched.replace(from, to);
    }
    if patched != content {
        fs::write(path, patched).map_err(|e| format!("écriture {path:?}: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("secmet-migrate-test-{nanos}"))
    }

    #[test]
    fn migrate_rename_patches_json() {
        let home = temp_home();
        let legacy = home.join(LEGACY_LOGS_DIR);
        fs::create_dir_all(&legacy).unwrap();
        let json_path = legacy.join("test.json");
        fs::write(
            &json_path,
            r#"{"tool_versions":{"shred_graphique":"shred-graphique 0.1.0"},"path":"~/.shred_graphique_logs/cert.pdf"}"#,
        )
        .unwrap();

        let target = home.join(LOGS_DIR);
        migrate_rename(&legacy, &target).unwrap();

        assert!(!legacy.exists());
        assert!(target.exists());
        let content = fs::read_to_string(target.join("test.json")).unwrap();
        assert!(content.contains(r#""secmet 0.1.0""#));
        assert!(content.contains(".secmet_logs"));
        assert!(!content.contains("shred-graphique"));

        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn migrate_merge_moves_without_overwrite() {
        let home = temp_home();
        let legacy = home.join(LEGACY_LOGS_DIR);
        let target = home.join(LOGS_DIR);
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(legacy.join("old.json"), r#"{"x":"shred-graphique"}"#).unwrap();
        fs::write(target.join("keep.json"), r#"{"y":1}"#).unwrap();

        migrate_merge(&legacy, &target).unwrap();

        assert!(target.join("old.json").exists());
        assert!(target.join("keep.json").exists());
        assert!(home.join(format!("{LEGACY_LOGS_DIR}.bak")).exists());
        let content = fs::read_to_string(target.join("old.json")).unwrap();
        assert!(content.contains("secmet"));

        let _ = fs::remove_dir_all(&home);
    }
}
