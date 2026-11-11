//! SecMet Clé USB : écrit l'image publiée `secmet-key_<version>_x86_64.img.xz`
//! sur une clé USB, après vérification de la signature de la release avec
//! la clé de version de SecMet.
//!
//! Sans argument : interface graphique ; avec un chemin d'image en
//! argument (association de fichier, glisser sur l'icône), l'interface
//! part directement de cette image. Le processus privilégié qui écrit
//! réellement sur le disque est ce même exécutable relancé avec
//! `--worker <disque> <image>` (par `pkexec` sous Linux) ; il dialogue avec
//! l'interface par des lignes JSON sur sa sortie standard
//! ([`worker`]) et s'arrête quand son entrée standard se ferme.
//!
//! `--list-disks` : disques candidats ; `--verify <image>` : vérification
//! de l'image seule (signature de la release et empreinte), pour le
//! dépannage.
//!
//! Windows : exécutable sans console (sous-système « windows ») ; les modes
//! en ligne de commande s'attachent à la console du parent. L'interface
//! prend les droits d'administrateur au lancement (relance par UAC), le
//! processus `--worker` en hérite.

// Pas de fenêtre de console noire au lancement sous Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod disks;
mod gpt;
mod gui;
mod image;
mod platform;
mod release;
mod worker;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if matches!(
        args.get(1).map(String::as_str),
        Some("--list-disks" | "--verify" | "--version")
    ) {
        cli_console();
    }
    match args.get(1).map(String::as_str) {
        Some("--worker") => match (args.get(2), args.get(3)) {
            (Some(device), Some(image)) => worker::run(device, image),
            _ => {
                eprintln!("usage : secmet-key-writer --worker <disque> <image.img.xz>");
                ExitCode::from(2)
            }
        },
        Some("--list-disks") => match disks::list() {
            Ok(list) => {
                for d in &list {
                    println!("{}", d.describe());
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        },
        Some("--verify") => match args.get(2) {
            Some(path) => verify_cli(std::path::Path::new(path)),
            None => {
                eprintln!("usage : secmet-key-writer --verify <image.img.xz>");
                ExitCode::from(2)
            }
        },
        Some("--version") => {
            println!("secmet-key-writer {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(path) if !path.starts_with("--") => run_gui(Some(std::path::PathBuf::from(path))),
        Some(other) => {
            eprintln!("argument inconnu : {other}");
            ExitCode::from(2)
        }
        None => run_gui(None),
    }
}

/// Interface graphique ; sous Windows, d'abord les droits d'administrateur
/// (relance par UAC, ce processus se termine).
fn run_gui(image: Option<std::path::PathBuf>) -> ExitCode {
    #[cfg(windows)]
    if !platform::is_privileged() {
        let args: Vec<String> = match &image {
            Some(p) => vec![std::fs::canonicalize(p)
                .unwrap_or_else(|_| p.clone())
                .to_string_lossy()
                .into_owned()],
            None => Vec::new(),
        };
        return match platform::relaunch_elevated(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) if e.raw_os_error() == Some(1223) => {
                platform::message_box(
                    "SecMet Clé USB",
                    "L'écriture d'une clé USB demande les droits d'administrateur.\n\nRelancez l'outil et acceptez l'invite de Windows.",
                );
                ExitCode::FAILURE
            }
            Err(e) => {
                platform::message_box(
                    "SecMet Clé USB",
                    &format!("Impossible de relancer l'outil en administrateur : {e}"),
                );
                ExitCode::FAILURE
            }
        };
    }
    match gui::run(image) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Modes en ligne de commande : sous Windows, l'exécutable sans console
/// s'attache à celle du parent pour que `println!` soit visible.
fn cli_console() {
    #[cfg(windows)]
    platform::attach_console();
}

/// `--verify` : même contrôle que l'interface, résultat sur la sortie
/// standard.
fn verify_cli(path: &std::path::Path) -> ExitCode {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let latest = release::fetch_latest().ok();
    match image::verify_with_release(path, latest.as_ref(), &cancel, &mut |_, _| {}) {
        Ok((v, info)) => {
            println!(
                "VALIDE  {}  clé de version {}  SHA-256 {}  {} à écrire",
                path.display(),
                v.key_id,
                v.sha256,
                fmt_size(info.uncompressed_size)
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("INVALIDE  {} : {e}", path.display());
            ExitCode::FAILURE
        }
    }
}

/// Taille lisible, en unités binaires (Mio, Gio) comme SecMet.
pub fn fmt_size(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.1} Gio", b / GIB)
    } else if b >= MIB {
        format!("{:.0} Mio", b / MIB)
    } else {
        format!("{} Kio", bytes / 1024)
    }
}

/// Taille commerciale (Go décimaux), comme sur l'emballage d'une clé.
pub fn fmt_size_decimal(bytes: u64) -> String {
    format!("{:.0} Go", bytes as f64 / 1e9)
}
