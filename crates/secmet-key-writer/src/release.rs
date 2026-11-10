//! Canal de publication : `latest.json` signé (version courante), adresses
//! des fichiers de la release, téléchargement avec progression.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use secmet_attest::release::{self, LatestRelease, SignedLatest};

/// Clés publiques de version acceptées (hex), comme dans SecMet.
pub const RELEASE_PUBLIC_KEYS: &[&str] =
    &["bde3e3d9de271f8de9bb26c8dd4ed73f4c7344e309ebac7c3dffd50e02ca3bd5"];
/// `latest.json` de la dernière release du dépôt public.
pub const LATEST_URL: &str =
    "https://github.com/hclcdev12/secmet/releases/latest/download/latest.json";
/// Variable d'environnement remplaçant l'adresse du canal (essais).
pub const URL_ENV: &str = "SECMET_UPDATE_URL";
const MAX_SMALL: u64 = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

pub fn trusted_keys() -> Vec<String> {
    RELEASE_PUBLIC_KEYS.iter().map(|k| k.to_string()).collect()
}

pub fn agent() -> ureq::Agent {
    let builder = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(Duration::from_secs(60))
        .user_agent(concat!("secmet-key-writer/", env!("CARGO_PKG_VERSION")));
    #[cfg(windows)]
    let builder = match native_tls::TlsConnector::new() {
        Ok(c) => builder.tls_connector(std::sync::Arc::new(c)),
        Err(_) => builder,
    };
    builder.build()
}

fn http_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("HTTP {code}"),
        ureq::Error::Transport(t) => match t.message() {
            Some(m) => format!("{} : {}", t.kind(), m),
            None => t.kind().to_string(),
        },
    }
}

/// Lit un petit document texte (`latest.json`, `SHA256SUMS`, `.sig`).
pub fn fetch_text(url: &str) -> Result<String, String> {
    if !release::secure_url(url) {
        return Err(format!("adresse non sécurisée : {url}"));
    }
    let resp = agent().get(url).call().map_err(http_error)?;
    let mut body = String::new();
    resp.into_reader()
        .take(MAX_SMALL + 1)
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() as u64 > MAX_SMALL {
        return Err("réponse trop longue".to_string());
    }
    Ok(body)
}

/// Dernière version publiée, `latest.json` vérifié avec la clé de version.
pub fn fetch_latest() -> Result<LatestRelease, String> {
    let url = std::env::var(URL_ENV).unwrap_or_else(|_| LATEST_URL.to_string());
    let text = fetch_text(&url)?;
    let signed: SignedLatest =
        serde_json::from_str(&text).map_err(|e| format!("latest.json illisible : {e}"))?;
    let v = release::verify_latest(&signed, &trusted_keys()).map_err(|e| e.to_string())?;
    if !v.trusted {
        return Err(format!(
            "latest.json signé par une clé inconnue ({})",
            signed.key_id
        ));
    }
    if let Some(moved) = &signed.payload.moved_to {
        return Err(format!(
            "le canal a déménagé vers {moved} : mettre l'outil à jour"
        ));
    }
    Ok(signed.payload)
}

/// Nom du fichier image d'une version.
pub fn image_name(version: &str) -> String {
    format!("secmet-key_{version}_x86_64.img.xz")
}

/// Dossier des fichiers d'une release sur GitHub.
pub fn release_dir_url(version: &str) -> String {
    format!("https://github.com/hclcdev12/secmet/releases/download/v{version}/")
}

/// Adresses de `SHA256SUMS` et `SHA256SUMS.sig` d'une version, à partir de
/// `latest.json` si c'est la version courante, sinon par convention.
pub fn sums_urls(latest: Option<&LatestRelease>, version: &str) -> (String, String) {
    let sums = match latest {
        Some(l) if l.version == version => l.sha256sums_url.clone(),
        _ => format!("{}SHA256SUMS", release_dir_url(version)),
    };
    (format!("{sums}.sig"), sums)
}

/// Dossier de destination des téléchargements.
pub fn download_dir() -> PathBuf {
    dirs::download_dir()
        .filter(|p| p.is_dir())
        .or_else(|| dirs::home_dir().filter(|p| p.is_dir()))
        .unwrap_or_else(std::env::temp_dir)
}

/// Télécharge `url` dans `dest` (via `dest.part`), en appelant `progress`
/// avec les octets reçus et la taille annoncée.
pub fn download(
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), String> {
    if !release::secure_url(url) {
        return Err(format!("adresse non sécurisée : {url}"));
    }
    let resp = agent().get(url).call().map_err(http_error)?;
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let part = dest.with_extension("xz.part");
    let mut out = File::create(&part).map_err(|e| format!("{} : {e}", part.display()))?;
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    progress(0, total);
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(out);
            let _ = std::fs::remove_file(&part);
            return Err("téléchargement annulé".to_string());
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("lecture réseau : {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .map_err(|e| format!("écriture de {} : {e}", part.display()))?;
        done += n as u64;
        progress(done, total);
    }
    out.flush().map_err(|e| e.to_string())?;
    drop(out);
    if let Some(t) = total {
        if t != done {
            let _ = std::fs::remove_file(&part);
            return Err(format!("téléchargement incomplet ({done} octets sur {t})"));
        }
    }
    std::fs::rename(&part, dest).map_err(|e| format!("{} : {e}", dest.display()))?;
    Ok(())
}
