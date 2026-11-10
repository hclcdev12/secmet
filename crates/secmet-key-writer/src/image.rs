//! L'image `.img.xz` : version d'après son nom, taille décompressée d'après
//! l'index xz, empreinte et signature de la release.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use secmet_attest::release::{self, SumsSignature};
use sha2::{Digest, Sha256};
use xz_native::XzSeekableReader;

/// Ce que l'index xz dit de l'image.
#[derive(Debug, Clone)]
pub struct ImageInfo {
    /// Taille compressée (le fichier).
    pub compressed_size: u64,
    /// Taille une fois décompressée : ce qui sera écrit sur la clé.
    pub uncompressed_size: u64,
    /// Nombre de blocs xz.
    pub blocks: usize,
}

/// `secmet-key_0.3.13_x86_64.img.xz` → `0.3.13`.
pub fn version_from_name(name: &str) -> Option<String> {
    let rest = name.strip_prefix("secmet-key_")?;
    let (version, suffix) = rest.split_once('_')?;
    (suffix == "x86_64.img.xz" && release::parse_version(version).is_some())
        .then(|| version.to_string())
}

/// Lit l'index xz (fin du fichier) : taille décompressée exacte, sans
/// décompresser.
pub fn inspect(path: &Path) -> Result<ImageInfo, String> {
    let f = File::open(path).map_err(|e| format!("{} : {e}", path.display()))?;
    let compressed_size = f.metadata().map_err(|e| e.to_string())?.len();
    let r = XzSeekableReader::new(BufReader::new(f))
        .map_err(|e| format!("ce fichier n'est pas une image xz valide : {e}"))?;
    Ok(ImageInfo {
        compressed_size,
        uncompressed_size: r.uncompressed_size(),
        blocks: r.block_count(),
    })
}

/// Résultat d'une vérification réussie.
#[derive(Debug, Clone)]
pub struct Verified {
    /// Identifiant court de la clé de version qui a signé.
    pub key_id: String,
    /// Empreinte SHA-256 de l'image (hex).
    pub sha256: String,
}

/// Vérifie `SHA256SUMS.sig` avec les clés de version, puis l'empreinte de
/// `image` contre la ligne correspondante de `SHA256SUMS`.
pub fn verify(
    image: &Path,
    sums_text: &str,
    sig_text: &str,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Verified, String> {
    let sig: SumsSignature =
        serde_json::from_str(sig_text).map_err(|e| format!("SHA256SUMS.sig illisible : {e}"))?;
    let v = release::verify_sums(&sig, sums_text, &crate::release::trusted_keys())
        .map_err(|e| format!("signature de SHA256SUMS invalide : {e}"))?;
    if !v.trusted {
        return Err(format!(
            "SHA256SUMS signé par une clé qui n'est pas la clé de version SecMet ({})",
            sig.key_id
        ));
    }
    let name = image
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "nom de fichier invalide".to_string())?;
    let entries = release::parse_sums(sums_text).map_err(|e| e.to_string())?;
    let expected = entries
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.sha256.clone())
        .ok_or_else(|| format!("{name} n'est pas listé dans SHA256SUMS de cette version"))?;
    let actual = sha256_file(image, cancel, progress)?;
    if actual != expected {
        return Err(
            "empreinte SHA-256 différente : fichier altéré ou téléchargement corrompu".to_string(),
        );
    }
    Ok(Verified {
        key_id: v.key_id,
        sha256: actual,
    })
}

/// Vérification complète d'une image : index xz, puis `SHA256SUMS` et
/// `SHA256SUMS.sig` pris à côté du fichier s'ils y sont, sinon téléchargés
/// pour la version lue dans le nom du fichier.
pub fn verify_with_release(
    path: &Path,
    latest: Option<&secmet_attest::release::LatestRelease>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(Verified, ImageInfo), String> {
    let info = inspect(path)?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let local_sums = dir.join(release::SUMS_FILE);
    let local_sig = dir.join(release::SUMS_SIGNATURE_FILE);
    let (sums, sig) = if local_sums.is_file() && local_sig.is_file() {
        (
            std::fs::read_to_string(&local_sums).map_err(|e| e.to_string())?,
            std::fs::read_to_string(&local_sig).map_err(|e| e.to_string())?,
        )
    } else {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let version = version_from_name(name).ok_or_else(|| {
            "nom de fichier modifié : impossible de retrouver la version pour vérifier la signature \
             (renommer en secmet-key_<version>_x86_64.img.xz ou placer SHA256SUMS et SHA256SUMS.sig à côté)"
                .to_string()
        })?;
        let (sig_url, sums_url) = crate::release::sums_urls(latest, &version);
        (
            crate::release::fetch_text(&sums_url).map_err(|e| format!("SHA256SUMS : {e}"))?,
            crate::release::fetch_text(&sig_url).map_err(|e| format!("SHA256SUMS.sig : {e}"))?,
        )
    };
    let v = verify(path, &sums, &sig, cancel, progress)?;
    Ok((v, info))
}

/// SHA-256 hex d'un fichier, avec progression.
pub fn sha256_file(
    path: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<String, String> {
    let f = File::open(path).map_err(|e| format!("{} : {e}", path.display()))?;
    let total = f.metadata().map_err(|e| e.to_string())?.len();
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("vérification annulée".to_string());
        }
        let n = r.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    Ok(secmet_attest::hex(&h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing() {
        assert_eq!(
            version_from_name("secmet-key_0.3.13_x86_64.img.xz").as_deref(),
            Some("0.3.13")
        );
        assert_eq!(version_from_name("secmet-key_0.3.13_x86_64.img"), None);
        assert_eq!(version_from_name("secmet-live_0.3.13_x86_64.tar"), None);
        assert_eq!(version_from_name("secmet-key_abc_x86_64.img.xz"), None);
    }
}
