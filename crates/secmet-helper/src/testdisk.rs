//! Exécution de TestDisk en mode scripté (`analyze,search`) sur le disque
//! d'une session, lignes de sortie relayées à l'UI, journal récupéré.
//! TestDisk reste un outil externe : il n'est lancé qu'avec des arguments
//! fixes, sur un nœud ré-identifié, jamais avec des arguments de l'UI.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::{HelperError, Result};

/// Binaire embarqué (paquet .deb / AppImage), prioritaire.
pub const BUNDLE_BIN: &str = "/usr/lib/secmet/bin/testdisk";
/// Bibliothèques privées du binaire embarqué.
pub const BUNDLE_LIB: &str = "/usr/lib/secmet/lib";
const SYSTEM_CANDIDATES: &[&str] = &["/usr/bin/testdisk", "/usr/sbin/testdisk", "/sbin/testdisk"];
/// Racine des journaux temporaires : sous `/run/secmet` (root, 0711), jamais
/// dans un répertoire sticky partagé.
const LOG_ROOT: &str = "/run/secmet";
/// Taille maximale conservée pour chaque sortie texte (queue), afin que la
/// réponse tienne dans une trame IPC.
const MAX_TEXT_BYTES: usize = 256 << 10;

/// Les `max` derniers octets de `text`, sur une frontière de ligne.
fn tail_text(mut text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let cut = text.len() - max;
    let start = text[cut..].find('\n').map_or(cut, |i| cut + i + 1);
    text.drain(..start);
    text.insert_str(0, "[… tronqué …]\n");
    text
}

/// Résultat brut d'une exécution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TestdiskRun {
    /// Version annoncée (`testdisk /version`).
    pub version: String,
    /// Code de sortie (`None` : tué par signal).
    pub exit_code: Option<i32>,
    /// Sortie standard complète.
    pub stdout: String,
    /// Sortie d'erreur complète.
    pub stderr: String,
    /// Contenu de `testdisk.log`.
    pub log: String,
    /// L'opérateur a interrompu l'analyse.
    pub cancelled: bool,
}

/// Chemin du binaire TestDisk, embarqué d'abord.
pub fn resolve_binary() -> Option<PathBuf> {
    std::iter::once(BUNDLE_BIN)
        .chain(SYSTEM_CANDIDATES.iter().copied())
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

fn command(binary: &Path) -> Command {
    let mut cmd = Command::new(binary);
    if binary.starts_with("/usr/lib/secmet/bin") {
        let existing = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        let value = if existing.is_empty() {
            BUNDLE_LIB.to_string()
        } else {
            format!("{BUNDLE_LIB}:{existing}")
        };
        cmd.env("LD_LIBRARY_PATH", value);
    }
    cmd
}

/// Version de TestDisk (première ligne contenant « TestDisk »).
pub fn version(binary: &Path) -> String {
    command(binary)
        .arg("/version")
        .output()
        .ok()
        .map(|o| {
            let text = String::from_utf8_lossy(&o.stdout).into_owned()
                + &String::from_utf8_lossy(&o.stderr);
            parse_version(&text)
        })
        .unwrap_or_default()
}

/// Extrait `7.2` de `TestDisk 7.2, Data Recovery Utility, …`.
pub fn parse_version(text: &str) -> String {
    text.lines()
        .find_map(|l| {
            let rest = l.trim().strip_prefix("TestDisk ")?;
            Some(rest.split([',', ' ']).next()?.to_string())
        })
        .unwrap_or_default()
}

/// Lance l'analyse sur `device` ; chaque ligne de sortie est passée à
/// `on_line`. `slot` reçoit le processus enfant pour permettre un `kill`
/// depuis une autre connexion.
pub fn run(
    device: &Path,
    slot: &Arc<Mutex<Option<Child>>>,
    mut on_line: impl FnMut(&str),
) -> Result<TestdiskRun> {
    let binary = resolve_binary().ok_or_else(|| {
        HelperError::Refused("TestDisk est absent (paquet testdisk ou bundle SecMet)".into())
    })?;
    let version = version(&binary);
    let log_dir = PathBuf::from(LOG_ROOT).join(format!(
        "secmet-testdisk-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(LOG_ROOT).map_err(|e| HelperError::io(LOG_ROOT.to_string(), e))?;
    // `create_dir` (et non `_all`) : un répertoire préexistant est refusé.
    fs::create_dir(&log_dir).map_err(|e| HelperError::io(log_dir.display().to_string(), e))?;
    let log_path = log_dir.join("testdisk.log");

    let mut child = command(&binary)
        .arg("/log")
        .arg("/logname")
        .arg(&log_path)
        .arg("/cmd")
        .arg(device)
        .arg("analyze,search")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| HelperError::io(format!("lancement {}", binary.display()), e))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);

    let stderr_thread = stderr.map(|err| {
        std::thread::spawn(move || {
            let mut text = String::new();
            for line in BufReader::new(err)
                .lines()
                .map_while(std::result::Result::ok)
            {
                text.push_str(&line);
                text.push('\n');
            }
            text
        })
    });
    let mut out = String::new();
    if let Some(stdout) = stdout {
        for line in BufReader::new(stdout)
            .lines()
            .map_while(std::result::Result::ok)
        {
            on_line(&line);
            out.push_str(&line);
            out.push('\n');
        }
    }
    let status = {
        let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        match guard.take() {
            Some(mut child) => child
                .wait()
                .map_err(|e| HelperError::io("attente testdisk", e))?,
            None => return Err(HelperError::Internal("processus testdisk perdu".into())),
        }
    };
    let stderr_text = stderr_thread
        .and_then(|t| t.join().ok())
        .unwrap_or_default();
    let log = fs::read_to_string(&log_path).unwrap_or_default();
    let _ = fs::remove_dir_all(&log_dir);
    use std::os::unix::process::ExitStatusExt;
    Ok(TestdiskRun {
        version,
        exit_code: status.code(),
        stdout: tail_text(out, MAX_TEXT_BYTES),
        stderr: tail_text(stderr_text, 64 << 10),
        log: tail_text(log, MAX_TEXT_BYTES),
        cancelled: status.signal().is_some(),
    })
}

/// Tue le processus de `slot`, s'il existe.
pub fn kill(slot: &Arc<Mutex<Option<Child>>>) -> bool {
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(child) => child.kill().is_ok(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn version_is_parsed_from_the_banner() {
        assert_eq!(
            parse_version("TestDisk 7.2, Data Recovery Utility, February 2024\nChristophe GRENIER"),
            "7.2"
        );
        assert_eq!(parse_version("TestDisk 7.1-WIP\n"), "7.1-WIP");
        assert_eq!(parse_version("rien"), "");
    }

    #[test]
    fn kill_on_an_empty_slot_is_false() {
        let slot = Arc::new(Mutex::new(None));
        assert!(!kill(&slot));
    }
}
