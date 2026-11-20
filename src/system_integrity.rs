//! Intégrité du système de la clé USB (Annexe M §M.3). Au démarrage de la
//! clé, `secmet-image-check` vérifie les empreintes signées du système
//! (`SYSTEM.sums`, clé de version) et écrit son rapport dans
//! [`STATUS_FILE`]. Ce module le lit : réserve sur les certificats si le
//! système n'est pas vérifié ou s'il a été altéré ([`caveat_key`]),
//! bandeau dans la fenêtre si altéré. L'effacement n'est jamais bloqué
//! (une version modifiée de SecMet doit fonctionner ; ses certificats le
//! disent).
//!
//! Limite : le vérificateur fait partie du système qu'il vérifie ; il
//! détecte la corruption et l'altération naïve, pas un attaquant ciblé.

use std::path::Path;

use secmet_attest::system_image::{IntegrityReport, IntegrityStatus};

/// Rapport écrit au démarrage par `secmet-image-check`.
pub const STATUS_FILE: &str = "/run/secmet-image/system-integrity.json";
/// Présent sur un système démarré par live-boot (la clé).
const LIVE_MARKER: &str = "/run/live";

/// Réserve : image altérée.
pub const CAVEAT_ALTERED: &str = "système de la clé USB altéré : empreintes ou signature différentes de l'image publiée (contrôle au démarrage)";
/// Réserve : image sans empreintes signées.
pub const CAVEAT_UNSIGNED: &str =
    "intégrité du système de la clé USB non vérifiée : image sans empreintes signées";
/// Réserve : contrôle non terminé ou impossible.
pub const CAVEAT_UNCHECKED: &str =
    "intégrité du système de la clé USB non vérifiée : contrôle au démarrage non terminé ou impossible";

/// Rapport lu dans `path` ; `None` s'il manque ou est illisible.
pub fn read_report(path: &Path) -> Option<IntegrityReport> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Réserve (clé de traduction) pour un rapport : `live` dit si le système
/// a démarré depuis une clé (sans rapport, une clé n'a pas été vérifiée ;
/// un banc installé qui emploie un dossier de données dédié n'a rien à
/// vérifier). Pur.
pub fn caveat_for(report: Option<&IntegrityReport>, live: bool) -> Option<&'static str> {
    match report.map(|r| r.status) {
        Some(IntegrityStatus::Ok) => None,
        Some(IntegrityStatus::Altered) => Some(CAVEAT_ALTERED),
        Some(IntegrityStatus::Unsigned) => Some(CAVEAT_UNSIGNED),
        Some(IntegrityStatus::Unavailable | IntegrityStatus::Error | IntegrityStatus::Pending) => {
            Some(CAVEAT_UNCHECKED)
        }
        None if live => Some(CAVEAT_UNCHECKED),
        None => None,
    }
}

/// Réserve à porter sur les certificats, lue à chaque certificat (le
/// contrôle tourne en tâche de fond au démarrage) ; `None` hors clé USB ou
/// si le système est intact.
pub fn caveat_key() -> Option<&'static str> {
    if !crate::paths::dedicated_data_dir() {
        return None;
    }
    caveat_for(
        read_report(Path::new(STATUS_FILE)).as_ref(),
        Path::new(LIVE_MARKER).is_dir(),
    )
}

/// Rapport courant si le système de la clé est altéré (bandeau) ; `None`
/// sinon, hors clé USB compris.
pub fn altered_report() -> Option<IntegrityReport> {
    if !crate::paths::dedicated_data_dir() {
        return None;
    }
    read_report(Path::new(STATUS_FILE)).filter(|r| r.status == IntegrityStatus::Altered)
}

/// Vrai quand le contrôle du démarrage a rendu son verdict (rapport présent
/// et pas « en cours ») ou qu'il n'y a rien à attendre (hors clé).
pub fn settled() -> bool {
    if !crate::paths::dedicated_data_dir() || !Path::new(LIVE_MARKER).is_dir() {
        return true;
    }
    read_report(Path::new(STATUS_FILE)).is_some_and(|r| r.status != IntegrityStatus::Pending)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::i18n::{tr_in, Lang};

    fn report(status: IntegrityStatus) -> IntegrityReport {
        serde_json::from_value(serde_json::json!({ "status": status })).unwrap()
    }

    #[test]
    fn caveat_follows_the_boot_check() {
        assert_eq!(caveat_for(Some(&report(IntegrityStatus::Ok)), true), None);
        assert_eq!(
            caveat_for(Some(&report(IntegrityStatus::Altered)), true),
            Some(CAVEAT_ALTERED)
        );
        assert_eq!(
            caveat_for(Some(&report(IntegrityStatus::Unsigned)), true),
            Some(CAVEAT_UNSIGNED)
        );
        for s in [
            IntegrityStatus::Unavailable,
            IntegrityStatus::Error,
            IntegrityStatus::Pending,
        ] {
            assert_eq!(caveat_for(Some(&report(s)), true), Some(CAVEAT_UNCHECKED));
        }
        // Clé sans rapport (service absent) : non vérifiée ; banc installé
        // avec un dossier de données dédié : rien à dire.
        assert_eq!(caveat_for(None, true), Some(CAVEAT_UNCHECKED));
        assert_eq!(caveat_for(None, false), None);
    }

    #[test]
    fn report_file_is_read_tolerantly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("system-integrity.json");
        assert!(read_report(&path).is_none());
        std::fs::write(&path, "{ pas du json").unwrap();
        assert!(read_report(&path).is_none());
        // Rapport écrit par le script shell (unavailable, error, pending).
        std::fs::write(
            &path,
            "{\n  \"status\": \"unavailable\",\n  \"problems\": [\"partition SECMET-SYS introuvable\"]\n}\n",
        )
        .unwrap();
        let r = read_report(&path).unwrap();
        assert_eq!(r.status, IntegrityStatus::Unavailable);
        assert_eq!(r.problems.len(), 1);
    }

    #[test]
    fn caveats_are_translated() {
        for key in [CAVEAT_ALTERED, CAVEAT_UNSIGNED, CAVEAT_UNCHECKED] {
            assert_eq!(tr_in(Lang::Fr, key), key);
            assert_ne!(tr_in(Lang::En, key), key, "{key}");
            assert_ne!(tr_in(Lang::De, key), key, "{key}");
        }
    }
}
