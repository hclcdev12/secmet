//! Horloge du poste (Annexe M, §M.7). Sur un laptop de récupération, la
//! pile du BIOS est souvent épuisée (date en 2000) ou l'horloge réglée en
//! heure locale par Windows ; les dates des certificats et l'ordre du lot
//! en dépendent. Ce module est la seule source d'heure de l'application :
//! [`now`] rend l'heure du poste, décalée si l'opérateur a saisi la date
//! réelle, sans toucher à l'horloge du système.
//!
//! Le contrôle ([`run_check`]) ne tourne qu'avec un dossier de données
//! dédié (clé USB) : une horloge synchronisée par le réseau est acceptée ;
//! sinon la date doit être postérieure aux références connues (construction
//! du logiciel, dernière entrée de lot). Le statut se retrouve en réserve
//! sur les certificats ([`caveat_key`]).

use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};

/// Format des horodatages de l'application.
pub const STAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
/// Format de saisie par l'opérateur.
pub const INPUT_FORMAT: &str = "%Y-%m-%d %H:%M";
/// Fichier créé par `systemd-timesyncd` quand l'horloge est synchronisée.
const TIMESYNC_FLAG: &str = "/run/systemd/timesync/synchronized";

static OFFSET_SECS: AtomicI64 = AtomicI64::new(0);
static STATUS: Mutex<ClockStatus> = Mutex::new(ClockStatus::NotChecked);

/// Diagnostic de l'horloge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockStatus {
    /// Pas de contrôle (banc installé) : l'heure du poste fait foi.
    NotChecked,
    /// Synchronisée par le réseau.
    Synced,
    /// Pas de source réseau, mais rien ne la contredit.
    Unverified,
    /// Antérieure à une référence connue ; décision de l'opérateur attendue.
    Suspect {
        /// Référence dépassée.
        floor: DateTime<Local>,
        /// Nom de la référence (clé de traduction).
        reason: String,
    },
    /// Douteuse, conservée sur confirmation de l'opérateur.
    SuspectConfirmed,
    /// Date saisie par l'opérateur, appliquée par décalage.
    Manual,
}

/// Heure courante de l'application.
pub fn now() -> DateTime<Local> {
    Local::now() + chrono::Duration::seconds(OFFSET_SECS.load(Ordering::Relaxed))
}

/// Horodatage `AAAA-MM-JJ HH:MM:SS`.
pub fn stamp() -> String {
    now().format(STAMP_FORMAT).to_string()
}

/// Statut courant.
pub fn status() -> ClockStatus {
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn set_status(status: ClockStatus) {
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = status;
}

/// Applique la date saisie par l'opérateur : toutes les heures de
/// l'application sont décalées d'autant, l'horloge du poste est intacte.
pub fn set_manual(target: DateTime<Local>) {
    let offset = (target - Local::now()).num_seconds();
    OFFSET_SECS.store(offset, Ordering::Relaxed);
    set_status(ClockStatus::Manual);
}

/// L'opérateur garde une horloge douteuse.
pub fn confirm_suspect() {
    set_status(ClockStatus::SuspectConfirmed);
}

/// Horloge synchronisée par le réseau (`systemd-timesyncd`).
pub fn ntp_synchronized() -> bool {
    Path::new(TIMESYNC_FLAG).exists()
}

/// Analyse un horodatage de l'application.
pub fn parse_stamp(text: &str) -> Option<DateTime<Local>> {
    let naive = NaiveDateTime::parse_from_str(text.trim(), STAMP_FORMAT).ok()?;
    Local.from_local_datetime(&naive).single()
}

/// Analyse une saisie `AAAA-MM-JJ HH:MM` (secondes tolérées).
pub fn parse_input(text: &str) -> Option<DateTime<Local>> {
    let text = text.trim();
    NaiveDateTime::parse_from_str(text, INPUT_FORMAT)
        .or_else(|_| NaiveDateTime::parse_from_str(text, STAMP_FORMAT))
        .ok()
        .and_then(|naive| Local.from_local_datetime(&naive).single())
}

/// Date de construction du logiciel : date de modification de l'exécutable
/// (l'AppImage ou le binaire ne peuvent pas précéder leur construction).
pub fn build_floor() -> Option<DateTime<Local>> {
    let exe = std::env::current_exe().ok()?;
    let modified = std::fs::metadata(exe).ok()?.modified().ok()?;
    Some(DateTime::<Local>::from(modified))
}

/// Verdict pur : `synced` l'emporte ; sinon la plus haute référence
/// dépassée rend l'horloge suspecte.
pub fn check(
    now: DateTime<Local>,
    synced: bool,
    floors: &[(DateTime<Local>, &str)],
) -> ClockStatus {
    if synced {
        return ClockStatus::Synced;
    }
    match floors
        .iter()
        .filter(|(floor, _)| now < *floor)
        .max_by_key(|(floor, _)| *floor)
    {
        Some((floor, reason)) => ClockStatus::Suspect {
            floor: *floor,
            reason: (*reason).to_string(),
        },
        None => ClockStatus::Unverified,
    }
}

/// Contrôle au démarrage : références = construction du logiciel et
/// `extra` (dernière entrée de lot, dernier lot clôturé). Enregistre et
/// rend le statut.
pub fn run_check(extra: &[(DateTime<Local>, &str)]) -> ClockStatus {
    let mut floors: Vec<(DateTime<Local>, &str)> = extra.to_vec();
    if let Some(build) = build_floor() {
        floors.push((build, "construction du logiciel"));
    }
    let status = check(Local::now(), ntp_synchronized(), &floors);
    set_status(status.clone());
    status
}

/// Réserve à porter sur les certificats (clé de traduction), selon le
/// statut ; `None` si l'horloge est fiable ou non contrôlée.
pub fn caveat_key() -> Option<&'static str> {
    match status() {
        ClockStatus::NotChecked | ClockStatus::Synced => None,
        ClockStatus::Unverified | ClockStatus::Suspect { .. } => Some(
            "horloge du poste non synchronisée (aucune source de temps réseau) : dates fournies \
             par le poste",
        ),
        ClockStatus::SuspectConfirmed => Some(
            "horloge du poste douteuse (antérieure à une référence connue), conservée sur \
             confirmation de l'opérateur",
        ),
        ClockStatus::Manual => {
            Some("date et heure saisies par l'opérateur : horloge du poste non fiable")
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn at(text: &str) -> DateTime<Local> {
        parse_stamp(text).unwrap()
    }

    #[test]
    fn verdicts() {
        let now = at("2026-10-30 12:00:00");
        let build = (at("2026-10-01 00:00:00"), "construction du logiciel");
        let lot = (at("2026-10-29 18:00:00"), "dernière entrée du lot");
        assert_eq!(check(now, true, &[build, lot]), ClockStatus::Synced);
        assert_eq!(check(now, false, &[build, lot]), ClockStatus::Unverified);
        // Pile du BIOS morte : 2000. La référence la plus haute est citée.
        let dead = at("2000-01-01 00:00:00");
        match check(dead, false, &[build, lot]) {
            ClockStatus::Suspect { floor, reason } => {
                assert_eq!(floor, lot.0);
                assert_eq!(reason, "dernière entrée du lot");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(check(dead, false, &[]), ClockStatus::Unverified);
    }

    #[test]
    fn manual_date_shifts_now_without_touching_the_system_clock() {
        let target = Local::now() + chrono::Duration::days(3);
        set_manual(target);
        let delta = (now() - target).num_seconds().abs();
        assert!(delta <= 2, "décalage appliqué : {delta}s");
        assert_eq!(status(), ClockStatus::Manual);
        assert!(caveat_key().unwrap().contains("saisies par l'opérateur"));
        OFFSET_SECS.store(0, Ordering::Relaxed);
        set_status(ClockStatus::NotChecked);
        assert!(caveat_key().is_none());
    }

    #[test]
    fn input_parsing() {
        assert!(parse_input("2026-10-30 09:15").is_some());
        assert!(parse_input(" 2026-10-30 09:15:30 ").is_some());
        assert!(parse_input("30/10/2026 09:15").is_none());
        assert!(parse_stamp("2026-02-30 00:00:00").is_none());
    }
}
