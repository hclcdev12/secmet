//! Installation d'une mise à jour depuis le canal de publication (Annexe N,
//! §N.5) : téléchargement du paquet de la plateforme, vérification de son
//! empreinte contre `SHA256SUMS` signé par la clé de version, puis
//! installation par le gestionnaire de paquets sous `pkexec` (mot de passe
//! administrateur demandé par PolicyKit). Rien n'est exécuté sans
//! empreinte vérifiée.

use std::ffi::CStr;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use sha2::Digest;

use secmet_attest::release::{self, LatestRelease, SumsSignature};

use crate::update::RELEASE_PUBLIC_KEYS;

/// Taille maximale d'un paquet téléchargé.
const MAX_PACKAGE: u64 = 256 * 1024 * 1024;
/// Délai réseau par requête.
const TIMEOUT: Duration = Duration::from_secs(300);
/// Taille maximale de `SHA256SUMS` et de sa signature.
const MAX_TEXT: u64 = 256 * 1024;

/// Paquet attendu pour ce poste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    /// `.deb`, construit pour Ubuntu 24.04 (glibc ≥ 2.39) ou 22.04.
    Deb {
        /// Suffixe de distribution du nom de fichier.
        distro: &'static str,
    },
    /// `.rpm` Fedora.
    Rpm,
}

impl PackageKind {
    /// Nom de fichier publié pour `version`.
    pub fn asset_name(self, version: &str) -> String {
        match self {
            // Nom publié : convention GitHub, qui remplace « ~ » par « . ».
            PackageKind::Deb { distro } => format!("secmet_{version}.{distro}_amd64.deb"),
            PackageKind::Rpm => format!("secmet-{version}-1.fc42.x86_64.rpm"),
        }
    }

    /// Commande d'installation (sous `pkexec`).
    pub fn install_command(self, path: &str) -> Vec<String> {
        let mut cmd: Vec<String> = vec!["pkexec".into()];
        match self {
            PackageKind::Deb { .. } => cmd.extend(
                ["apt-get", "install", "-y", "--allow-downgrades", path]
                    .iter()
                    .map(|s| s.to_string()),
            ),
            PackageKind::Rpm => {
                cmd.extend(["dnf", "install", "-y", path].iter().map(|s| s.to_string()))
            }
        }
        cmd
    }
}

/// Version de la glibc du poste (`"2.39"`), `None` hors glibc.
fn glibc_version() -> Option<(u32, u32)> {
    // SAFETY : gnu_get_libc_version rend une chaîne C statique, valide
    // pendant toute l'exécution ; on ne fait que la lire.
    let raw = unsafe { CStr::from_ptr(libc::gnu_get_libc_version()) };
    parse_glibc(raw.to_str().ok()?)
}

/// `"2.39"` → `(2, 39)`.
pub fn parse_glibc(text: &str) -> Option<(u32, u32)> {
    let (major, minor) = text.trim().split_once('.')?;
    let minor: String = minor.chars().take_while(|c| c.is_ascii_digit()).collect();
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// Choix du `.deb` selon la glibc : le paquet Ubuntu 24.04 exige 2.39.
pub fn deb_distro_for(glibc: Option<(u32, u32)>) -> &'static str {
    match glibc {
        Some((major, minor)) if (major, minor) >= (2, 39) => "ubuntu24.04",
        _ => "ubuntu22.04",
    }
}

fn has_tool(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

/// Paquet installable sur ce poste, `None` si aucun gestionnaire connu
/// (AppImage sur une autre distribution : lien de téléchargement seulement).
pub fn package_kind() -> Option<PackageKind> {
    if has_tool("apt-get") && has_tool("dpkg") {
        Some(PackageKind::Deb {
            distro: deb_distro_for(glibc_version()),
        })
    } else if has_tool("dnf") && has_tool("rpm") {
        Some(PackageKind::Rpm)
    } else {
        None
    }
}

fn agent() -> ureq::Agent {
    crate::update::agent(TIMEOUT)
}

use crate::update::http_error;

fn get_text(url: &str) -> Result<String, String> {
    let response = agent()
        .get(url)
        .set(
            "User-Agent",
            &format!("secmet/{}", crate::update::INSTALLED_VERSION),
        )
        .call()
        .map_err(http_error)?;
    let mut text = String::new();
    response
        .into_reader()
        .take(MAX_TEXT)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text)
}

/// Dossier des téléchargements de mise à jour.
fn download_dir() -> PathBuf {
    std::env::temp_dir().join(format!("secmet-update-{}", unsafe_uid()))
}

fn unsafe_uid() -> u32 {
    // SAFETY : getuid n'a pas d'effet de bord et ne peut pas échouer.
    unsafe { libc::getuid() }
}

/// Télécharge le paquet de `latest` pour `kind` et vérifie son empreinte
/// contre `SHA256SUMS` (signé). Bloquant : hors du fil GTK. Rend le chemin
/// du fichier vérifié.
pub fn download_and_verify(latest: &LatestRelease, kind: PackageKind) -> Result<PathBuf, String> {
    let base = latest
        .sha256sums_url
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .ok_or_else(|| "adresse de SHA256SUMS invalide".to_string())?;
    if !base.starts_with("https://") && !base.starts_with("http://") {
        return Err("adresse de SHA256SUMS invalide".to_string());
    }
    let sums_text = get_text(&latest.sha256sums_url)?;
    let sig_text = get_text(&format!("{base}/{}", release::SUMS_SIGNATURE_FILE))?;
    let sig: SumsSignature =
        serde_json::from_str(&sig_text).map_err(|_| "SHA256SUMS.sig illisible".to_string())?;
    let trusted: Vec<String> = RELEASE_PUBLIC_KEYS.iter().map(|k| k.to_string()).collect();
    let verified = release::verify_sums(&sig, &sums_text, &trusted).map_err(|e| e.to_string())?;
    if !verified.trusted {
        return Err("SHA256SUMS signé par une clé inconnue".to_string());
    }
    let name = kind.asset_name(&latest.version);
    let expected = release::parse_sums(&sums_text)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|e| e.name == name)
        .map(|e| e.sha256)
        .ok_or_else(|| format!("{name} absent de SHA256SUMS"))?;

    let dir = download_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    let path = dir.join(&name);
    let tmp = dir.join(format!("{name}.part"));
    let response = agent()
        .get(&format!("{base}/{name}"))
        .set(
            "User-Agent",
            &format!("secmet/{}", crate::update::INSTALLED_VERSION),
        )
        .call()
        .map_err(http_error)?;
    let mut reader = response.into_reader().take(MAX_PACKAGE + 1);
    let mut file = fs::File::create(&tmp).map_err(|e| format!("{} : {e}", tmp.display()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_PACKAGE {
            let _ = fs::remove_file(&tmp);
            return Err("paquet trop volumineux".to_string());
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    let digest = format!("{:x}", hasher.finalize());
    if digest != expected {
        let _ = fs::remove_file(&tmp);
        return Err(format!("{name} : empreinte différente de SHA256SUMS"));
    }
    fs::rename(&tmp, &path).map_err(|e| format!("{} : {e}", path.display()))?;
    // Clé live : le paquet vérifié et les fichiers signés sont conservés
    // sur la partition de données ; le service secmet-app-update les
    // revérifie et installe la version au démarrage suivant (Annexe N §N.6).
    if let Some(store) = crate::paths::app_store_dir() {
        store_for_key(&store, &latest.version, &path, &sums_text, &sig_text)?;
    }
    Ok(path)
}

/// Copie le paquet et ses preuves dans `<app>/<version>/`, puis ne garde
/// que les deux versions les plus récentes (la précédente sert de repli).
fn store_for_key(
    store: &std::path::Path,
    version: &str,
    package: &std::path::Path,
    sums_text: &str,
    sig_text: &str,
) -> Result<(), String> {
    let dir = store.join(version);
    fs::create_dir_all(&dir).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "{} : {e} (dossier app/ de la partition de données absent ou réservé à root : \
                 redémarrer la clé le recrée, ou le créer pour l'opérateur)",
                dir.display()
            )
        } else {
            format!("{} : {e}", dir.display())
        }
    })?;
    let name = package
        .file_name()
        .ok_or_else(|| "nom de paquet invalide".to_string())?;
    fs::copy(package, dir.join(name)).map_err(|e| format!("{} : {e}", dir.display()))?;
    fs::write(dir.join(release::SUMS_FILE), sums_text).map_err(|e| e.to_string())?;
    fs::write(dir.join(release::SUMS_SIGNATURE_FILE), sig_text).map_err(|e| e.to_string())?;
    if let Some(signed) = crate::update::load_state().latest {
        if signed.payload.version == version {
            let text = serde_json::to_string_pretty(&signed).map_err(|e| e.to_string())?;
            fs::write(dir.join(release::LATEST_FILE), text).map_err(|e| e.to_string())?;
        }
    }
    for old in versions_to_prune(&list_versions(store), 2) {
        let _ = fs::remove_dir_all(store.join(old));
    }
    Ok(())
}

/// Sous-dossiers de `store` qui sont des versions `X.Y.Z`.
fn list_versions(store: &std::path::Path) -> Vec<String> {
    fs::read_dir(store)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| {
                    n.split('.').count() == 3 && n.split('.').all(|p| p.parse::<u32>().is_ok())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Versions à supprimer pour n'en garder que `keep` (les plus récentes).
pub fn versions_to_prune(versions: &[String], keep: usize) -> Vec<String> {
    let mut sorted: Vec<String> = versions.to_vec();
    sorted.sort_by(|a, b| {
        if release::is_newer(a, b) {
            std::cmp::Ordering::Greater
        } else if release::is_newer(b, a) {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    });
    sorted.reverse();
    sorted.into_iter().skip(keep).collect()
}

/// Installe le paquet vérifié (bloquant, PolicyKit demande le mot de passe).
/// Rend les dernières lignes de sortie du gestionnaire de paquets.
pub fn install(kind: PackageKind, path: &std::path::Path) -> Result<String, String> {
    let path = path.to_string_lossy().to_string();
    let cmd = kind.install_command(&path);
    let output = Command::new(&cmd[0])
        .args(&cmd[1..])
        .env("DEBIAN_FRONTEND", "noninteractive")
        .output()
        .map_err(|e| format!("{} : {e}", cmd[0]))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let tail: String = text
        .lines()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    match output.status.code() {
        Some(0) => Ok(tail),
        // pkexec : 126 annulé, 127 refusé.
        Some(126) | Some(127) => Err("Authentification annulée ou refusée.".to_string()),
        other => Err(format!("installation échouée (code {other:?})\n{tail}")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn asset_names_follow_the_release_layout() {
        assert_eq!(
            PackageKind::Deb {
                distro: "ubuntu24.04"
            }
            .asset_name("0.3.2"),
            "secmet_0.3.2.ubuntu24.04_amd64.deb"
        );
        assert_eq!(
            PackageKind::Rpm.asset_name("0.3.2"),
            "secmet-0.3.2-1.fc42.x86_64.rpm"
        );
    }

    #[test]
    fn deb_flavour_follows_glibc() {
        assert_eq!(parse_glibc("2.39"), Some((2, 39)));
        assert_eq!(parse_glibc("2.35-0ubuntu3"), Some((2, 35)));
        assert_eq!(deb_distro_for(Some((2, 41))), "ubuntu24.04");
        assert_eq!(deb_distro_for(Some((2, 39))), "ubuntu24.04");
        assert_eq!(deb_distro_for(Some((2, 35))), "ubuntu22.04");
        assert_eq!(deb_distro_for(None), "ubuntu22.04");
    }

    #[test]
    fn only_the_two_newest_versions_are_kept() {
        let v: Vec<String> = ["0.3.4", "0.3.10", "0.3.9", "0.4.0"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut pruned = versions_to_prune(&v, 2);
        pruned.sort();
        assert_eq!(pruned, vec!["0.3.4".to_string(), "0.3.9".to_string()]);
        assert!(versions_to_prune(&v[..1], 2).is_empty());
    }

    #[test]
    fn install_runs_under_pkexec() {
        let cmd = PackageKind::Deb {
            distro: "ubuntu24.04",
        }
        .install_command("/tmp/x.deb");
        assert_eq!(cmd[0], "pkexec");
        assert_eq!(cmd[1], "apt-get");
        assert!(cmd.contains(&"/tmp/x.deb".to_string()));
        assert_eq!(PackageKind::Rpm.install_command("/tmp/x.rpm")[1], "dnf");
    }
}
