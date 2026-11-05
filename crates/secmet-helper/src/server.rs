//! Daemon : socket Unix, connexions, dispatch des commandes, jobs.
//!
//! Chaque connexion a son fil de lecture ; les écritures passent par un
//! verrou sur le flux, partagé avec le fil qui relaie les événements du job.
//! Un job survit à la déconnexion de l'UI (un effacement interrompu est
//! pire qu'un effacement non suivi) : l'UI se reconnecte, s'authentifie et
//! interroge `GetStatus` avec son jeton.

use std::io::{BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use secmet_core::canary::{self, SessionSeed};
use secmet_core::engine::{self, ErasePlan, Verdict};
use secmet_core::taxonomy::TargetCapabilities;
use secmet_device::ata::{security, AtaDevice};
use secmet_device::block::OpenMode;
use secmet_device::defrozen::{self, DefrozenOptions};
use secmet_device::protection::{self, DevNum};
use secmet_device::sysfs::{self, SysfsPaths};
use secmet_device::DeviceError;
use serde_json::{json, Value};

use crate::disk_target::DiskTarget;
use crate::error::{HelperError, IsolationError, Result};
use crate::hostinfo;
use crate::identity::{self, Scan};
use crate::ipc::{self, Command, Event, FrameError, Message, PolicyId, PreferencesWire, Request};
use crate::isolation;
use crate::polkit::{self, Bootstrap};
use crate::recovery;
use crate::runner::{self, JobHandle};
use crate::session::{DiskIdentity, SessionStore, DEFAULT_TTL};
use crate::testdisk;
use crate::vault::PasswordVault;

/// Nom du socket dans le répertoire.
pub const SOCKET_NAME: &str = "helper.sock";
/// Répertoire par défaut.
pub const DEFAULT_SOCKET_DIR: &str = "/run/secmet";

/// Configuration du daemon.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Chemin du socket (le répertoire est déjà préparé).
    pub socket_path: PathBuf,
    /// Uid autorisé à se connecter (en plus de root).
    pub allowed_uid: u32,
    /// Secret d'amorçage.
    pub bootstrap: Bootstrap,
    /// Durée de vie des jetons.
    pub ttl: Duration,
    /// Racines sysfs/proc/dev.
    pub paths: SysfsPaths,
    /// Coffre des mots de passe ATA.
    pub vault_dir: PathBuf,
}

/// Le daemon.
#[derive(Debug)]
pub struct Server {
    config: ServerConfig,
    sessions: SessionStore,
    shutdown: AtomicBool,
    /// Connexions acceptées et encore ouvertes.
    connections: AtomicUsize,
    /// Dernière fermeture de connexion (ou démarrage) : sert à l'arrêt
    /// automatique d'un daemon orphelin.
    last_activity: Mutex<Instant>,
    /// Inode du fichier socket créé par `bind` : seul ce fichier-là est
    /// retiré à l'arrêt.
    socket_inode: Mutex<Option<u64>>,
}

/// Connexions simultanées acceptées (l'UI en tient trois ou quatre).
const MAX_CONNECTIONS: usize = 32;
/// Délai de lecture accordé à une connexion non encore authentifiée.
const UNAUTHENTICATED_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Délai d'écriture : une UI qui ne lit plus ne bloque pas le daemon.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// Sans aucune connexion ni job pendant ce délai, le daemon s'arrête de
/// lui-même (UI disparue sans `Shutdown`, par exemple sur un abort).
const IDLE_EXIT: Duration = Duration::from_secs(600);
/// Pas de la boucle d'acceptation.
const ACCEPT_POLL: Duration = Duration::from_millis(200);

/// Décrémente le compteur de connexions à la fermeture, quoi qu'il arrive.
struct ConnectionGuard<'a>(&'a Server);

impl Drop for ConnectionGuard<'_> {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::SeqCst);
        *self
            .0
            .last_activity
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }
}

type Writer = Arc<Mutex<UnixStream>>;

struct Connection {
    authenticated: bool,
    peer: polkit::PeerCredentials,
    writer: Writer,
}

fn send(writer: &Writer, message: &Message) -> std::io::Result<()> {
    let mut stream = writer.lock().unwrap_or_else(|e| e.into_inner());
    ipc::write_line(&mut *stream, message)
}

impl Server {
    /// Coffre ATA : sur la clé SecMet quand le système a démarré dessus et
    /// que sa partition de données est montée (résolu à chaque usage), sinon
    /// le dossier configuré. Un `--vault-dir` explicite (tests) est respecté.
    fn vault_dir(&self) -> PathBuf {
        if self.config.vault_dir == Path::new(crate::vault::DEFAULT_DIR) {
            crate::vault::resolve_dir(&self.config.vault_dir)
        } else {
            self.config.vault_dir.clone()
        }
    }

    /// Nouveau daemon.
    pub fn new(config: ServerConfig) -> Arc<Self> {
        Arc::new(Self {
            sessions: SessionStore::new(config.ttl),
            config,
            shutdown: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
            last_activity: Mutex::new(Instant::now()),
            socket_inode: Mutex::new(None),
        })
    }

    /// Chemin du socket.
    pub fn socket_path(&self) -> &PathBuf {
        &self.config.socket_path
    }

    /// Lie le socket et le restreint à `allowed_uid`.
    pub fn bind(&self) -> Result<UnixListener> {
        let listener = UnixListener::bind(&self.config.socket_path).map_err(|e| {
            HelperError::io(format!("bind {}", self.config.socket_path.display()), e)
        })?;
        polkit::restrict_socket(&self.config.socket_path, self.config.allowed_uid)?;
        *self.socket_inode.lock().unwrap_or_else(|e| e.into_inner()) =
            polkit::inode_of_path(&self.config.socket_path);
        listener
            .set_nonblocking(true)
            .map_err(|e| HelperError::io("set_nonblocking", e))?;
        Ok(listener)
    }

    /// Demande l'arrêt de la boucle d'acceptation.
    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Arrêt demandé et aucun job en cours : la boucle peut s'arrêter. Un
    /// job (commande firmware bloquante comprise) va toujours à son terme,
    /// sinon ses étapes finales (levée du mot de passe ATA, validation,
    /// rapport) seraient perdues.
    fn should_stop(&self) -> bool {
        if self.sessions.any_job_running() {
            return false;
        }
        if self.shutdown.load(Ordering::SeqCst) {
            return true;
        }
        // Orphelin : plus aucune connexion depuis IDLE_EXIT.
        self.connections.load(Ordering::SeqCst) == 0
            && self
                .last_activity
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .elapsed()
                > IDLE_EXIT
    }

    /// Boucle d'acceptation, jusqu'à `Shutdown` (différé tant qu'un job
    /// tourne) ou jusqu'à l'inactivité prolongée. Une erreur d'`accept`
    /// (descripteurs épuisés, connexion avortée) n'arrête jamais le daemon :
    /// un job en cours en dépend.
    pub fn run(self: &Arc<Self>, listener: UnixListener) -> Result<()> {
        while !self.should_stop() {
            match listener.accept() {
                Ok((stream, _)) => {
                    if self.connections.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                        drop(stream);
                        continue;
                    }
                    self.connections.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(UNAUTHENTICATED_READ_TIMEOUT));
                    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
                    let server = Arc::clone(self);
                    thread::spawn(move || {
                        let _guard = ConnectionGuard(&server);
                        server.handle_connection(stream);
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(ACCEPT_POLL);
                    self.sessions.purge_expired(Instant::now());
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => thread::sleep(ACCEPT_POLL),
            }
        }
        // Ne retirer que notre propre socket : une autre instance a pu
        // s'installer au même chemin pendant un job prolongé.
        let own_inode = *self.socket_inode.lock().unwrap_or_else(|e| e.into_inner());
        if own_inode.is_some() && polkit::inode_of_path(&self.config.socket_path) == own_inode {
            let _ = std::fs::remove_file(&self.config.socket_path);
        }
        Ok(())
    }

    fn handle_connection(&self, stream: UnixStream) {
        let peer = match polkit::peer_credentials(&stream) {
            Ok(peer) => peer,
            Err(_) => return,
        };
        if polkit::peer_allowed(peer, self.config.allowed_uid).is_err() {
            // Fermé sans un octet : rien à apprendre pour un pair étranger.
            return;
        }
        let Ok(read_half) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(read_half);
        let mut conn = Connection {
            authenticated: false,
            peer,
            writer: Arc::new(Mutex::new(stream)),
        };
        loop {
            let request = match ipc::read_request(&mut reader) {
                Ok(request) => request,
                Err(FrameError::Json(e)) => {
                    let error = HelperError::Protocol(e.to_string());
                    if send(&conn.writer, &Message::error(0, &error)).is_err() {
                        break;
                    }
                    continue;
                }
                Err(_) => break,
            };
            let id = request.id;
            let close_after =
                matches!(request.cmd, Command::Authenticate { .. }) && !conn.authenticated;
            let was_authenticated = conn.authenticated;
            let outcome = self.dispatch(&mut conn, request);
            if conn.authenticated && !was_authenticated {
                // Authentifiée : les opérations longues n'ont plus de délai.
                let _ = conn
                    .writer
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .set_read_timeout(None);
            }
            let message = match &outcome {
                Ok(result) => Message::Ok {
                    id,
                    result: result.clone(),
                },
                Err(error) => Message::error(id, error),
            };
            if send(&conn.writer, &message).is_err() {
                break;
            }
            // Un amorçage faux ferme la connexion : pas de seconde chance.
            if close_after && !conn.authenticated {
                break;
            }
        }
    }

    fn dispatch(&self, conn: &mut Connection, request: Request) -> Result<Value> {
        let now = Instant::now();
        match request.cmd {
            Command::Authenticate { bootstrap } => {
                if !self.config.bootstrap.matches(&bootstrap) {
                    return Err(IsolationError::BadBootstrap.into());
                }
                conn.authenticated = true;
                Ok(json!({
                    "uid": conn.peer.uid,
                    "pid": conn.peer.pid,
                    "ttl_seconds": self.config.ttl.as_secs(),
                }))
            }
            _ if !conn.authenticated => Err(IsolationError::Unauthenticated.into()),
            Command::ScanDevice {
                device,
                policy,
                preferences,
            } => self.scan_device(&device, policy, preferences, now),
            Command::PrepareCanaries { token } => self.prepare_canaries(&token, now),
            Command::ExecutePlan {
                token,
                confirm_serial,
                policy,
                preferences,
            } => self.execute_plan(
                conn,
                request.id,
                &token,
                &confirm_serial,
                policy,
                preferences,
                now,
            ),
            Command::ListDisks {} => self.list_disks(),
            Command::HostInfo {} => Ok(serde_json::to_value(hostinfo::collect(
                &self.config.paths.sys,
                &self.config.paths.proc,
            ))?),
            Command::Unfreeze { token } => self.unfreeze(&token, now),
            Command::Unlock {
                token,
                password,
                identifier,
            } => self.unlock(&token, &password, identifier, now),
            Command::VerifyTestdisk { token } => {
                self.verify_testdisk(conn, request.id, &token, now)
            }
            Command::Cancel { token } => {
                let id = self.sessions.resolve(&token, now)?;
                let (job, testdisk) = self
                    .sessions
                    .with(id, |s| (s.job.clone(), Arc::clone(&s.testdisk)))
                    .ok_or(IsolationError::TokenUnknown)?;
                if testdisk::kill(&testdisk) {
                    return Ok(json!({
                        "requested": true,
                        "honored_now": true,
                        "note": "analyse TestDisk interrompue",
                    }));
                }
                let job = job.ok_or(HelperError::NoJob)?;
                if job.is_finished() {
                    return Err(HelperError::NoJob);
                }
                let honored = job.request_cancel();
                Ok(json!({
                    "requested": true,
                    "honored_now": honored,
                    "note": if honored {
                        "arrêt à la prochaine frontière de bloc ou de sondage"
                    } else {
                        "phase bloquante (Format NVM, SECURITY ERASE UNIT) : arrêt après la commande"
                    },
                }))
            }
            Command::GetStatus { token } => {
                let id = self.sessions.resolve(&token, now)?;
                let snapshot = self
                    .sessions
                    .with(id, |s| {
                        json!({
                            "session": {
                                "identity": s.identity,
                                "expires_in": s.remaining(now),
                                "canaries_prepared": s.canaries_seed.is_some(),
                            },
                            "job": s.job.as_ref().map(|j| json!({
                                "finished": j.is_finished(),
                                "cancel_requested": j.is_cancelled(),
                                "cancellable": j.is_cancellable(),
                                "progress": j.status(),
                                "report": j.report(),
                            })),
                        })
                    })
                    .ok_or(IsolationError::TokenUnknown)?;
                Ok(snapshot)
            }
            Command::Shutdown {} => {
                self.request_shutdown();
                let deferred = self.sessions.any_job_running();
                Ok(json!({
                    "stopping": true,
                    "deferred": deferred,
                    "note": if deferred {
                        "un job est en cours : arrêt du daemon à sa fin"
                    } else {
                        "arrêt immédiat"
                    },
                }))
            }
        }
    }

    /// Inventaire sysfs + protections. Un montage intraçable ne fait pas
    /// échouer l'inventaire : tous les disques sont alors marqués tenus.
    fn list_disks(&self) -> Result<Value> {
        let paths = &self.config.paths;
        let disks = sysfs::list_disks(paths)?;
        let (report, note) = match protection::protections(paths, &disks) {
            Ok(report) => (Some(report), None),
            Err(DeviceError::Untraceable(detail)) => (None, Some(detail)),
            Err(e) => return Err(e.into()),
        };
        let entries: Vec<Value> = disks
            .iter()
            .map(|d| {
                let p = report
                    .as_ref()
                    .map(|r| r.of(&d.name))
                    .unwrap_or(protection::Protection {
                        hosts_system: false,
                        mounted: false,
                        swap: false,
                        held: true,
                    });
                json!({
                    "name": d.name,
                    "path": format!("/dev/{}", d.name),
                    "major": d.major,
                    "minor": d.minor,
                    "transport": d.transport.label(),
                    "size_bytes": d.size_bytes,
                    "size_label": d.size_label(),
                    "rotational": d.rotational,
                    "read_only": d.read_only,
                    "removable": d.removable,
                    "model": d.model,
                    "serial": d.serial,
                    "firmware": d.firmware,
                    "wwid": d.wwid,
                    "partitions": d.partitions,
                    "protection": {
                        "hosts_system": p.hosts_system,
                        "mounted": p.mounted,
                        "swap": p.swap,
                        "held": p.held,
                        "summary": p.summary(),
                    },
                })
            })
            .collect();
        Ok(json!({
            "disks": entries,
            "root_traced": report.as_ref().map(|r| r.root_traced),
            "note": note,
        }))
    }

    /// Dé-frozen : suspend-to-RAM puis ré-identification du disque de la session.
    fn unfreeze(&self, token: &str, now: Instant) -> Result<Value> {
        let id = self.sessions.resolve_fresh(token, now)?;
        let expected = self.session_identity(id)?;
        self.ensure_no_job(id)?;
        if self.sessions.any_job_running() {
            // La mise en veille coupe l'alimentation de tous les disques :
            // interdite tant qu'un effacement ou une analyse tourne.
            return Err(HelperError::JobRunning);
        }
        let (device, scan) =
            identity::scan(&self.config.paths, &expected.name, OpenMode::ReadOnly)?;
        expected.check_against(&scan.identity)?;
        if device.is_nvme() {
            return Err(HelperError::Refused(
                "le dé-frozen ATA ne s'applique pas à un disque NVMe".into(),
            ));
        }
        let ata = AtaDevice::for_transport(device.file(), scan.entry.transport);
        let outcome = defrozen::defrozen_by_suspend(&ata, DefrozenOptions::default())?;
        Ok(json!({
            "was_frozen": outcome.was_frozen,
            "suspended": outcome.suspended,
            "still_frozen": outcome.still_frozen,
        }))
    }

    /// Déverrouillage avec un mot de passe fourni par l'opérateur : SECURITY
    /// UNLOCK si le disque est verrouillé, puis SECURITY DISABLE PASSWORD pour
    /// rendre le firmware éligible. Le disque n'est jamais contourné : sans le
    /// bon mot de passe, le firmware refuse et l'erreur est remontée telle quelle.
    fn unlock(
        &self,
        token: &str,
        password: &ipc::Secret,
        identifier: ipc::PasswordIdentifier,
        now: Instant,
    ) -> Result<Value> {
        let id = self.sessions.resolve_fresh(token, now)?;
        let expected = self.session_identity(id)?;
        self.ensure_no_job(id)?;
        let (device, scan) =
            identity::scan(&self.config.paths, &expected.name, OpenMode::ReadOnly)?;
        expected.check_against(&scan.identity)?;
        if device.is_nvme() {
            return Err(HelperError::Refused(
                "ATA Security ne s'applique pas à un disque NVMe".into(),
            ));
        }
        let ata = scan.caps.ata.ok_or_else(|| {
            HelperError::Refused("état ATA Security inconnu (inspection firmware échouée)".into())
        })?;
        if !ata.security_enabled {
            return Err(HelperError::Refused(
                "aucun mot de passe actif sur ce disque".into(),
            ));
        }
        if ata.security_frozen {
            return Err(HelperError::Refused(
                "disque gelé (FROZEN) : dé-frozen requis avant le déverrouillage".into(),
            ));
        }
        if ata.security_attempts_expired {
            return Err(HelperError::Refused(
                "compteur de tentatives expiré : cycle d'alimentation du disque requis".into(),
            ));
        }
        let password = security::Password::from_ascii(password.expose())?;
        let who = match identifier {
            ipc::PasswordIdentifier::User => security::Identifier::User,
            ipc::PasswordIdentifier::Master => security::Identifier::Master,
        };
        let drive = AtaDevice::for_transport(device.file(), scan.entry.transport);
        if ata.security_locked {
            drive.security_unlock(who, &password)?;
        }
        drive.security_disable_password(who, &password)?;
        let after = drive.identify()?;
        if !after.security.enabled {
            let _ = PasswordVault::new(self.vault_dir()).remove(&scan.identity.serial);
        }
        Ok(json!({
            "unlocked": !after.security.locked,
            "security_enabled": after.security.enabled,
            "locked": after.security.locked,
        }))
    }

    /// TestDisk sur le disque de la session : ré-identification en lecture
    /// seule, puis analyse scriptée avec les lignes relayées en événements.
    fn verify_testdisk(
        &self,
        conn: &Connection,
        request_id: u64,
        token: &str,
        now: Instant,
    ) -> Result<Value> {
        let id = self.sessions.resolve_fresh(token, now)?;
        let expected = self.session_identity(id)?;
        self.ensure_no_job(id)?;
        let slot = self
            .sessions
            .with(id, |s| Arc::clone(&s.testdisk))
            .ok_or(IsolationError::TokenUnknown)?;
        {
            let (device, scan) =
                identity::scan(&self.config.paths, &expected.name, OpenMode::ReadOnly)?;
            expected.check_against(&scan.identity)?;
            drop(device);
        }
        let device_path = self.config.paths.dev.join(&expected.name);
        let devnum = DevNum {
            major: expected.major,
            minor: expected.minor,
        };
        let on_path = identity::devnum_of_path(&device_path)
            .map_err(|e| HelperError::io(format!("stat {}", device_path.display()), e))?;
        if on_path != devnum {
            return Err(IsolationError::IdentityMismatch {
                field: "devnum",
                expected: devnum.to_string(),
                found: on_path.to_string(),
            }
            .into());
        }
        let writer = Arc::clone(&conn.writer);
        let run = testdisk::run(&device_path, &slot, |line| {
            let _ = send(
                &writer,
                &Message::Event {
                    id: request_id,
                    event: Event::Log {
                        message: line.to_string(),
                    },
                },
            );
        })?;
        Ok(serde_json::to_value(run)?)
    }

    fn decide(
        caps: &TargetCapabilities,
        policy: PolicyId,
        preferences: PreferencesWire,
    ) -> Result<Verdict> {
        Ok(engine::decide(caps, &policy.policy(), &preferences.into())?)
    }

    fn scan_device(
        &self,
        device: &str,
        policy: PolicyId,
        preferences: PreferencesWire,
        now: Instant,
    ) -> Result<Value> {
        // Un disque tenu par un job (effacement, TestDisk) ne reçoit aucune
        // commande d'inspection : IDENTIFY pendant un SECURITY ERASE UNIT
        // déclenche une réinitialisation du bus et casse l'effacement.
        {
            let path = secmet_device::block::device_path(device)?;
            let name = secmet_device::block::device_name(&path)?;
            let entry = sysfs::read_disk(&self.config.paths, &name)?;
            if self.sessions.busy_devnum(entry.major, entry.minor) {
                return Err(HelperError::JobRunning);
            }
        }
        let (handle, mut scan) = identity::scan(&self.config.paths, device, OpenMode::ReadOnly)?;
        // Mot de passe ATA laissé par un SECURITY ERASE interrompu : levée
        // automatique si le coffre le connaît, puis réinspection.
        let vault = PasswordVault::new(self.vault_dir());
        let recovery = recovery::recover_ata_password(&handle, &scan, &vault);
        if recovery
            .as_ref()
            .is_some_and(|r| r.status == recovery::RecoveryStatus::Recovered)
        {
            scan = identity::inspect(&self.config.paths, &handle)?;
        }
        drop(handle);
        let verdict = Self::decide(&scan.caps, policy, preferences)?;
        let (session_id, token) = self.sessions.issue(scan.identity.clone(), now)?;
        Ok(json!({
            "token": token,
            "session_id": session_id,
            "expires_in": self.config.ttl.as_secs(),
            "identity": scan.identity,
            "disk": disk_view(&scan),
            "verdict": verdict_view(&verdict),
            "recovery": recovery,
        }))
    }

    fn session_identity(&self, id: u64) -> Result<DiskIdentity> {
        self.sessions
            .with(id, |s| s.identity.clone())
            .ok_or_else(|| IsolationError::TokenUnknown.into())
    }

    /// Refuse si un job ou une analyse tient le disque de la session,
    /// par cette session ou par une autre (même disque, jeton plus récent).
    fn ensure_no_job(&self, id: u64) -> Result<()> {
        let identity = self.session_identity(id)?;
        if self.sessions.busy_devnum(identity.major, identity.minor) {
            Err(HelperError::JobRunning)
        } else {
            Ok(())
        }
    }

    fn session_seed(&self, id: u64) -> Result<SessionSeed> {
        let fresh = SessionSeed::from_bytes(polkit::random_bytes()?);
        // Lecture et pose sous un seul verrou : deux appels concurrents sur
        // le même jeton partagent la même graine.
        let hex = self
            .sessions
            .with(id, |s| {
                s.canaries_seed
                    .get_or_insert_with(|| fresh.to_hex())
                    .clone()
            })
            .ok_or(IsolationError::TokenUnknown)?;
        SessionSeed::from_hex(&hex)
            .ok_or_else(|| HelperError::Internal("seed de session illisible".into()))
    }

    fn prepare_canaries(&self, token: &str, now: Instant) -> Result<Value> {
        let id = self.sessions.resolve_fresh(token, now)?;
        let expected = self.session_identity(id)?;
        self.ensure_no_job(id)?;
        let isolated = isolation::open_for_erase(&self.config.paths, &expected, false)?;
        let seed = self.session_seed(id)?;
        let rules = canary::SamplingRules::default();
        let sample = canary::sample_lbas(&seed, isolated.device.block_count(), &rules)?;
        let vault = PasswordVault::new(self.vault_dir());
        let mut target = DiskTarget::new(isolated.device, &isolated.scan, vault);
        let handle = JobHandle::new();
        let mut sink: Vec<Event> = Vec::new();
        let written = runner::prepare_canaries(&mut target, &seed, &sample, &handle, &mut sink)
            .map_err(|f| HelperError::Internal(format!("{f:?}")))?;
        Ok(json!({
            "seed": seed.to_hex(),
            "sampled": sample.len(),
            "written": sample.len(),
            "readable": written,
            "guarded_devnums": isolated.guarded.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_plan(
        &self,
        conn: &Connection,
        request_id: u64,
        token: &str,
        confirm_serial: &str,
        policy: PolicyId,
        preferences: PreferencesWire,
        now: Instant,
    ) -> Result<Value> {
        let id = self.sessions.resolve_fresh(token, now)?;
        let expected = self.session_identity(id)?;
        // Sans numéro de série (pont USB muet), la confirmation porte sur
        // l'empreinte d'identité : une chaîne vide ne confirme rien.
        let reference = if expected.serial.is_empty() {
            expected.fingerprint.as_str()
        } else {
            expected.serial.as_str()
        };
        if confirm_serial.is_empty() || confirm_serial != reference {
            return Err(HelperError::Refused(
                "identité du disque non confirmée : numéro de série (ou empreinte, à défaut) \
                 à renvoyer à l'identique"
                    .into(),
            ));
        }
        self.ensure_no_job(id)?;

        // Ouverture exclusive et ré-identification, puis décision sur les
        // capacités relues derrière ce descripteur.
        let isolated = isolation::open_for_erase(&self.config.paths, &expected, false)?;
        let plan = match Self::decide(&isolated.scan.caps, policy, preferences)? {
            Verdict::Ready(plan) => *plan,
            Verdict::Refused { reasons } => {
                return Err(HelperError::Refused(format!("{reasons:?}")))
            }
            Verdict::Blocked { reasons, guidance } => {
                return Err(HelperError::Blocked(format!(
                    "{reasons:?}, consigne {guidance:?}"
                )))
            }
        };
        if plan.steps().any(|s| s.whole_controller) {
            // Une commande sur tout le contrôleur : les frères doivent être
            // libres eux aussi. Rouvrir avec la garde étendue.
            drop(isolated);
            let widened = isolation::open_for_erase(&self.config.paths, &expected, true)?;
            return self.spawn_job(conn, request_id, id, widened, plan);
        }
        self.spawn_job(conn, request_id, id, isolated, plan)
    }

    fn spawn_job(
        &self,
        conn: &Connection,
        request_id: u64,
        session_id: u64,
        isolated: isolation::Isolated,
        plan: ErasePlan,
    ) -> Result<Value> {
        let seed = self.session_seed(session_id)?;
        let handle = Arc::new(JobHandle::new());
        self.sessions
            .with(session_id, |s| s.job = Some(Arc::clone(&handle)));
        let (tx, rx) = mpsc::channel::<Event>();
        let writer = Arc::clone(&conn.writer);
        let relay = thread::spawn(move || {
            for event in rx {
                if send(
                    &writer,
                    &Message::Event {
                        id: request_id,
                        event,
                    },
                )
                .is_err()
                {
                    // UI partie : on continue de consommer pour ne jamais
                    // bloquer l'exécuteur, sans rien écrire.
                    continue;
                }
            }
        });
        let vault = PasswordVault::new(self.vault_dir());
        let plan_view = plan_view(&plan);
        let job_handle = Arc::clone(&handle);
        let scan = isolated.scan.clone();
        let device = isolated.device;
        thread::spawn(move || {
            let mut tx = tx;
            let mut target = DiskTarget::new(device, &scan, vault);
            let report = runner::run_plan(&mut target, &plan, &seed, &job_handle, &mut tx);
            let value =
                serde_json::to_value(&report).unwrap_or_else(|e| json!({ "error": e.to_string() }));
            tx.emit_done(value.clone());
            // Le relais doit avoir écrit `done` avant que le job soit déclaré
            // fini : un arrêt différé coupe le processus juste après.
            drop(tx);
            let _ = relay.join();
            job_handle.finish(value);
        });
        Ok(json!({
            "accepted": true,
            "plan": plan_view,
            "seed": self.sessions.with(session_id, |s| s.canaries_seed.clone()).flatten(),
            "guarded_devnums": isolated.guarded.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
        }))
    }
}

trait DoneSink {
    fn emit_done(&mut self, value: Value);
}

impl DoneSink for mpsc::Sender<Event> {
    fn emit_done(&mut self, value: Value) {
        let _ = self.send(Event::Done(value));
    }
}

fn disk_view(scan: &Scan) -> Value {
    let caps = &scan.caps;
    json!({
        "name": scan.identity.name,
        "model": scan.model,
        "firmware": scan.firmware,
        "serial": scan.identity.serial,
        "transport": format!("{:?}", caps.transport).to_lowercase(),
        "media": format!("{:?}", caps.media).to_lowercase(),
        "block_size": caps.block_size,
        "block_count": caps.block_count,
        "size_bytes": caps.capacity_bytes(),
        "size_label": scan.entry.size_label(),
        "partitions": scan.entry.partitions,
        "removable": scan.entry.removable,
        "protection": {
            "hosts_system": caps.protection.hosts_system,
            "mounted": caps.protection.mounted,
            "swap": caps.protection.swap,
            "held": caps.protection.held,
        },
        "inspection_failed": caps.inspection_failed,
        "inspection_error": scan.inspection_error,
        "smart": scan.smart,
        "rpm": scan.rpm,
        "ata": caps.ata.map(|a| json!({
            "sanitize_supported": a.sanitize_supported,
            "sanitize_block_erase": a.sanitize_block_erase,
            "sanitize_crypto_scramble": a.sanitize_crypto_scramble,
            "sanitize_overwrite": a.sanitize_overwrite,
            "sanitize_frozen": a.sanitize_frozen,
            "security_supported": a.security_supported,
            "security_enabled": a.security_enabled,
            "security_locked": a.security_locked,
            "security_frozen": a.security_frozen,
            "enhanced_erase_supported": a.enhanced_erase_supported,
            "security_erase_minutes": a.security_erase_minutes,
            "enhanced_erase_minutes": a.enhanced_erase_minutes,
        })),
        "nvme": caps.nvme.map(|n| json!({
            "sanitize_block_erase": n.sanitize_block_erase,
            "sanitize_crypto_erase": n.sanitize_crypto_erase,
            "sanitize_overwrite": n.sanitize_overwrite,
            "format_supported": n.format_supported,
            "format_crypto_erase": n.format_crypto_erase,
            "format_erases_all_namespaces": n.format_erases_all_namespaces,
            "multiple_namespaces": n.multiple_namespaces,
            "read_only": n.read_only,
        })),
        "health": {
            "smart_failing": caps.health.smart_failing,
            "reallocated_sectors": caps.health.reallocated_sectors,
            "pending_sectors": caps.health.pending_sectors,
            "nvme_percentage_used": caps.health.nvme_percentage_used,
            "nvme_media_errors": caps.health.nvme_media_errors.to_string(),
        },
    })
}

fn step_view(step: &engine::PlannedStep) -> Value {
    json!({
        "method": step.method.label(),
        "level": step.level.to_string(),
        "failure_criteria": step.failure_criteria.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>(),
        "write_canaries": step.verification.write_canaries,
        "require_no_signatures": step.verification.require_no_signatures,
        "samples": step.verification.sampling.random_samples,
        "whole_controller": step.whole_controller,
        "estimated_seconds": step.estimated_seconds,
    })
}

/// Vue JSON d'un plan.
pub fn plan_view(plan: &ErasePlan) -> Value {
    json!({
        "policy": plan.policy_id,
        "target_level": plan.target_level.to_string(),
        "worst_level": plan.worst_level().to_string(),
        "primary": step_view(&plan.primary),
        "fallbacks": plan.fallbacks.iter().map(step_view).collect::<Vec<_>>(),
        "excluded": plan.excluded.iter().map(|e| json!({
            "method": e.method.label(),
            "level": e.level.to_string(),
            "reason": format!("{:?}", e.reason),
            "guidance": format!("{:?}", e.reason.guidance()),
        })).collect::<Vec<_>>(),
        "degradation": plan.degradation.as_ref().map(|d| json!({
            "from": d.from.to_string(),
            "to": d.to.to_string(),
            "lost": d.lost.iter().map(|e| e.method.label()).collect::<Vec<_>>(),
            "guidance": d.guidance.iter().map(|g| format!("{g:?}")).collect::<Vec<_>>(),
        })),
        "recommend_destroy": plan.recommend_destroy.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>(),
        "notes": plan.notes.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>(),
    })
}

/// Vue JSON d'un verdict.
pub fn verdict_view(verdict: &Verdict) -> Value {
    match verdict {
        Verdict::Refused { reasons } => json!({
            "state": "refused",
            "reasons": reasons.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>(),
        }),
        Verdict::Blocked { reasons, guidance } => json!({
            "state": "blocked",
            "reasons": reasons.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>(),
            "guidance": guidance.iter().map(|g| format!("{g:?}")).collect::<Vec<_>>(),
        }),
        Verdict::Ready(plan) => json!({
            "state": "ready",
            "plan": plan_view(plan),
        }),
    }
}

/// Point d'entrée de `secmet-helper serve`.
pub fn main(args: &[String]) -> i32 {
    match serve(args) {
        Ok(()) => 0,
        Err(e) => {
            // stderr peut être un tube fermé (UI partie) : ne jamais paniquer.
            let _ = writeln!(std::io::stderr(), "secmet-helper serve: {e}");
            match e {
                HelperError::Protocol(_) => 2,
                _ => 1,
            }
        }
    }
}

fn serve(args: &[String]) -> Result<()> {
    let mut socket_dir = PathBuf::from(DEFAULT_SOCKET_DIR);
    let mut vault_dir = PathBuf::from(crate::vault::DEFAULT_DIR);
    let mut ttl = DEFAULT_TTL;
    let mut allow_uid: Option<u32> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let value = |iter: &mut std::slice::Iter<'_, String>| {
            iter.next()
                .cloned()
                .ok_or_else(|| HelperError::Protocol(format!("{arg} attend une valeur")))
        };
        match arg.as_str() {
            "--socket-dir" => socket_dir = PathBuf::from(value(&mut iter)?),
            "--vault-dir" => vault_dir = PathBuf::from(value(&mut iter)?),
            "--ttl" => {
                ttl = Duration::from_secs(value(&mut iter)?.parse().map_err(|_| {
                    HelperError::Protocol("--ttl attend un nombre de secondes".into())
                })?)
            }
            "--allow-uid" => {
                allow_uid = Some(
                    value(&mut iter)?
                        .parse()
                        .map_err(|_| HelperError::Protocol("--allow-uid attend un uid".into()))?,
                )
            }
            other => {
                return Err(HelperError::Protocol(format!("argument inconnu: {other}")));
            }
        }
    }
    let euid = polkit::effective_uid();
    if polkit::pkexec_uid().is_some()
        && (allow_uid.is_some()
            || socket_dir != Path::new(DEFAULT_SOCKET_DIR)
            || vault_dir != Path::new(crate::vault::DEFAULT_DIR))
    {
        // Sous pkexec (auth_admin_keep), un processus du même utilisateur
        // pourrait relancer le helper sans invite avec ses propres chemins :
        // seuls les chemins compilés sont acceptés en mode privilégié.
        return Err(HelperError::Protocol(
            "--socket-dir, --vault-dir et --allow-uid sont réservés au mode de test hors pkexec"
                .into(),
        ));
    }
    let allowed_uid = match (allow_uid, polkit::pkexec_uid()) {
        (Some(uid), _) => uid,
        (None, Some(uid)) => uid,
        (None, None) if euid != 0 => euid,
        (None, None) => {
            return Err(HelperError::Protocol(
                "PKEXEC_UID absent : lancer via pkexec, ou passer --allow-uid".into(),
            ))
        }
    };
    if euid != 0 {
        eprintln!("secmet-helper serve: avertissement, pas root (uid {euid}) : aucun disque ne pourra être ouvert");
    }
    polkit::set_umask(0o077);
    let socket_path = polkit::prepare_socket_dir(&socket_dir, SOCKET_NAME)?;
    let bootstrap = Bootstrap::random()?;
    let server = Server::new(ServerConfig {
        socket_path: socket_path.clone(),
        allowed_uid,
        bootstrap: bootstrap.clone(),
        ttl,
        paths: SysfsPaths::default(),
        vault_dir,
    });
    let listener = server.bind()?;
    let ready = json!({
        "event": "ready",
        "socket": socket_path,
        "bootstrap": bootstrap.to_hex(),
        "ttl_seconds": ttl.as_secs(),
        "uid": allowed_uid,
    });
    {
        let mut stdout = std::io::stdout().lock();
        ipc::write_line(&mut stdout, &ready).map_err(|e| HelperError::io("stdout", e))?;
        let _ = stdout.flush();
    }
    server.run(listener)
}
