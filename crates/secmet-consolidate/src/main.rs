//! SecMet Regroupement : regroupe les lots clôturés de plusieurs clés
//! SecMet (par exemple 100 portables traités avec 5 clés, un lot par clé)
//! en un seul livrable : un PDF de synthèse, un index JSON et les fichiers
//! d'origine recopiés tels quels. Voir [`secmet_attest::consolidation`].
//!
//! Sans argument : interface graphique. En ligne de commande :
//!
//! ```text
//! secmet-consolidate --out <dossier> [--lang fr|en|de] <lot ou export>...
//! ```
//! Chaque chemin est le dossier d'un lot clôturé (`SECMET-EXP/<LOT>/`) ou
//! la partition d'export d'une clé entière (ses lots sont pris).
//!
//! Windows : exécutable sans console ; la ligne de commande s'attache à la
//! console du parent.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod gui;
mod job;

use std::path::PathBuf;
use std::process::ExitCode;

use secmet_attest::lang::Lang;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return match gui::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    attach_console();
    if args[0] == "--version" {
        println!("secmet-consolidate {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    cli(&args)
}

const USAGE: &str =
    "usage : secmet-consolidate --out <dossier> [--lang fr|en|de] <lot ou partition d'export>...";

fn cli(args: &[String]) -> ExitCode {
    let mut out: Option<PathBuf> = None;
    let mut lang = Lang::Fr;
    let mut sources = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = it.next().map(PathBuf::from),
            "--lang" => {
                lang = match it.next().map(String::as_str) {
                    Some("fr") => Lang::Fr,
                    Some("en") => Lang::En,
                    Some("de") => Lang::De,
                    _ => {
                        eprintln!("{USAGE}");
                        return ExitCode::from(2);
                    }
                }
            }
            s if s.starts_with("--") => {
                eprintln!("argument inconnu : {s}\n{USAGE}");
                return ExitCode::from(2);
            }
            s => sources.push(PathBuf::from(s)),
        }
    }
    let Some(out) = out.filter(|_| !sources.is_empty()) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let mut lots = Vec::new();
    let mut refused = false;
    for s in &sources {
        let found = secmet_attest::consolidation::discover(s);
        for r in &found.refused {
            eprintln!("REFUSÉ  {r}");
            refused = true;
        }
        for l in &found.lots {
            let n = l.certificates.len();
            println!(
                "lot {}  clé {}  {n} {}",
                l.lot_id(),
                l.key_id(),
                if n == 1 { "certificat" } else { "certificats" }
            );
        }
        lots.extend(found.lots);
    }
    if refused {
        eprintln!("Regroupement abandonné : corrigez ou retirez les dossiers refusés.");
        return ExitCode::FAILURE;
    }
    let grouping = match secmet_attest::consolidation::build(lots) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    for w in &grouping.warnings {
        eprintln!("ATTENTION  {w}");
    }
    match job::write(&grouping, &out, lang) {
        Ok(w) => {
            let t = grouping.totals();
            println!(
                "{} : {} {}, {} {}, {} {}",
                w.dir.display(),
                t.disks,
                if t.disks == 1 { "disque" } else { "disques" },
                t.success,
                if t.success == 1 {
                    "réussi"
                } else {
                    "réussis"
                },
                t.redone,
                if t.redone == 1 {
                    "repassé"
                } else {
                    "repassés"
                },
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Windows : l'exécutable sans console s'attache à celle du parent pour
/// que la ligne de commande affiche quelque chose.
fn attach_console() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        // SAFETY: appel Win32 sans pointeur ; échec sans conséquence (pas
        // de console parente).
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}
