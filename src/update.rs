//! Vérification des mises à jour (Annexe N) : lecture de `latest.json`
//! signé sur le canal de publication, contrôle de la signature avec la clé
//! publique de version intégrée, comparaison avec la version installée.
//!
//! Le réseau n'est jamais bloquant : délai court, échec silencieux qui
//! laisse l'état précédent et un message. Aucune donnée n'est envoyée :
//! une requête HTTPS vers l'adresse du canal, une fois par jour au plus.
//! Ce module est pur (pas de GTK) ; le fil d'exécution et le point jaune
//! sont dans `ui::update_ui`.

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use secmet_attest::release::{self, LatestRelease, SignedLatest};
use serde::{Deserialize, Serialize};

/// Clés publiques de version acceptées (hex). Une nouvelle clé est livrée
/// par une version signée avec l'ancienne, qui reste listée.
pub const RELEASE_PUBLIC_KEYS: &[&str] =
    &["bde3e3d9de271f8de9bb26c8dd4ed73f4c7344e309ebac7c3dffd50e02ca3bd5"];
/// Adresse par défaut de `latest.json` : dernière release du dépôt public.
pub const DEFAULT_URL: &str =
    "https://github.com/hclcdev12/secmet-releases/releases/latest/download/latest.json";
/// Variable d'environnement remplaçant l'adresse du canal.
pub const URL_ENV: &str = "SECMET_UPDATE_URL";
/// Version installée.
pub const INSTALLED_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Intervalle minimal entre deux vérifications automatiques.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 3600);
/// Délai réseau total d'une vérification.
const TIMEOUT: Duration = Duration::from_secs(8);
/// Taille maximale acceptée pour `latest.json`.
const MAX_BODY: u64 = 64 * 1024;
/// Fichier d'état.
const STATE_FILE: &str = "update-state.json";

/// État persistant de la vérification (`update-state.json` dans le dossier
/// de configuration).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateState {
    /// Dernière vérification tentée (réussie ou non).
    #[serde(default)]
    pub checked_at: Option<DateTime<Utc>>,
    /// Dernier `latest.json` valide (signature vérifiée).
    #[serde(default)]
    pub latest: Option<SignedLatest>,
    /// Erreur de la dernière vérification, sinon `None`.
    #[serde(default)]
    pub error: Option<String>,
    /// Nouvelle adresse du canal annoncée (signée) par `latest.json`.
    #[serde(default)]
    pub moved_to: Option<String>,
}

impl UpdateState {
    /// Version plus récente que celle installée, si connue.
    pub fn available(&self) -> Option<&LatestRelease> {
        self.latest
            .as_ref()
            .map(|signed| &signed.payload)
            .filter(|latest| release::is_newer(&latest.version, INSTALLED_VERSION))
    }

    /// Une vérification automatique est-elle due ? Jamais vérifié, plus de
    /// [`CHECK_INTERVAL`] écoulé, ou horloge revenue en arrière.
    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        match self.checked_at {
            None => true,
            Some(then) => {
                let elapsed = (now - then).num_seconds();
                elapsed < 0
                    || u64::try_from(elapsed).unwrap_or(u64::MAX) >= CHECK_INTERVAL.as_secs()
            }
        }
    }
}

/// Adresse consultée : variable d'environnement (`http://` toléré, pour les
/// essais sur un serveur local), puis réglage `update_url`, puis
/// déménagement annoncé par le canal, puis adresse par défaut ; `https://`
/// obligatoire pour tout ce qui vient d'un fichier.
pub fn url() -> String {
    if let Ok(url) = std::env::var(URL_ENV) {
        if url.starts_with("https://") || url.starts_with("http://") {
            return url;
        }
    }
    if let Some(url) = crate::settings::load().update_url {
        if url.starts_with("https://") {
            return url;
        }
    }
    if let Some(url) = load_state().moved_to {
        if url.starts_with("https://") {
            return url;
        }
    }
    DEFAULT_URL.to_string()
}

/// Chemin du fichier d'état.
pub fn state_path() -> Option<PathBuf> {
    Some(crate::paths::config_file(STATE_FILE))
}

/// État enregistré ; vide si absent ou illisible.
pub fn load_state() -> UpdateState {
    state_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Enregistrement silencieux : un échec n'empêche pas l'affichage.
pub fn save_state(state: &UpdateState) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(path, json);
    }
}

/// Analyse et vérifie un `latest.json` : signature valide **et** clé de
/// version connue. Les messages sont des clés de traduction.
pub fn parse_and_verify(text: &str) -> Result<SignedLatest, String> {
    let signed: SignedLatest =
        serde_json::from_str(text).map_err(|_| "latest.json illisible".to_string())?;
    let trusted: Vec<String> = RELEASE_PUBLIC_KEYS.iter().map(|k| k.to_string()).collect();
    let verified = release::verify_latest(&signed, &trusted).map_err(|e| e.to_string())?;
    if !verified.trusted {
        return Err("latest.json signé par une clé inconnue".to_string());
    }
    Ok(signed)
}

/// Télécharge et vérifie `latest.json` (bloquant, hors du fil GTK).
pub fn fetch(url: &str) -> Result<SignedLatest, String> {
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    let response = agent
        .get(url)
        .set("User-Agent", &format!("secmet/{INSTALLED_VERSION}"))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("HTTP {code}"),
            ureq::Error::Transport(t) => match t.message() {
                Some(m) => format!("{} ({m})", t.kind()),
                None => t.kind().to_string(),
            },
        })?;
    let mut text = String::new();
    response
        .into_reader()
        .take(MAX_BODY)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    parse_and_verify(&text)
}

/// Vérification complète : télécharge, vérifie, met à jour et enregistre
/// l'état. Toujours un état (jamais de panique ni de blocage long).
pub fn check_now() -> UpdateState {
    let mut state = load_state();
    state.checked_at = Some(Utc::now());
    match fetch(&url()) {
        Ok(signed) => {
            state.moved_to = signed.payload.moved_to.clone();
            state.latest = Some(signed);
            state.error = None;
        }
        Err(e) => state.error = Some(e),
    }
    save_state(&state);
    state
}

/// Page de la version pour l'opérateur : dossier de `sha256sums_url`, et sur
/// GitHub `releases/download/<tag>` devient la page `releases/tag/<tag>`.
pub fn release_page_url(latest: &LatestRelease) -> String {
    let base = latest
        .sha256sums_url
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or(&latest.sha256sums_url);
    base.replacen("/releases/download/", "/releases/tag/", 1)
}

/// Notes de version dans `lang`, sinon français, anglais, puis n'importe
/// laquelle ; `None` si vides.
pub fn notes_for<'a>(latest: &'a LatestRelease, lang: &str) -> Option<&'a str> {
    [lang, "fr", "en"]
        .iter()
        .find_map(|l| latest.notes.get(*l))
        .or_else(|| latest.notes.values().next())
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use secmet_attest::sign::SigningIdentity;
    use std::collections::BTreeMap;

    fn latest(version: &str) -> LatestRelease {
        LatestRelease {
            version: version.to_string(),
            published_at: "2026-11-15".into(),
            notes: BTreeMap::from([("en".to_string(), "Fixes.".to_string())]),
            sha256sums_url:
                "https://github.com/hclcdev12/secmet-releases/releases/download/v9.9.9/SHA256SUMS"
                    .into(),
            moved_to: None,
        }
    }

    #[test]
    fn unknown_release_key_is_refused_even_if_valid() {
        let other = SigningIdentity::from_seed(&[3u8; 32]);
        let signed = release::sign_latest(&other, &latest("9.9.9")).unwrap();
        let text = serde_json::to_string(&signed).unwrap();
        assert_eq!(
            parse_and_verify(&text).unwrap_err(),
            "latest.json signé par une clé inconnue"
        );
        assert_eq!(parse_and_verify("{").unwrap_err(), "latest.json illisible");
        let mut tampered = signed;
        tampered.payload.version = "9.9.8".into();
        assert!(parse_and_verify(&serde_json::to_string(&tampered).unwrap()).is_err());
    }

    #[test]
    fn availability_follows_installed_version() {
        let id = SigningIdentity::from_seed(&[4u8; 32]);
        let newer = UpdateState {
            latest: Some(release::sign_latest(&id, &latest("9.9.9")).unwrap()),
            ..Default::default()
        };
        assert_eq!(newer.available().map(|l| l.version.as_str()), Some("9.9.9"));
        let same = UpdateState {
            latest: Some(release::sign_latest(&id, &latest(INSTALLED_VERSION)).unwrap()),
            ..Default::default()
        };
        assert!(same.available().is_none());
        assert!(UpdateState::default().available().is_none());
    }

    #[test]
    fn due_after_interval_or_clock_rollback() {
        let now = Utc::now();
        assert!(UpdateState::default().is_due(now));
        let recent = UpdateState {
            checked_at: Some(now - chrono::Duration::hours(1)),
            ..Default::default()
        };
        assert!(!recent.is_due(now));
        let old = UpdateState {
            checked_at: Some(now - chrono::Duration::hours(25)),
            ..Default::default()
        };
        assert!(old.is_due(now));
        let future = UpdateState {
            checked_at: Some(now + chrono::Duration::hours(1)),
            ..Default::default()
        };
        assert!(future.is_due(now));
    }

    #[test]
    fn release_page_and_notes() {
        let l = latest("9.9.9");
        assert_eq!(
            release_page_url(&l),
            "https://github.com/hclcdev12/secmet-releases/releases/tag/v9.9.9"
        );
        assert_eq!(notes_for(&l, "de"), Some("Fixes."));
        let mut empty = l.clone();
        empty.notes.clear();
        assert_eq!(notes_for(&empty, "fr"), None);
    }

    /// Réseau réel : `cargo test -p secmet -- --ignored network`. Tant
    /// qu'aucune release n'est publiée, le canal répond 404 ; ensuite, le
    /// fichier doit être signé par la clé intégrée.
    #[test]
    #[ignore]
    fn network_channel_answers() {
        match fetch(DEFAULT_URL) {
            Ok(signed) => assert!(release::parse_version(&signed.payload.version).is_some()),
            Err(e) => assert_eq!(e, "HTTP 404", "canal injoignable ou réponse inattendue"),
        }
    }

    #[test]
    fn state_round_trip() {
        let state = UpdateState {
            checked_at: Some(Utc::now()),
            error: Some("HTTP 404".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: UpdateState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
        let old: UpdateState = serde_json::from_str("{}").unwrap();
        assert_eq!(old, UpdateState::default());
    }
}
