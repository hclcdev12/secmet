//! Sessions : un jeton aléatoire à durée de vie limitée, lié à l'identité
//! complète d'un disque (numéro de périphérique, numéro de série, empreinte
//! firmware). Une commande d'effacement n'est acceptée que si le disque
//! ré-identifié à l'instant de l'exécution porte exactement cette identité.

use std::collections::HashMap;
use std::process::Child;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{IsolationError, Result};
use crate::polkit::{constant_time_eq, hex, random_bytes, unhex32};
use crate::runner::JobHandle;

/// Durée de vie par défaut d'un jeton.
pub const DEFAULT_TTL: Duration = Duration::from_secs(10 * 60);

/// Identité complète d'un disque, telle que liée à un jeton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskIdentity {
    /// Nom noyau (`sdb`, `nvme0n1`).
    pub name: String,
    /// Majeur.
    pub major: u32,
    /// Mineur.
    pub minor: u32,
    /// Numéro de série (firmware si inspecté, sinon sysfs).
    pub serial: String,
    /// SHA-256 hexadécimal de (modèle, S/N, firmware, WWN, taille, bloc).
    pub fingerprint: String,
}

impl DiskIdentity {
    /// `major:minor`.
    pub fn devnum(&self) -> String {
        format!("{}:{}", self.major, self.minor)
    }

    /// Compare champ à champ ; la première différence nomme le champ.
    pub fn check_against(&self, found: &DiskIdentity) -> std::result::Result<(), IsolationError> {
        let mismatch = |field: &'static str, expected: String, found: String| {
            IsolationError::IdentityMismatch {
                field,
                expected,
                found,
            }
        };
        if (self.major, self.minor) != (found.major, found.minor) {
            return Err(mismatch("devnum", self.devnum(), found.devnum()));
        }
        if self.serial != found.serial {
            return Err(mismatch(
                "serial",
                self.serial.clone(),
                found.serial.clone(),
            ));
        }
        if self.fingerprint != found.fingerprint {
            return Err(mismatch(
                "fingerprint",
                self.fingerprint.clone(),
                found.fingerprint.clone(),
            ));
        }
        Ok(())
    }
}

/// Jeton de session (32 octets), effacé à la libération.
#[derive(Clone)]
pub struct Token(Zeroizing<[u8; 32]>);

impl Token {
    /// Nouveau jeton aléatoire.
    pub fn random() -> Result<Self> {
        Ok(Self(Zeroizing::new(random_bytes()?)))
    }

    /// Hexadécimal à transmettre à l'UI.
    pub fn to_hex(&self) -> String {
        hex(&*self.0)
    }

    fn matches(&self, presented: &[u8; 32]) -> bool {
        constant_time_eq(&*self.0, presented)
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(…)")
    }
}

/// Une session émise.
#[derive(Debug)]
pub struct Session {
    token: Token,
    /// Disque lié.
    pub identity: DiskIdentity,
    /// Émission.
    pub issued: Instant,
    /// Expiration.
    pub expires: Instant,
    /// Job en cours ou terminé pour cette session.
    pub job: Option<Arc<JobHandle>>,
    /// Seed hexadécimal des canaris déjà écrits (`PrepareCanaries`).
    pub canaries_seed: Option<String>,
    /// Processus TestDisk en cours pour cette session (annulable).
    pub testdisk: Arc<Mutex<Option<Child>>>,
}

impl Session {
    /// Expirée à `now` ?
    pub fn expired(&self, now: Instant) -> bool {
        now >= self.expires
    }

    /// Secondes restantes à `now`.
    pub fn remaining(&self, now: Instant) -> u64 {
        self.expires.saturating_duration_since(now).as_secs()
    }
}

/// Job d'effacement non terminé, ou analyse TestDisk en cours.
fn session_busy(s: &Session) -> bool {
    s.job.as_ref().is_some_and(|j| !j.is_finished())
        || s.testdisk
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
}

/// Registre des sessions, protégé par un verrou.
#[derive(Debug)]
pub struct SessionStore {
    ttl: Duration,
    sessions: Mutex<HashMap<u64, Session>>,
    next_id: Mutex<u64>,
}

impl SessionStore {
    /// Registre avec la durée de vie donnée.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            sessions: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<u64, Session>> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Durée de vie.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Émet un jeton pour `identity` ; une session précédente du même disque
    /// sans job actif est remplacée.
    pub fn issue(&self, identity: DiskIdentity, now: Instant) -> Result<(u64, String)> {
        let token = Token::random()?;
        let hex = token.to_hex();
        let mut sessions = self.lock();
        sessions.retain(|_, s| {
            !(s.identity == identity && s.job.as_ref().map_or(true, |j| j.is_finished()))
        });
        let id = {
            let mut next = self.next_id.lock().unwrap_or_else(|e| e.into_inner());
            let id = *next;
            *next += 1;
            id
        };
        sessions.insert(
            id,
            Session {
                token,
                identity,
                issued: now,
                expires: now + self.ttl,
                job: None,
                canaries_seed: None,
                testdisk: Arc::new(Mutex::new(None)),
            },
        );
        Ok((id, hex))
    }

    /// Identifiant de la session portant `token`, ou l'erreur d'isolation.
    /// Un jeton expiré est retiré (sauf si un job tourne encore : il reste
    /// consultable et annulable jusqu'à la fin du job).
    pub fn resolve(&self, token: &str, now: Instant) -> std::result::Result<u64, IsolationError> {
        let presented = unhex32(token).ok_or(IsolationError::TokenUnknown)?;
        let mut sessions = self.lock();
        let found = sessions
            .iter()
            .find(|(_, s)| s.token.matches(&presented))
            .map(|(id, _)| *id)
            .ok_or(IsolationError::TokenUnknown)?;
        let session = sessions.get(&found).ok_or(IsolationError::TokenUnknown)?;
        let job_running = session_busy(session);
        if session.expired(now) && !job_running {
            sessions.remove(&found);
            return Err(IsolationError::TokenExpired);
        }
        Ok(found)
    }

    /// Jeton valide **et** non expiré, même avec un job : pour lancer un
    /// nouvel effacement.
    pub fn resolve_fresh(
        &self,
        token: &str,
        now: Instant,
    ) -> std::result::Result<u64, IsolationError> {
        let id = self.resolve(token, now)?;
        let sessions = self.lock();
        match sessions.get(&id) {
            Some(s) if s.expired(now) => Err(IsolationError::TokenExpired),
            Some(_) => Ok(id),
            None => Err(IsolationError::TokenUnknown),
        }
    }

    /// Lit une session sous verrou.
    pub fn with<T>(&self, id: u64, f: impl FnOnce(&mut Session) -> T) -> Option<T> {
        self.lock().get_mut(&id).map(f)
    }

    /// Retire les sessions expirées sans job actif.
    pub fn purge_expired(&self, now: Instant) -> usize {
        let mut sessions = self.lock();
        let before = sessions.len();
        sessions.retain(|_, s| !s.expired(now) || session_busy(s));
        before - sessions.len()
    }

    /// Un job (effacement) ou une analyse TestDisk est-il encore en cours
    /// dans une session ?
    pub fn any_job_running(&self) -> bool {
        self.lock().values().any(session_busy)
    }

    /// Le disque `major:minor` est-il tenu par un job ou une analyse
    /// TestDisk d'une session, quelle qu'elle soit ? Aucune commande
    /// firmware ne doit lui être envoyée pendant ce temps.
    pub fn busy_devnum(&self, major: u32, minor: u32) -> bool {
        self.lock()
            .values()
            .any(|s| s.identity.major == major && s.identity.minor == minor && session_busy(s))
    }

    /// Nombre de sessions vivantes.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Aucune session.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn identity(serial: &str) -> DiskIdentity {
        DiskIdentity {
            name: "sdz".into(),
            major: 8,
            minor: 240,
            serial: serial.into(),
            fingerprint: "f".repeat(64),
        }
    }

    #[test]
    fn a_running_job_is_visible_until_it_finishes() {
        let store = SessionStore::new(Duration::from_secs(60));
        let now = Instant::now();
        let (id, _) = store.issue(identity("S1"), now).unwrap();
        assert!(!store.any_job_running());
        let handle = Arc::new(JobHandle::new());
        store.with(id, |s| s.job = Some(Arc::clone(&handle)));
        assert!(store.any_job_running());
        // Un jeton expiré ne purge pas une session dont le job tourne.
        assert_eq!(store.purge_expired(now + Duration::from_secs(120)), 0);
        handle.finish(serde_json::json!({}));
        assert!(!store.any_job_running());
    }

    #[test]
    fn identity_check_names_the_first_differing_field() {
        let a = identity("S1");
        assert!(a.check_against(&a).is_ok());
        let other_dev = DiskIdentity {
            minor: 16,
            ..a.clone()
        };
        assert!(matches!(
            a.check_against(&other_dev),
            Err(IsolationError::IdentityMismatch {
                field: "devnum",
                ..
            })
        ));
        assert!(matches!(
            a.check_against(&identity("S2")),
            Err(IsolationError::IdentityMismatch {
                field: "serial",
                ..
            })
        ));
        let other_fw = DiskIdentity {
            fingerprint: "0".repeat(64),
            ..a.clone()
        };
        assert!(matches!(
            a.check_against(&other_fw),
            Err(IsolationError::IdentityMismatch {
                field: "fingerprint",
                ..
            })
        ));
        assert_eq!(a.devnum(), "8:240");
    }

    #[test]
    fn tokens_resolve_until_ttl_then_expire() {
        let store = SessionStore::new(Duration::from_secs(600));
        let t0 = Instant::now();
        let (id, token) = store.issue(identity("S1"), t0).unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(store.resolve(&token, t0).unwrap(), id);
        assert_eq!(
            store
                .resolve(&token, t0 + Duration::from_secs(599))
                .unwrap(),
            id
        );
        assert_eq!(
            store
                .resolve_fresh(&token, t0 + Duration::from_secs(599))
                .unwrap(),
            id
        );
        assert_eq!(
            store.resolve(&token, t0 + Duration::from_secs(600)),
            Err(IsolationError::TokenExpired)
        );
        // Purgé : désormais inconnu.
        assert_eq!(
            store.resolve(&token, t0 + Duration::from_secs(601)),
            Err(IsolationError::TokenUnknown)
        );
        assert!(store.is_empty());
    }

    #[test]
    fn unknown_or_malformed_tokens_are_rejected() {
        let store = SessionStore::new(DEFAULT_TTL);
        let now = Instant::now();
        let (_, token) = store.issue(identity("S1"), now).unwrap();
        assert_eq!(store.resolve("", now), Err(IsolationError::TokenUnknown));
        assert_eq!(
            store.resolve(&"0".repeat(64), now),
            Err(IsolationError::TokenUnknown)
        );
        let mut flipped = token.clone().into_bytes();
        flipped[10] = if flipped[10] == b'0' { b'1' } else { b'0' };
        assert_eq!(
            store.resolve(&String::from_utf8(flipped).unwrap(), now),
            Err(IsolationError::TokenUnknown)
        );
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn reissuing_for_the_same_disk_replaces_the_idle_session() {
        let store = SessionStore::new(DEFAULT_TTL);
        let now = Instant::now();
        let (_, first) = store.issue(identity("S1"), now).unwrap();
        let (_, second) = store.issue(identity("S1"), now).unwrap();
        assert_ne!(first, second);
        assert_eq!(store.len(), 1);
        assert_eq!(
            store.resolve(&first, now),
            Err(IsolationError::TokenUnknown)
        );
        assert!(store.resolve(&second, now).is_ok());
        store.issue(identity("S2"), now).unwrap();
        assert_eq!(store.len(), 2);
        assert_eq!(store.purge_expired(now + DEFAULT_TTL), 2);
    }

    #[test]
    fn a_running_job_keeps_an_expired_token_consultable_but_not_fresh() {
        let store = SessionStore::new(Duration::from_secs(1));
        let t0 = Instant::now();
        let (id, token) = store.issue(identity("S1"), t0).unwrap();
        let handle = Arc::new(JobHandle::new());
        store.with(id, |s| s.job = Some(Arc::clone(&handle)));
        let later = t0 + Duration::from_secs(5);
        assert_eq!(store.resolve(&token, later).unwrap(), id);
        assert_eq!(
            store.resolve_fresh(&token, later),
            Err(IsolationError::TokenExpired)
        );
        assert_eq!(store.purge_expired(later), 0);
        handle.finish(serde_json::json!({"outcome": "cancelled"}));
        assert_eq!(
            store.resolve(&token, later),
            Err(IsolationError::TokenExpired)
        );
    }
}
