//! Client du daemon privilégié `secmet-helper serve`.
//!
//! Un seul prompt Polkit : `pkexec secmet-helper serve` est lancé une fois ;
//! sa ligne `ready` livre le chemin du socket et le secret d'amorçage, puis
//! chaque connexion s'authentifie avec ce secret. Les appels courts passent
//! par une connexion de contrôle partagée ; les opérations longues
//! (effacement, TestDisk) ouvrent une connexion dédiée qui relaie les
//! événements de progression au fil de l'eau.

use std::fmt;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use secmet_helper::ipc::{self, Command, Event, Message, PolicyId, PreferencesWire, Request};
use serde::Deserialize;
use serde_json::Value;

pub use secmet_helper::hostinfo::HostInfo;
pub use secmet_helper::identity::{AtaSmart, AtaSmartAttribute, NvmeSmart, SmartSnapshot};
pub use secmet_helper::ipc::Progress;
pub use secmet_helper::runner::{JobReport, Outcome, StepReport};
pub use secmet_helper::testdisk::TestdiskRun;

/// Attente maximale de la ligne `ready` (le prompt Polkit est compris).
const READY_TIMEOUT: Duration = Duration::from_secs(300);
/// Délai de lecture sur la connexion de contrôle.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(120);

/// Erreur d'un appel au helper, avec le `kind` stable du protocole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallError {
    pub kind: String,
    pub message: String,
}

impl CallError {
    /// Panne de liaison côté client (`transport`) ou délai dépassé
    /// (`timeout`). Distinct du `io` du helper, qui désigne une erreur
    /// d'E/S sur ses propres fichiers et n'implique pas de reconnexion.
    fn io(e: std::io::Error) -> Self {
        let kind = match e.kind() {
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => "timeout",
            _ => "transport",
        };
        Self {
            kind: kind.into(),
            message: format!("Liaison avec le helper interrompue : {e}"),
        }
    }

    /// Trame illisible ou liaison rompue côté lecture.
    fn frame(e: ipc::FrameError) -> Self {
        match e {
            ipc::FrameError::Io(e) => Self::io(e),
            ipc::FrameError::Json(e) => {
                Self::protocol(format!("réponse du helper illisible : {e}"))
            }
            other => Self {
                kind: "transport".into(),
                message: format!("Liaison avec le helper interrompue : {other}"),
            },
        }
    }

    /// La liaison est rompue ou désynchronisée : la connexion est à jeter.
    pub fn is_transport(&self) -> bool {
        self.kind == "transport" || self.kind == "timeout"
    }

    fn protocol(message: impl Into<String>) -> Self {
        Self {
            kind: "protocol".into(),
            message: message.into(),
        }
    }

    pub fn is_token_expired(&self) -> bool {
        self.kind == "token_expired" || self.kind == "token_unknown"
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<CallError> for String {
    fn from(e: CallError) -> Self {
        e.message
    }
}

struct Connection {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl Connection {
    fn open(socket: &Path, bootstrap: &str, timeout: Option<Duration>) -> Result<Self, CallError> {
        let stream = UnixStream::connect(socket).map_err(CallError::io)?;
        stream.set_read_timeout(timeout).map_err(CallError::io)?;
        let reader = BufReader::new(stream.try_clone().map_err(CallError::io)?);
        let mut conn = Self {
            stream,
            reader,
            next_id: 1,
        };
        conn.request(Command::Authenticate {
            bootstrap: bootstrap.to_string(),
        })?;
        Ok(conn)
    }

    fn send(&mut self, cmd: Command) -> Result<u64, CallError> {
        let id = self.next_id;
        self.next_id += 1;
        ipc::write_line(&mut self.stream, &Request { id, cmd }).map_err(CallError::io)?;
        Ok(id)
    }

    fn read(&mut self) -> Result<Message, CallError> {
        ipc::read_message(&mut self.reader).map_err(CallError::frame)
    }

    fn request(&mut self, cmd: Command) -> Result<Value, CallError> {
        let id = self.send(cmd)?;
        loop {
            match self.read()? {
                Message::Ok { id: got, result } if got == id => return Ok(result),
                Message::Error { error, .. } => {
                    return Err(CallError {
                        kind: error.kind,
                        message: error.message,
                    })
                }
                _ => continue,
            }
        }
    }

    /// Requête dont la réponse peut être précédée (TestDisk) ou suivie
    /// (effacement : `accepted` puis événements jusqu'à `done`) d'événements.
    fn request_streaming(
        &mut self,
        cmd: Command,
        wait_done: bool,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Value, CallError> {
        let id = self.send(cmd)?;
        let mut result = None;
        let mut done = !wait_done;
        loop {
            match self.read()? {
                Message::Event { id: got, event } if got == id => {
                    if matches!(event, Event::Done(_)) {
                        done = true;
                    }
                    on_event(event);
                }
                Message::Ok { id: got, result: r } if got == id => result = Some(r),
                Message::Error { error, .. } => {
                    return Err(CallError {
                        kind: error.kind,
                        message: error.message,
                    })
                }
                _ => {}
            }
            if done {
                if let Some(r) = result {
                    return Ok(r);
                }
            }
        }
    }
}

/// Session helper : processus `pkexec secmet-helper serve` et son socket.
pub struct Helper {
    child: Mutex<Option<Child>>,
    socket: PathBuf,
    bootstrap: String,
    control: Mutex<Option<Connection>>,
    pub ttl_seconds: u64,
}

#[derive(Deserialize)]
struct Ready {
    socket: PathBuf,
    bootstrap: String,
    #[serde(default)]
    ttl_seconds: u64,
}

impl Helper {
    /// Lance le helper via pkexec (un prompt) et ouvre la connexion de contrôle.
    pub fn start() -> Result<Arc<Self>, String> {
        let helper = crate::paths::helper_path();
        let mut child =
            crate::pkexec_gate::pkexec_spawn(&[helper.as_str(), "serve"]).map_err(|e| {
                format!(
                    "Impossible de démarrer le helper privilégié (pkexec).\n\
                     Détail : {e}"
                )
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Helper : sortie standard manquante".to_string())?;
        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    eprintln!("[secmet-helper] {line}");
                }
            });
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let _ = tx.send(reader.read_line(&mut line).map(|_| line));
        });
        let line = match rx.recv_timeout(READY_TIMEOUT) {
            Ok(Ok(line)) => line,
            Ok(Err(e)) => {
                let _ = child.kill();
                return Err(format!("Helper : lecture impossible ({e})"));
            }
            Err(_) => {
                // Le plus souvent : fenêtre d'authentification laissée sans
                // réponse, ou refusée (pkexec sort en 126/127).
                let code = child.try_wait().ok().flatten().and_then(|s| s.code());
                let _ = child.kill();
                return Err(match code {
                    Some(126) | Some(127) => "Authentification annulée ou refusée.\n\
                         Validez le mot de passe administrateur puis réessayez."
                        .to_string(),
                    _ => "Helper : aucune réponse dans le délai imparti (5 min).\n\
                          La fenêtre d'authentification est restée sans réponse ? \
                          Relancez et validez le mot de passe."
                        .to_string(),
                });
            }
        };
        if line.trim().is_empty() {
            let code = child.wait().ok().and_then(|s| s.code());
            return Err(match code {
                Some(126) | Some(127) => "Authentification annulée ou refusée.\n\
                     Validez le mot de passe administrateur puis réessayez."
                    .to_string(),
                other => format!(
                    "Le helper s'est arrêté avant d'être prêt (code {other:?}).\n\
                     Vérifiez l'installation : make verify-polkit"
                ),
            });
        }
        let ready: Ready = serde_json::from_str(line.trim())
            .map_err(|e| format!("Helper : ligne de démarrage illisible ({e})"))?;
        let control = Connection::open(&ready.socket, &ready.bootstrap, Some(CONTROL_TIMEOUT))?;
        crate::pkexec_gate::mark_session_active();
        Ok(Arc::new(Self {
            child: Mutex::new(Some(child)),
            socket: ready.socket,
            bootstrap: ready.bootstrap,
            control: Mutex::new(Some(control)),
            ttl_seconds: ready.ttl_seconds,
        }))
    }

    /// Appel court sur la connexion de contrôle ; une connexion rompue est
    /// rouverte une fois.
    pub fn call(&self, cmd: Command) -> Result<Value, CallError> {
        let mut guard = self.control.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(Connection::open(
                &self.socket,
                &self.bootstrap,
                Some(CONTROL_TIMEOUT),
            )?);
        }
        let outcome = match guard.as_mut() {
            Some(conn) => conn.request(cmd.clone()),
            None => Err(CallError::protocol("connexion de contrôle absente")),
        };
        match outcome {
            // Liaison rompue ou réponse en retard : la connexion est jetée
            // (une réponse tardive désynchroniserait les identifiants).
            // Seules les commandes sans effet sont rejouées : un `Unlock`
            // rejoué consommerait une tentative ATA, un `Unfreeze` une
            // seconde mise en veille.
            Err(e) if e.is_transport() => {
                *guard = None;
                let idempotent = matches!(
                    cmd,
                    Command::ListDisks {} | Command::HostInfo {} | Command::GetStatus { .. }
                );
                if e.kind != "transport" || !idempotent {
                    return Err(e);
                }
                let mut fresh =
                    Connection::open(&self.socket, &self.bootstrap, Some(CONTROL_TIMEOUT))?;
                let result = fresh.request(cmd);
                *guard = Some(fresh);
                result
            }
            other => other,
        }
    }

    /// Opération longue sur une connexion dédiée, événements relayés.
    pub fn stream(
        &self,
        cmd: Command,
        wait_done: bool,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Value, CallError> {
        let mut conn = Connection::open(&self.socket, &self.bootstrap, None)?;
        conn.request_streaming(cmd, wait_done, on_event)
    }

    /// Demande l'arrêt du daemon et attend sa fin, sans jamais bloquer
    /// l'application : si un job est en cours, le daemon diffère son arrêt
    /// jusqu'à la fin du job et on le laisse vivre ; sinon on l'attend 5 s,
    /// on le tue, puis on l'abandonne au bout de 2 s de plus (un thread
    /// bloqué dans une commande firmware ne peut pas mourir).
    pub fn shutdown(&self) {
        let deferred = self
            .call(Command::Shutdown {})
            .ok()
            .and_then(|v| v.get("deferred").and_then(Value::as_bool))
            .unwrap_or(false);
        let child = self.child.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(mut child) = child else {
            return;
        };
        if deferred {
            eprintln!(
                "[secmet-helper] un job est en cours : le daemon s'arrêtera de lui-même à sa fin"
            );
            return;
        }
        for _ in 0..50 {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        for _ in 0..20 {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        eprintln!("[secmet-helper] le daemon ne répond plus et ne se termine pas : abandonné");
    }
}

// ── Vues typées des réponses ────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ProtectionView {
    #[serde(default)]
    pub hosts_system: bool,
    #[serde(default)]
    pub mounted: bool,
    #[serde(default)]
    pub swap: bool,
    #[serde(default)]
    pub held: bool,
    #[serde(default)]
    pub summary: String,
}

impl ProtectionView {
    pub fn any(&self) -> bool {
        self.hosts_system || self.mounted || self.swap || self.held
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct InventoryDisk {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub transport: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub size_label: String,
    #[serde(default)]
    pub rotational: Option<bool>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub removable: bool,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub serial: Option<String>,
    #[serde(default)]
    pub firmware: Option<String>,
    #[serde(default)]
    pub partitions: Vec<String>,
    #[serde(default)]
    pub protection: ProtectionView,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Inventory {
    #[serde(default)]
    pub disks: Vec<InventoryDisk>,
    #[serde(default)]
    pub root_traced: Option<bool>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct IdentityView {
    pub name: String,
    pub major: u32,
    pub minor: u32,
    pub serial: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AtaView {
    #[serde(default)]
    pub sanitize_supported: bool,
    #[serde(default)]
    pub sanitize_block_erase: bool,
    #[serde(default)]
    pub sanitize_crypto_scramble: bool,
    #[serde(default)]
    pub sanitize_overwrite: bool,
    #[serde(default)]
    pub sanitize_frozen: bool,
    #[serde(default)]
    pub security_supported: bool,
    #[serde(default)]
    pub security_enabled: bool,
    #[serde(default)]
    pub security_locked: bool,
    #[serde(default)]
    pub security_frozen: bool,
    #[serde(default)]
    pub enhanced_erase_supported: bool,
    #[serde(default)]
    pub security_erase_minutes: Option<u32>,
    #[serde(default)]
    pub enhanced_erase_minutes: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NvmeView {
    #[serde(default)]
    pub sanitize_block_erase: bool,
    #[serde(default)]
    pub sanitize_crypto_erase: bool,
    #[serde(default)]
    pub sanitize_overwrite: bool,
    #[serde(default)]
    pub format_supported: bool,
    #[serde(default)]
    pub format_crypto_erase: bool,
    #[serde(default)]
    pub format_erases_all_namespaces: bool,
    #[serde(default)]
    pub multiple_namespaces: bool,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HealthView {
    #[serde(default)]
    pub smart_failing: bool,
    #[serde(default)]
    pub reallocated_sectors: Option<u64>,
    #[serde(default)]
    pub pending_sectors: Option<u64>,
    #[serde(default)]
    pub nvme_percentage_used: Option<u8>,
    #[serde(default)]
    pub nvme_media_errors: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DiskView {
    pub name: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub firmware: String,
    #[serde(default)]
    pub serial: String,
    #[serde(default)]
    pub transport: String,
    #[serde(default)]
    pub media: String,
    #[serde(default)]
    pub block_size: u64,
    #[serde(default)]
    pub block_count: u64,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub size_label: String,
    #[serde(default)]
    pub partitions: Vec<String>,
    #[serde(default)]
    pub removable: bool,
    #[serde(default)]
    pub protection: ProtectionView,
    #[serde(default)]
    pub inspection_failed: bool,
    #[serde(default)]
    pub inspection_error: Option<String>,
    #[serde(default)]
    pub ata: Option<AtaView>,
    #[serde(default)]
    pub nvme: Option<NvmeView>,
    #[serde(default)]
    pub health: HealthView,
    #[serde(default)]
    pub smart: SmartSnapshot,
    #[serde(default)]
    pub rpm: Option<u16>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StepView {
    pub method: String,
    pub level: String,
    #[serde(default)]
    pub failure_criteria: Vec<String>,
    #[serde(default)]
    pub write_canaries: bool,
    #[serde(default)]
    pub require_no_signatures: bool,
    #[serde(default)]
    pub samples: u32,
    #[serde(default)]
    pub whole_controller: bool,
    #[serde(default)]
    pub estimated_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExcludedView {
    pub method: String,
    pub level: String,
    pub reason: String,
    pub guidance: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DegradationView {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub lost: Vec<String>,
    #[serde(default)]
    pub guidance: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlanView {
    pub policy: String,
    pub target_level: String,
    #[serde(default)]
    pub worst_level: String,
    pub primary: StepView,
    #[serde(default)]
    pub fallbacks: Vec<StepView>,
    #[serde(default)]
    pub excluded: Vec<ExcludedView>,
    #[serde(default)]
    pub degradation: Option<DegradationView>,
    #[serde(default)]
    pub recommend_destroy: Vec<String>,
    /// Limites connues du plan (`NvmeOtherNamespacesUntouched`…).
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Code de la limite « autres namespaces NVMe non couverts ».
pub const NOTE_NVME_OTHER_NAMESPACES: &str = "NvmeOtherNamespacesUntouched";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct VerdictView {
    pub state: String,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub guidance: Vec<String>,
    #[serde(default)]
    pub plan: Option<PlanView>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScanResult {
    pub token: String,
    #[serde(default)]
    pub expires_in: u64,
    pub identity: IdentityView,
    pub disk: DiskView,
    pub verdict: VerdictView,
    /// Levée automatique d'un mot de passe ATA SecMet, si elle a été tentée.
    #[serde(default)]
    pub recovery: Option<RecoveryView>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RecoveryView {
    pub status: String,
    #[serde(default)]
    pub detail: String,
}

impl RecoveryView {
    pub fn recovered(&self) -> bool {
        self.status == "recovered"
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Accepted {
    #[serde(default)]
    pub accepted: bool,
    pub plan: PlanView,
    #[serde(default)]
    pub seed: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct UnfreezeOutcome {
    pub was_frozen: bool,
    pub suspended: bool,
    pub still_frozen: bool,
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, CallError> {
    serde_json::from_value(value)
        .map_err(|e| CallError::protocol(format!("réponse du helper illisible : {e}")))
}

/// Préférences par défaut : passes HDD mémorisées, pas de firmware via USB.
pub fn default_preferences() -> PreferencesWire {
    PreferencesWire {
        host_passes: crate::settings::clamp_hdd_passes(crate::settings::load().hdd_passes),
        ..PreferencesWire::default()
    }
}

pub fn list_disks(helper: &Helper) -> Result<Inventory, CallError> {
    decode(helper.call(Command::ListDisks {})?)
}

pub fn host_info(helper: &Helper) -> Result<HostInfo, CallError> {
    decode(helper.call(Command::HostInfo {})?)
}

/// Identifie un disque et obtient un jeton de session frais.
pub fn scan_device(
    helper: &Helper,
    name: &str,
    preferences: PreferencesWire,
) -> Result<ScanResult, CallError> {
    decode(helper.call(Command::ScanDevice {
        device: name.to_string(),
        policy: PolicyId::default(),
        preferences,
    })?)
}

pub fn cancel(helper: &Helper, token: &str) -> Result<Value, CallError> {
    helper.call(Command::Cancel {
        token: token.to_string(),
    })
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnlockOutcome {
    #[serde(default)]
    pub unlocked: bool,
    #[serde(default)]
    pub security_enabled: bool,
    #[serde(default)]
    pub locked: bool,
}

/// Déverrouille le disque de la session avec un mot de passe connu, puis le lève.
pub fn unlock(
    helper: &Helper,
    token: &str,
    password: &str,
    master: bool,
) -> Result<UnlockOutcome, CallError> {
    decode(helper.call(Command::Unlock {
        token: token.to_string(),
        password: ipc::Secret::new(password),
        identifier: if master {
            ipc::PasswordIdentifier::Master
        } else {
            ipc::PasswordIdentifier::User
        },
    })?)
}

pub fn unfreeze(helper: &Helper, token: &str) -> Result<UnfreezeOutcome, CallError> {
    decode(helper.call(Command::Unfreeze {
        token: token.to_string(),
    })?)
}

/// Lance le plan et relaie les événements jusqu'au rapport final.
pub fn execute_plan(
    helper: &Helper,
    token: &str,
    confirm_serial: &str,
    preferences: PreferencesWire,
    mut on_event: impl FnMut(Event),
) -> Result<(Accepted, JobReport), CallError> {
    let mut report: Option<Value> = None;
    let accepted = helper.stream(
        Command::ExecutePlan {
            token: token.to_string(),
            confirm_serial: confirm_serial.to_string(),
            policy: PolicyId::default(),
            preferences,
        },
        true,
        &mut |event| {
            if let Event::Done(value) = &event {
                report = Some(value.clone());
            }
            on_event(event);
        },
    )?;
    let accepted: Accepted = decode(accepted)?;
    let report = report.ok_or_else(|| CallError::protocol("rapport final absent"))?;
    Ok((accepted, decode(report)?))
}

/// Analyse TestDisk, lignes relayées.
pub fn verify_testdisk(
    helper: &Helper,
    token: &str,
    mut on_line: impl FnMut(String),
) -> Result<TestdiskRun, CallError> {
    let value = helper.stream(
        Command::VerifyTestdisk {
            token: token.to_string(),
        },
        false,
        &mut |event| {
            if let Event::Log { message } = event {
                on_line(message);
            }
        },
    )?;
    decode(value)
}

// ── Session globale (un helper par vie de l'application) ────────────────

static CURRENT: Mutex<Option<Arc<Helper>>> = Mutex::new(None);
/// Tenu pendant un démarrage : un second `ensure` attend la fin du premier
/// (une seule invite Polkit) puis réutilise la session ouverte.
static START_LOCK: Mutex<()> = Mutex::new(());

pub fn current() -> Option<Arc<Helper>> {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn is_active() -> bool {
    current().is_some()
}

/// Helper existant, ou démarrage (un prompt Polkit). Bloque le temps de
/// l'invite : à appeler hors du fil GTK quand la session n'est pas ouverte.
pub fn ensure() -> Result<Arc<Helper>, String> {
    let _starting = START_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(h) = current() {
        return Ok(h);
    }
    let helper = Helper::start()?;
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&helper));
    Ok(helper)
}

/// Un démarrage du helper est en cours sur un autre fil (invite Polkit
/// affichée) : les actions qui exigeraient la session doivent attendre.
pub fn is_starting() -> bool {
    matches!(
        START_LOCK.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    )
}

/// Fin de l'application : arrêt du daemon.
pub fn shutdown_app_session() {
    let helper = CURRENT.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(helper) = helper {
        helper.shutdown();
    }
    crate::pkexec_gate::clear_session_active();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_decode_a_helper_scan_payload() {
        let value = serde_json::json!({
            "token": "ab".repeat(32),
            "expires_in": 600,
            "identity": {"name": "sdb", "major": 8, "minor": 16, "serial": "S1", "fingerprint": "f"},
            "disk": {
                "name": "sdb", "model": "M", "serial": "S1", "transport": "sata", "media": "ssd",
                "size_bytes": 1000, "size_label": "1 kB",
                "protection": {"hosts_system": false, "mounted": false, "swap": false, "held": false},
                "ata": {"security_frozen": true},
                "health": {"smart_failing": false, "nvme_media_errors": "0"},
                "smart": {"ata": {"passed": true, "attributes": [{"id": 5, "flags": 51, "value": 100, "worst": 100, "threshold": 10, "raw": 0}]}, "nvme": null},
                "rpm": 0
            },
            "verdict": {"state": "ready", "plan": {"policy": "nist-800-88r1", "target_level": "Purge",
                "primary": {"method": "ATA_SANITIZE_BLOCK_ERASE", "level": "Purge"}}}
        });
        let scan: ScanResult = decode(value).unwrap();
        assert_eq!(scan.identity.serial, "S1");
        assert!(scan.disk.ata.as_ref().unwrap().security_frozen);
        assert_eq!(
            scan.verdict.plan.as_ref().unwrap().primary.method,
            "ATA_SANITIZE_BLOCK_ERASE"
        );
        assert_eq!(
            scan.disk.smart.ata.as_ref().unwrap().attributes[0].threshold,
            Some(10)
        );
        assert_eq!(scan.disk.rpm, Some(0));
        let err = CallError {
            kind: "token_expired".into(),
            message: "x".into(),
        };
        assert!(err.is_token_expired());
        assert_eq!(String::from(err), "x");
    }
}
