//! Protocole NDJSON entre l'UI et le helper.
//!
//! Une ligne JSON par message, 1 Mio au plus (ligne plus longue → la
//! connexion est fermée). Les requêtes portent un `id` que le helper
//! recopie dans la réponse et dans chaque événement du job qu'elles ont
//! lancé. Tous les types sont `deny_unknown_fields` : un champ inconnu est
//! une erreur de protocole, jamais un comportement implicite.
//!
//! ```text
//! → {"id":1,"cmd":{"type":"Authenticate","bootstrap":"<hex>"}}
//! ← {"type":"ok","id":1,"result":{"uid":1000}}
//! → {"id":2,"cmd":{"type":"ScanDevice","device":"/dev/sdb"}}
//! ← {"type":"ok","id":2,"result":{"token":"…","identity":{…},"verdict":{…}}}
//! → {"id":3,"cmd":{"type":"ExecutePlan","token":"…","confirm_serial":"…"}}
//! ← {"type":"ok","id":3,"result":{"accepted":true,…}}
//! ← {"type":"event","id":3,"event":{"kind":"progress",…}}
//! ← {"type":"event","id":3,"event":{"kind":"done",…}}
//! ```

use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::error::{HelperError, Opener};

/// Taille maximale d'une ligne, séparateur compris.
pub const MAX_MESSAGE_BYTES: usize = 1 << 20;

/// Politique de classification demandée par l'UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyId {
    /// NIST SP 800-88 Rev. 1 (défaut).
    #[default]
    Nist80088r1,
    /// Lecture conservatrice Rev. 2 / IEEE 2883.
    Nist80088r2,
}

/// Préférences d'exécution (miroir sérialisable de `engine::Preferences`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PreferencesWire {
    /// Autoriser le repli vers une écriture hôte.
    pub allow_host_fallback: bool,
    /// Passes pseudo-aléatoires de l'écriture hôte (1 à 16).
    pub host_passes: u8,
    /// Passe finale de zéros.
    pub host_final_zero: bool,
    /// Commandes firmware à travers un pont USB.
    pub firmware_over_usb: bool,
    /// Préférer l'effacement cryptographique.
    pub prefer_cryptographic: bool,
}

impl Default for PreferencesWire {
    fn default() -> Self {
        let p = secmet_core::engine::Preferences::default();
        Self {
            allow_host_fallback: p.allow_host_fallback,
            host_passes: p.host_passes,
            host_final_zero: p.host_final_zero,
            firmware_over_usb: p.firmware_over_usb,
            prefer_cryptographic: p.prefer_cryptographic,
        }
    }
}

impl From<PreferencesWire> for secmet_core::engine::Preferences {
    fn from(w: PreferencesWire) -> Self {
        Self {
            allow_host_fallback: w.allow_host_fallback,
            host_passes: w.host_passes,
            host_final_zero: w.host_final_zero,
            firmware_over_usb: w.firmware_over_usb,
            prefer_cryptographic: w.prefer_cryptographic,
        }
    }
}

impl PolicyId {
    /// Politique du moteur.
    pub const fn policy(self) -> secmet_core::taxonomy::Policy {
        match self {
            Self::Nist80088r1 => secmet_core::taxonomy::Policy::nist_800_88_rev1(),
            Self::Nist80088r2 => secmet_core::taxonomy::Policy::nist_800_88_rev2(),
        }
    }
}

/// Commandes acceptées.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Command {
    /// Présente le jeton d'amorçage obtenu au lancement par `pkexec`.
    Authenticate {
        /// Hexadécimal du secret écrit par le helper sur sa sortie standard.
        bootstrap: String,
    },
    /// Identifie un disque, calcule le verdict du moteur, émet un jeton de session.
    ScanDevice {
        /// `/dev/sdb`, `nvme0n1`… (jamais une preuve d'identité).
        device: String,
        /// Politique de classification.
        #[serde(default)]
        policy: PolicyId,
        /// Préférences.
        #[serde(default)]
        preferences: PreferencesWire,
    },
    /// Écrit les canaris de la session sur l'échantillon et les relit.
    PrepareCanaries {
        /// Jeton de session.
        token: String,
    },
    /// Exécute le plan du moteur pour le disque de la session.
    ExecutePlan {
        /// Jeton de session.
        token: String,
        /// Numéro de série complet, retapé ou confirmé par l'opérateur.
        confirm_serial: String,
        /// Politique.
        #[serde(default)]
        policy: PolicyId,
        /// Préférences.
        #[serde(default)]
        preferences: PreferencesWire,
    },
    /// Demande l'arrêt du job de la session.
    Cancel {
        /// Jeton de session.
        token: String,
    },
    /// État du job de la session.
    GetStatus {
        /// Jeton de session.
        token: String,
    },
    /// Inventaire des disques (sysfs + protections), sans ouverture.
    ListDisks {},
    /// Inventaire du banc (DMI, CPU, mémoire).
    HostInfo {},
    /// Dé-frozen ATA par suspend-to-RAM, disque de la session ré-identifié après.
    Unfreeze {
        /// Jeton de session.
        token: String,
    },
    /// Déverrouille le disque de la session avec un mot de passe ATA connu
    /// de l'opérateur, puis lève ce mot de passe (SECURITY DISABLE PASSWORD).
    Unlock {
        /// Jeton de session.
        token: String,
        /// Mot de passe ASCII (1 à 32 caractères, convention hdparm).
        password: Secret,
        /// Mot de passe utilisateur (défaut) ou maître.
        #[serde(default)]
        identifier: PasswordIdentifier,
    },
    /// Analyse TestDisk (`analyze,search`) du disque de la session ; les
    /// lignes de sortie arrivent en événements `log` avant la réponse.
    VerifyTestdisk {
        /// Jeton de session.
        token: String,
    },
    /// Arrête le daemon (aucun job en cours requis).
    Shutdown {},
}

/// Mot de passe utilisateur ou maître (ATA Security).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PasswordIdentifier {
    /// Mot de passe utilisateur.
    #[default]
    User,
    /// Mot de passe maître.
    Master,
}

/// Secret transporté par le protocole : jamais affiché par `Debug`, effacé
/// de la mémoire à la destruction.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Enveloppe `value`.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Le secret en clair.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.0.zeroize();
    }
}

/// Requête de l'UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Identifiant choisi par l'UI, recopié dans les réponses.
    pub id: u64,
    /// Commande.
    pub cmd: Command,
}

/// Corps d'erreur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    /// Identifiant stable (`busy`, `identity_mismatch`, `openers_found`, `token_expired`…).
    pub kind: String,
    /// Message lisible.
    pub message: String,
    /// Processus tiers, pour `openers_found`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openers: Vec<Opener>,
}

impl From<&HelperError> for ErrorBody {
    fn from(error: &HelperError) -> Self {
        let openers = match error {
            HelperError::Isolation(crate::error::IsolationError::OpenersFound(o)) => o.clone(),
            _ => Vec::new(),
        };
        Self {
            kind: error.kind().to_string(),
            message: error.to_string(),
            openers,
        }
    }
}

/// Progression d'un job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    /// Index de l'étape dans le plan (0 = primaire).
    pub step: usize,
    /// Méthode de l'étape.
    pub method: String,
    /// Phase (`canaries`, `firmware`, `pass 1/2`, `zero`, `verify`, `signatures`).
    pub phase: String,
    /// Pourcentage de la phase.
    pub percent: u8,
    /// Octets traités dans la phase.
    pub bytes_done: u64,
    /// Octets à traiter dans la phase (0 si inconnu).
    pub bytes_total: u64,
    /// Débit instantané, octets par seconde.
    pub bandwidth_bps: u64,
    /// Estimation du reste, en secondes.
    pub eta_seconds: Option<u64>,
    /// La phase courante peut-elle s'interrompre sur `Cancel` ? Faux pendant
    /// une commande firmware bloquante (Format NVM, SECURITY ERASE UNIT).
    #[serde(default)]
    pub cancellable: bool,
}

/// Événements d'un job, dans l'ordre d'émission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    /// Progression.
    Progress(Progress),
    /// Une étape commence.
    StepStarted {
        /// Index.
        step: usize,
        /// Méthode.
        method: String,
        /// Niveau NIST visé.
        level: String,
    },
    /// Une étape s'achève.
    StepFinished {
        /// Index.
        step: usize,
        /// `success`, `failed`, `cancelled`.
        outcome: String,
        /// Critère violé, si échec.
        criterion: Option<String>,
        /// Détail.
        detail: String,
    },
    /// Journal libre.
    Log {
        /// Message.
        message: String,
    },
    /// Fin du job.
    Done(serde_json::Value),
}

/// Message du helper vers l'UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    /// Réponse positive.
    Ok {
        /// Identifiant de la requête.
        id: u64,
        /// Résultat.
        result: serde_json::Value,
    },
    /// Réponse négative.
    Error {
        /// Identifiant de la requête (0 si illisible).
        id: u64,
        /// Erreur.
        error: ErrorBody,
    },
    /// Événement d'un job.
    Event {
        /// Identifiant de la requête `ExecutePlan`.
        id: u64,
        /// Événement.
        event: Event,
    },
}

impl Message {
    /// Réponse d'erreur pour `id`.
    pub fn error(id: u64, error: &HelperError) -> Self {
        Self::Error {
            id,
            error: ErrorBody::from(error),
        }
    }
}

/// Défaut de trame.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// Plus de [`MAX_MESSAGE_BYTES`] sans fin de ligne.
    #[error("message de plus de {MAX_MESSAGE_BYTES} octets")]
    Oversized,
    /// Le pair a fermé proprement.
    #[error("connexion fermée")]
    Closed,
    /// E/S.
    #[error("socket: {0}")]
    Io(#[from] io::Error),
    /// JSON invalide ou champ inconnu.
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Lit une ligne bornée. La ligne rendue ne contient pas le `\n`.
pub fn read_line<R: BufRead>(reader: &mut R) -> std::result::Result<Vec<u8>, FrameError> {
    let mut line = Vec::new();
    let mut limited = <&mut R as std::io::Read>::take(reader, MAX_MESSAGE_BYTES as u64);
    let read = limited.read_until(b'\n', &mut line)?;
    if read == 0 {
        return Err(FrameError::Closed);
    }
    match line.pop() {
        Some(b'\n') => {}
        Some(last) => {
            // Pas de fin de ligne : soit la limite est atteinte, soit le pair
            // a fermé au milieu d'un message. Les deux ferment la connexion.
            line.push(last);
            return Err(if line.len() >= MAX_MESSAGE_BYTES {
                FrameError::Oversized
            } else {
                FrameError::Closed
            });
        }
        None => return Err(FrameError::Closed),
    }
    Ok(line)
}

/// Lit et décode une requête.
pub fn read_request<R: BufRead>(reader: &mut R) -> std::result::Result<Request, FrameError> {
    // Effacée après décodage : la trame peut porter un mot de passe (`Unlock`).
    let line = zeroize::Zeroizing::new(read_line(reader)?);
    Ok(serde_json::from_slice(&line)?)
}

/// Lit et décode un message du helper (côté client et tests).
pub fn read_message<R: BufRead>(reader: &mut R) -> std::result::Result<Message, FrameError> {
    let line = read_line(reader)?;
    Ok(serde_json::from_slice(&line)?)
}

/// Sérialise `value` sur une ligne et vide le tampon.
pub fn write_line<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    let mut encoded = serde_json::to_vec(value).map_err(io::Error::other)?;
    if encoded.len() >= MAX_MESSAGE_BYTES {
        return Err(io::Error::other("message sortant au-delà de 1 Mio"));
    }
    encoded.push(b'\n');
    writer.write_all(&encoded)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::io::Cursor;

    #[test]
    fn every_command_round_trips_and_unknown_fields_are_refused() {
        let commands = [
            Command::Authenticate {
                bootstrap: "ab".into(),
            },
            Command::ScanDevice {
                device: "/dev/sdb".into(),
                policy: PolicyId::Nist80088r2,
                preferences: PreferencesWire::default(),
            },
            Command::PrepareCanaries { token: "t".into() },
            Command::ExecutePlan {
                token: "t".into(),
                confirm_serial: "S".into(),
                policy: PolicyId::default(),
                preferences: PreferencesWire {
                    host_passes: 3,
                    ..PreferencesWire::default()
                },
            },
            Command::Cancel { token: "t".into() },
            Command::GetStatus { token: "t".into() },
            Command::ListDisks {},
            Command::HostInfo {},
            Command::Unfreeze { token: "t".into() },
            Command::Unlock {
                token: "t".into(),
                password: Secret::new("hunter2"),
                identifier: PasswordIdentifier::Master,
            },
            Command::VerifyTestdisk { token: "t".into() },
            Command::Shutdown {},
        ];
        for (i, cmd) in commands.into_iter().enumerate() {
            let request = Request { id: i as u64, cmd };
            let text = serde_json::to_string(&request).unwrap();
            let back: Request = serde_json::from_str(&text).unwrap();
            assert_eq!(back, request, "{text}");
        }
        for bad in [
            r#"{"id":1,"cmd":{"type":"Cancel","token":"t","extra":1}}"#,
            r#"{"id":1,"cmd":{"type":"Authenticate","bootstrap":"x"},"extra":1}"#,
            r#"{"id":1,"cmd":{"type":"Nope"}}"#,
            r#"{"id":1,"cmd":{"type":"ScanDevice","device":"/dev/sdb","preferences":{"passes":1}}}"#,
            r#"{"id":1,"cmd":{"type":"ScanDevice","device":"/dev/sdb","policy":"dod"}}"#,
            r#"{"cmd":{"type":"Shutdown"}}"#,
        ] {
            assert!(serde_json::from_str::<Request>(bad).is_err(), "{bad}");
        }
        // Le secret ne fuit pas par Debug ; l'identifiant vaut `user` par défaut.
        let unlock: Request =
            serde_json::from_str(r#"{"id":2,"cmd":{"type":"Unlock","token":"t","password":"pw"}}"#)
                .unwrap();
        assert!(!format!("{unlock:?}").contains("pw"));
        assert!(matches!(
            unlock.cmd,
            Command::Unlock {
                identifier: PasswordIdentifier::User,
                ..
            }
        ));
        // Les champs optionnels prennent leur défaut.
        let minimal: Request =
            serde_json::from_str(r#"{"id":7,"cmd":{"type":"ScanDevice","device":"sdb"}}"#).unwrap();
        assert!(matches!(
            minimal.cmd,
            Command::ScanDevice {
                policy: PolicyId::Nist80088r1,
                ..
            }
        ));
    }

    #[test]
    fn framing_enforces_the_one_mebibyte_limit() {
        let mut ok = Cursor::new(b"{\"a\":1}\n".to_vec());
        assert_eq!(read_line(&mut ok).unwrap(), b"{\"a\":1}");
        assert!(matches!(read_line(&mut ok), Err(FrameError::Closed)));

        let mut big = vec![b'x'; MAX_MESSAGE_BYTES];
        big.push(b'\n');
        assert!(matches!(
            read_line(&mut Cursor::new(big)),
            Err(FrameError::Oversized)
        ));
        let mut just_under = vec![b'y'; MAX_MESSAGE_BYTES - 1];
        just_under.push(b'\n');
        assert_eq!(
            read_line(&mut Cursor::new(just_under)).unwrap().len(),
            MAX_MESSAGE_BYTES - 1
        );
        assert!(matches!(
            read_line(&mut Cursor::new(b"partial".to_vec())),
            Err(FrameError::Closed)
        ));
        let mut empty = Cursor::new(b"\n".to_vec());
        assert_eq!(read_line(&mut empty).unwrap(), b"");
    }

    #[test]
    fn messages_and_events_serialize_with_stable_tags() {
        let mut out = Vec::new();
        write_line(
            &mut out,
            &Message::Event {
                id: 3,
                event: Event::Progress(Progress {
                    step: 0,
                    method: "HOST_OVERWRITE_1_ZERO".into(),
                    phase: "pass 1/1".into(),
                    percent: 42,
                    bytes_done: 1,
                    bytes_total: 2,
                    bandwidth_bps: 3,
                    eta_seconds: Some(4),
                    cancellable: true,
                }),
            },
        )
        .unwrap();
        let text = String::from_utf8(out.clone()).unwrap();
        assert!(text.contains("\"type\":\"event\"") && text.contains("\"kind\":\"progress\""));
        assert!(text.ends_with('\n'));
        let back = read_message(&mut Cursor::new(out)).unwrap();
        assert!(matches!(back, Message::Event { id: 3, .. }));

        let error = Message::error(
            9,
            &HelperError::Isolation(crate::error::IsolationError::OpenersFound(vec![Opener {
                pid: 1,
                comm: "dd".into(),
                fd: 4,
                target: "/dev/sdz".into(),
            }])),
        );
        let text = serde_json::to_string(&error).unwrap();
        assert!(text.contains("\"kind\":\"openers_found\"") && text.contains("\"pid\":1"));
        let plain = serde_json::to_string(&Message::error(1, &HelperError::NoJob)).unwrap();
        assert!(!plain.contains("openers"));
    }

    #[test]
    fn preferences_and_policy_map_to_the_engine() {
        let wire = PreferencesWire {
            host_passes: 2,
            allow_host_fallback: false,
            ..PreferencesWire::default()
        };
        let engine: secmet_core::engine::Preferences = wire.into();
        assert_eq!(engine.host_passes, 2);
        assert!(!engine.allow_host_fallback);
        assert_eq!(PolicyId::Nist80088r2.policy().id, "nist-800-88r2");
        assert_eq!(
            serde_json::to_string(&PolicyId::Nist80088r1).unwrap(),
            "\"nist80088r1\""
        );
    }
}
