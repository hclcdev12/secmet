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

mod disks;
mod gui;
mod image;
mod platform;
mod release;
mod worker;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
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
        Some(path) if !path.starts_with("--") => {
            match gui::run(Some(std::path::PathBuf::from(path))) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(other) => {
            eprintln!("argument inconnu : {other}");
            ExitCode::from(2)
        }
        None => match gui::run(None) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        },
    }
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
