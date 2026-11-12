//! `secmet-release` : clé de version, signature et vérification d'une
//! release (Annexe N).
//!
//! ```text
//! secmet-release keygen <clé.hex>
//! secmet-release sign <dossier> --key <clé.hex> --version X.Y.Z --date AAAA-MM-JJ
//!                     --sums-url <https://…/SHA256SUMS> [--notes-fr|-en|-de <texte>]
//!                     [--moved-to <https://…>] [fichier...]
//! secmet-release verify <dossier> [--trust <clé ou key_id hex>]...
//! ```
//! `sign` écrit `SHA256SUMS`, `SHA256SUMS.sig` et `latest.json` dans le
//! dossier ; sans liste de fichiers, il prend les `.deb`, `.rpm`,
//! `.AppImage`, `.exe` (SecMet Clé USB pour Windows), `.tar` (livrable de
//! la clé USB) et `.img.xz` (image de clé USB) qui s'y trouvent. `verify` contrôle les deux signatures,
//! l'empreinte de chaque fichier et la cohérence de la version ; code 0 si
//! tout est valide **et** signé par une clé de confiance (`--trust` ou
//! `$SECMET_RELEASE_TRUSTED_KEYS`, séparateur `:`), 1 sinon, 2 sur usage.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use secmet_attest::release::{self, LatestRelease, SignedLatest, SumsSignature};
use secmet_attest::{keystore, AttestError};

const USAGE: &str = "usage: secmet-release keygen <clé.hex>
       secmet-release sign <dossier> --key <clé.hex> --version X.Y.Z --date AAAA-MM-JJ \
--sums-url <https://…/SHA256SUMS> [--notes-fr|--notes-en|--notes-de <texte>] \
[--moved-to <https://…>] [fichier...]
       secmet-release verify <dossier> [--trust <clé ou key_id hex>]... [--file <nom>]
         (--file : ne contrôle que ce fichier parmi ceux listés par SHA256SUMS)";

fn usage() -> i32 {
    eprintln!("{USAGE}");
    2
}

fn fail(message: impl std::fmt::Display) -> i32 {
    eprintln!("erreur : {message}");
    1
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => keygen(&args[1..]),
        Some("sign") => sign(&args[1..]),
        Some("verify") => verify(&args[1..]),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            0
        }
        _ => usage(),
    }
}

fn keygen(args: &[String]) -> i32 {
    let [path] = args else { return usage() };
    let path = Path::new(path);
    if path.exists() {
        return fail(format!(
            "{} existe déjà : rien n'est écrasé",
            path.display()
        ));
    }
    match keystore::load_or_create(path) {
        Ok(identity) => {
            println!(
                "clé privée   : {} (0600, à sauvegarder hors ligne)",
                path.display()
            );
            println!(
                "clé publique : {}",
                keystore::public_key_path(path).display()
            );
            println!("public_key   : {}", identity.public_key_hex());
            println!("key_id       : {}", identity.key_id());
            0
        }
        Err(e) => fail(e),
    }
}

/// Fichiers publiables d'un dossier, triés.
fn package_files(dir: &Path) -> Result<Vec<String>, AttestError> {
    let entries = fs::read_dir(dir).map_err(|e| AttestError::io(dir.display().to_string(), e))?;
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.ends_with(".deb")
                || name.ends_with(".rpm")
                || name.ends_with(".AppImage")
                || name.ends_with(".exe")
                || name.ends_with(".img.xz")
                || name.ends_with(".tar")
        })
        .collect();
    names.sort();
    Ok(names)
}

fn sign(args: &[String]) -> i32 {
    let Some(dir) = args.first() else {
        return usage();
    };
    let dir = PathBuf::from(dir);
    let mut key: Option<PathBuf> = None;
    let mut version: Option<String> = None;
    let mut date: Option<String> = None;
    let mut sums_url: Option<String> = None;
    let mut moved_to: Option<String> = None;
    let mut notes = BTreeMap::new();
    let mut files: Vec<String> = Vec::new();
    let mut iter = args[1..].iter();
    while let Some(arg) = iter.next() {
        let value = |iter: &mut std::slice::Iter<String>| iter.next().cloned();
        match arg.as_str() {
            "--key" => key = value(&mut iter).map(PathBuf::from),
            "--version" => version = value(&mut iter),
            "--date" => date = value(&mut iter),
            "--sums-url" => sums_url = value(&mut iter),
            "--moved-to" => moved_to = value(&mut iter),
            "--notes-fr" | "--notes-en" | "--notes-de" => {
                let Some(text) = value(&mut iter) else {
                    return usage();
                };
                notes.insert(arg.trim_start_matches("--notes-").to_string(), text);
            }
            other if other.starts_with('-') => {
                eprintln!("option inconnue : {other}");
                return usage();
            }
            other => files.push(other.to_string()),
        }
    }
    let (Some(key), Some(version), Some(date), Some(sums_url)) = (key, version, date, sums_url)
    else {
        return usage();
    };
    if !key.exists() {
        return fail(format!(
            "clé de version introuvable : {} (secmet-release keygen)",
            key.display()
        ));
    }
    let identity = match keystore::load_or_create(&key) {
        Ok(identity) => identity,
        Err(e) => return fail(e),
    };
    if files.is_empty() {
        files = match package_files(&dir) {
            Ok(names) => names,
            Err(e) => return fail(e),
        };
    }
    if files.is_empty() {
        return fail(format!("aucun paquet dans {}", dir.display()));
    }
    if !files.iter().any(|f| f.contains(version.as_str())) {
        return fail(format!("aucun fichier ne porte la version {version}"));
    }

    let entries = match release::sums_of(&dir, &files) {
        Ok(entries) => entries,
        Err(e) => return fail(e),
    };
    let sums_text = release::format_sums(&entries);
    let sums_sig = release::sign_sums(&identity, &sums_text);
    let latest = LatestRelease {
        version: version.clone(),
        published_at: date,
        notes,
        sha256sums_url: sums_url,
        moved_to,
    };
    let signed_latest = match release::sign_latest(&identity, &latest) {
        Ok(signed) => signed,
        Err(e) => return fail(e),
    };
    let outputs = [
        (release::SUMS_FILE, sums_text.clone()),
        (
            release::SUMS_SIGNATURE_FILE,
            serde_json::to_string_pretty(&sums_sig).unwrap_or_default() + "\n",
        ),
        (
            release::LATEST_FILE,
            serde_json::to_string_pretty(&signed_latest).unwrap_or_default() + "\n",
        ),
    ];
    for (name, content) in outputs {
        if let Err(e) = fs::write(dir.join(name), content) {
            return fail(format!("{name} : {e}"));
        }
    }
    print!("{sums_text}");
    println!(
        "signé avec la clé {} (version {version})",
        identity.key_id()
    );
    0
}

fn trusted_keys(args: &[String]) -> Option<Vec<String>> {
    let mut keys: Vec<String> = std::env::var("SECMET_RELEASE_TRUSTED_KEYS")
        .map(|env| {
            env.split(':')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--trust" => keys.push(iter.next()?.clone()),
            _ => return None,
        }
    }
    Some(keys)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, AttestError> {
    let text =
        fs::read_to_string(path).map_err(|e| AttestError::io(path.display().to_string(), e))?;
    Ok(serde_json::from_str(&text)?)
}

fn verify(args: &[String]) -> i32 {
    let Some(dir) = args.first() else {
        return usage();
    };
    let dir = PathBuf::from(dir);
    // --file <nom> : contrôle limité à ce fichier (clé live : un seul paquet
    // conservé sur la partition de données).
    let mut rest: Vec<String> = Vec::new();
    let mut only: Option<String> = None;
    let mut iter = args[1..].iter();
    while let Some(arg) = iter.next() {
        if arg == "--file" {
            match iter.next() {
                Some(name) => only = Some(name.clone()),
                None => return usage(),
            }
        } else {
            rest.push(arg.clone());
        }
    }
    let Some(trusted) = trusted_keys(&rest) else {
        return usage();
    };
    let sums_path = dir.join(release::SUMS_FILE);
    let sums_text = match fs::read_to_string(&sums_path) {
        Ok(text) => text,
        Err(e) => return fail(format!("{} : {e}", sums_path.display())),
    };
    let sums_sig: SumsSignature = match read_json(&dir.join(release::SUMS_SIGNATURE_FILE)) {
        Ok(sig) => sig,
        Err(e) => return fail(e),
    };
    let latest: SignedLatest = match read_json(&dir.join(release::LATEST_FILE)) {
        Ok(latest) => latest,
        Err(e) => return fail(e),
    };

    let sums_ok = match release::verify_sums(&sums_sig, &sums_text, &trusted) {
        Ok(v) => v,
        Err(e) => return fail(format!("{} : {e}", release::SUMS_SIGNATURE_FILE)),
    };
    let entries = match release::parse_sums(&sums_text) {
        Ok(entries) => entries,
        Err(e) => return fail(e),
    };
    let checked: Vec<release::SumEntry> = match &only {
        None => entries.clone(),
        Some(name) => {
            let selected: Vec<release::SumEntry> = entries
                .iter()
                .filter(|e| &e.name == name)
                .cloned()
                .collect();
            if selected.is_empty() {
                return fail(format!("{name} absent de {}", release::SUMS_FILE));
            }
            selected
        }
    };
    if let Err(e) = release::check_sums(&dir, &checked) {
        return fail(e);
    }
    let latest_ok = match release::verify_latest(&latest, &trusted) {
        Ok(v) => v,
        Err(e) => return fail(format!("{} : {e}", release::LATEST_FILE)),
    };
    let version = &latest.payload.version;
    if !entries.iter().any(|e| e.name.contains(version.as_str())) {
        return fail(format!(
            "{} annonce la version {version}, absente des fichiers",
            release::LATEST_FILE
        ));
    }
    if latest.key_id != sums_sig.key_id {
        return fail("SHA256SUMS.sig et latest.json signés par des clés différentes");
    }

    for e in &checked {
        println!("{}  {}", e.sha256, e.name);
    }
    println!(
        "version      : {} ({})",
        version, latest.payload.published_at
    );
    println!(
        "clé          : {} ({})",
        latest_ok.key_id,
        if latest_ok.trusted && sums_ok.trusted {
            "de confiance"
        } else {
            "non listée"
        }
    );
    println!("signatures   : VALIDES");
    if latest_ok.trusted && sums_ok.trusted {
        0
    } else {
        eprintln!("clé non vérifiée : ajoutez-la avec --trust");
        1
    }
}
