//! `secmet-verify` : vérification hors ligne d'un certificat SecMet.
//!
//! ```text
//! secmet-verify <certificat.json> [--trust <clé publique ou key_id hex>]...
//! secmet-verify --qr "<texte scanné SM1:...>" [--trust <hex>]...
//! secmet-verify --qr-stdin [--trust <hex>]...
//! secmet-verify --manifest <lot-manifest.json> [--trust <hex>]...
//! ```
//! `--manifest` vérifie la signature d'un manifeste de lot (Annexe M) et
//! le confronte aux attestations `.json` présentes dans le même dossier :
//! certificat manquant, modifié ou en trop.
//! Une signature valide ne vaut que pour une clé **de confiance** : la
//! liste vient de `--trust`, de `$SECMET_TRUSTED_KEYS` (séparateur `:`),
//! de `/etc/secmet/trusted-keys`, de `~/.config/secmet/attestation-key.pub`
//! et, si `SECMET_DATA_DIR` est défini, de `<dossier>/config/`
//! (une clé ou un `key_id` par ligne, `#` commente). Code de sortie 0 si
//! toutes les signatures sont valides **et** signées par une clé de
//! confiance, 1 sinon ; `--allow-untrusted` ramène à 0 pour une clé
//! inconnue, en l'affichant clairement.

use std::io::Read;
use std::path::PathBuf;

use secmet_attest::sign::{self, SignedAttestation};
use secmet_attest::{export, manifest, qr};

const USAGE: &str = "usage: secmet-verify <certificat.json> | --qr <texte> | --qr-stdin | --manifest <lot-manifest.json> \
                     [--trust <clé ou key_id hex>]... [--allow-untrusted]";

fn usage() -> i32 {
    eprintln!("{USAGE}");
    2
}

/// Clés de confiance des emplacements par défaut (voir la doc du module).
fn default_trusted_keys() -> Vec<String> {
    let mut keys = Vec::new();
    if let Ok(env) = std::env::var("SECMET_TRUSTED_KEYS") {
        keys.extend(
            env.split(':')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(String::from),
        );
    }
    let mut files = vec![PathBuf::from("/etc/secmet/trusted-keys")];
    if let Some(data) = std::env::var_os("SECMET_DATA_DIR").map(PathBuf::from) {
        files.push(data.join("config").join("attestation-key.pub"));
        files.push(data.join("config").join("trusted-keys"));
    }
    if let Some(cfg) = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    {
        files.push(cfg.join("secmet").join("attestation-key.pub"));
        files.push(cfg.join("secmet").join("trusted-keys"));
    }
    for file in files {
        if let Ok(text) = std::fs::read_to_string(&file) {
            keys.extend(
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(String::from),
            );
        }
    }
    keys
}

fn describe(signed: &SignedAttestation, trusted: bool) {
    let a = &signed.payload;
    println!("certificat   : {}", a.certificate_id);
    println!(
        "disque       : {} S/N {} ({})",
        a.disk.model, a.disk.serial, a.disk.media
    );
    println!(
        "méthode      : {} [{}] → {} ({:?})",
        a.erasure.method_label,
        a.erasure.method_code,
        a.erasure.nist_level.label(),
        a.erasure.outcome
    );
    if let Some(d) = a.erasure.disposition {
        let reasons: Vec<String> = a
            .erasure
            .disposition_reasons
            .iter()
            .map(|c| secmet_attest::lang::reason_label(c, secmet_attest::lang::Lang::Fr))
            .collect();
        println!(
            "sort         : {}{}",
            secmet_attest::lang::disposition_label(d, secmet_attest::lang::Lang::Fr),
            if reasons.is_empty() {
                String::new()
            } else {
                format!(" ({})", reasons.join(" · "))
            }
        );
    }
    println!("opérateur    : {} · {}", a.operator, a.issued_at);
    println!(
        "clé          : {} ({})",
        signed.key_id,
        if trusted {
            "de confiance"
        } else {
            "non listée"
        }
    );
    println!("signature    : VALIDE");
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut trusted = default_trusted_keys();
    let mut allow_untrusted = false;
    let mut file: Option<PathBuf> = None;
    let mut qr_text: Option<String> = None;
    let mut manifest_file: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--trust" => match iter.next() {
                Some(key) => trusted.push(key.clone()),
                None => return usage(),
            },
            "--allow-untrusted" => allow_untrusted = true,
            "--manifest" => match iter.next() {
                Some(path) => manifest_file = Some(PathBuf::from(path)),
                None => return usage(),
            },
            "--qr" => match iter.next() {
                Some(text) => qr_text = Some(text.clone()),
                None => return usage(),
            },
            "--qr-stdin" => {
                let mut text = String::new();
                if std::io::stdin().read_to_string(&mut text).is_err() {
                    return usage();
                }
                qr_text = Some(text);
            }
            // Aide demandée : usage sur stdout, code 0 (une option inconnue
            // garde l'usage sur stderr et le code 2).
            "-h" | "--help" => {
                println!("{}", USAGE.trim_end());
                return 0;
            }
            other if other.starts_with('-') => {
                eprintln!("option inconnue : {other}");
                return usage();
            }
            other => file = Some(PathBuf::from(other)),
        }
    }
    if let Some(path) = manifest_file {
        return verify_manifest_file(&path, &trusted, allow_untrusted);
    }
    let results = match (file, qr_text) {
        (Some(path), None) => export::verify_file(&path, &trusted),
        (None, Some(text)) => qr::decode_text(&text)
            .and_then(|signed| sign::verify(&signed, &trusted).map(|v| vec![(signed, v)])),
        _ => return usage(),
    };
    match results {
        Ok(entries) if entries.is_empty() => {
            eprintln!("aucune attestation trouvée");
            1
        }
        Ok(entries) => {
            for (i, (signed, verified)) in entries.iter().enumerate() {
                if i > 0 {
                    println!("---");
                }
                describe(signed, verified.trusted);
            }
            if entries.iter().any(|(_, v)| !v.trusted) {
                eprintln!(
                    "clé non vérifiée : la signature est valide mais la clé n'est pas de \
                     confiance (--trust <clé>, /etc/secmet/trusted-keys, \
                     ~/.config/secmet/attestation-key.pub){}",
                    if allow_untrusted {
                        " — acceptée (--allow-untrusted)"
                    } else {
                        ""
                    }
                );
                if !allow_untrusted {
                    return 1;
                }
            }
            0
        }
        Err(e) => {
            eprintln!("signature    : INVALIDE — {e}");
            1
        }
    }
}

/// Manifeste de lot : signature, clé de confiance, puis confrontation aux
/// attestations du même dossier (celles listées doivent être présentes et
/// intactes ; aucune en trop).
fn verify_manifest_file(path: &PathBuf, trusted: &[String], allow_untrusted: bool) -> i32 {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{} : {e}", path.display());
            return 1;
        }
    };
    let signed: manifest::SignedManifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{} : manifeste illisible ({e})", path.display());
            return 1;
        }
    };
    let verified = match manifest::verify_manifest(&signed, trusted) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("manifeste : {e}");
            println!("signature    : INVALIDE");
            return 1;
        }
    };
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut present = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p == *path || p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            // Une attestation seule ; le JSON du lot (tableau) et les autres
            // fichiers sont ignorés.
            if let Ok(att) = serde_json::from_str::<SignedAttestation>(&text) {
                present.push(att);
            }
        }
    }
    let p = &signed.payload;
    println!(
        "lot          : {} · {} · {}",
        p.lot_id, p.operator, p.closed_at
    );
    println!(
        "certificats  : {} listés, {} machine(s)",
        p.certificates.len(),
        p.machines
    );
    println!(
        "clé          : {} ({})",
        verified.key_id,
        if verified.trusted {
            "de confiance"
        } else {
            "non listée"
        }
    );
    println!("signature    : VALIDE");
    let problems = manifest::reconcile(p, &present).unwrap_or_default();
    for problem in &problems {
        println!("écart        : {problem}");
    }
    if problems.is_empty() {
        println!(
            "attestations : {} présentes, conformes au manifeste",
            present.len()
        );
    }
    if !problems.is_empty() {
        return 1;
    }
    if verified.trusted || allow_untrusted {
        0
    } else {
        eprintln!("clé non vérifiée : la signature est valide mais la clé n'est pas de confiance");
        1
    }
}
