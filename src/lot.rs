//! Lot persistant (Annexe M, §M.4) : un lot regroupe les machines traitées
//! successivement avec la même clé USB, d'un redémarrage à l'autre. Son
//! état vit sur le support de données, dans `lots/<LOT-ID>/` :
//!
//! - `lot.json` : instantané (statut, opérateur, entrées machine) ;
//! - `journal.ndjson` : journal chaîné, une ligne par événement, jamais
//!   réécrit ; chaque ligne porte l'empreinte de la précédente, supprimer
//!   ou modifier une ligne casse la chaîne ;
//! - `certificates/`, `sessions/` : certificats et journaux des machines.
//!
//! Toute écriture est forcée sur le support avant de continuer ; les
//! fichiers réécrits le sont par fichier temporaire puis renommage. Ce
//! module est pur (pas de GTK) et ne connaît ni le helper ni la session
//! d'effacement : l'interface l'appelle aux moments clés.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rand::Rng;
use secmet_attest::{canonical, sha256_hex};
use serde::{Deserialize, Serialize};

/// Instantané du lot.
pub const LOT_FILE: &str = "lot.json";
/// Journal chaîné.
pub const JOURNAL_FILE: &str = "journal.ndjson";
/// Certificats (PDF et attestations) des machines du lot.
pub const CERTIFICATES_DIR: &str = "certificates";
/// Journaux de session des machines du lot.
pub const SESSIONS_DIR: &str = "sessions";
/// Préfixe des identifiants de lot (`L-AAAAMMJJ-XXXX`).
pub const LOT_ID_PREFIX: &str = "L";
/// Version du schéma de `lot.json` et du journal.
pub const SCHEMA_VERSION: u32 = 1;
/// Étiquettes de la partition d'export (exFAT limite à 11 caractères).
pub const EXPORT_LABELS: [&str; 2] = ["SECMET-EXP", "SECMET-EXPORT"];

static ACTIVE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Dossier du lot actif, publié par l'interface : journaux de session et
/// certificats y vont tant qu'il est défini (lisible depuis tout fil).
pub fn set_active_dir(dir: Option<PathBuf>) {
    *ACTIVE_DIR.lock().unwrap_or_else(|e| e.into_inner()) = dir;
}

/// Dossier du lot actif, s'il y en a un.
pub fn active_dir() -> Option<PathBuf> {
    ACTIVE_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Point de montage de la partition d'export, si une partition portant
/// l'une des étiquettes [`EXPORT_LABELS`] est montée.
pub fn find_export_mount() -> Option<PathBuf> {
    let device = EXPORT_LABELS
        .iter()
        .map(|l| PathBuf::from("/dev/disk/by-label").join(l))
        .find_map(|p| fs::canonicalize(p).ok())?;
    let mounts = fs::read_to_string("/proc/self/mounts").ok()?;
    mounts.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let source = fields.next()?;
        let target = fields.next()?;
        (fs::canonicalize(source).ok()? == device)
            .then(|| PathBuf::from(target.replace("\\040", " ")))
    })
}

/// Statut du lot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LotStatus {
    /// Des machines peuvent encore y être ajoutées.
    Open,
    /// Clôturé : PDF groupé et manifeste produits, plus aucune écriture.
    Closed,
    /// Abandonné sans aucun certificat : archivé tel quel, avec son
    /// journal, sans certificat groupé ni manifeste.
    Abandoned,
}

/// Machine traitée (DMI lu par le helper sur le poste où tourne SecMet).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineIdentity {
    /// Constructeur.
    #[serde(default)]
    pub manufacturer: String,
    /// Modèle.
    #[serde(default)]
    pub model: String,
    /// Numéro de série ; vide si le DMI n'en donne pas.
    #[serde(default)]
    pub serial: String,
    /// Nom d'hôte, à titre indicatif.
    #[serde(default)]
    pub hostname: String,
}

/// Disque d'une machine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskIdentity {
    /// Nœud (`sda`, `nvme0n1`).
    pub name: String,
    /// Modèle.
    #[serde(default)]
    pub model: String,
    /// Numéro de série ; vide si inconnu.
    #[serde(default)]
    pub serial: String,
    /// Taille en octets.
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

/// Portée d'un lot : ce que compte une entrée et ce qui fait doublon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LotScope {
    /// Clé live : une entrée par machine (série DMI) ; doublon = même
    /// machine ou mêmes disques.
    #[default]
    Machines,
    /// Poste installé (banc) : une entrée par session de disques ; la
    /// machine est toujours le poste, doublon = mêmes disques seulement.
    Disks,
}

/// Statut d'une entrée machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    /// Effacement lancé, pas encore conclu.
    InProgress,
    /// Tous les disques traités avec succès, certificat produit.
    Finished,
    /// Au moins un échec ; certificat produit (en échec) ou non.
    Failed,
    /// Trouvée « en cours » au démarrage suivant : coupure ou plantage.
    Interrupted,
    /// Remplacée par une entrée plus récente de la même machine.
    Replaced,
}

/// Certificat produit pour une machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificateRef {
    /// `certificate_id` de l'attestation.
    pub certificate_id: String,
    /// Fichier JSON signé, relatif à `certificates/`.
    pub file: String,
    /// Empreinte SHA-256 de l'enveloppe (JSON canonique).
    pub sha256: String,
}

/// Une machine du lot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineEntry {
    /// Numéro d'entrée, croissant dans le lot.
    pub id: u32,
    /// Machine.
    pub machine: MachineIdentity,
    /// Disques traités.
    pub disks: Vec<DiskIdentity>,
    /// Session d'effacement SecMet (`SEC-…`).
    pub session_id: String,
    /// Début.
    pub started_at: String,
    /// Fin, si conclue.
    #[serde(default)]
    pub finished_at: Option<String>,
    /// Statut.
    pub status: EntryStatus,
    /// Certificats produits.
    #[serde(default)]
    pub certificates: Vec<CertificateRef>,
    /// Entrée qui remplace celle-ci.
    #[serde(default)]
    pub replaced_by: Option<u32>,
}

/// Contenu de `lot.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LotState {
    /// Version du schéma.
    pub version: u32,
    /// `L-AAAAMMJJ-XXXX`.
    pub id: String,
    /// Opérateur ayant ouvert le lot.
    pub operator: String,
    /// Référence client, libre.
    #[serde(default)]
    pub reference: String,
    /// Logiciel à l'ouverture.
    pub software: String,
    /// Ouverture.
    pub opened_at: String,
    /// Clôture.
    #[serde(default)]
    pub closed_at: Option<String>,
    /// Statut.
    pub status: LotStatus,
    /// Portée (machines ou disques) ; lots anciens : machines.
    #[serde(default)]
    pub scope: LotScope,
    /// Machines, dans l'ordre.
    #[serde(default)]
    pub entries: Vec<MachineEntry>,
    /// Prochain numéro d'entrée.
    pub next_entry_id: u32,
}

/// Événement du journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Ouverture du lot.
    LotOpened {
        /// Opérateur.
        operator: String,
        /// Référence client.
        reference: String,
        /// Logiciel.
        software: String,
        /// Portée du lot ; absente dans les journaux anciens (machines).
        #[serde(default)]
        scope: LotScope,
    },
    /// Effacement lancé sur une machine.
    MachineStarted {
        /// Numéro d'entrée.
        entry: u32,
        /// Machine.
        machine: MachineIdentity,
        /// Disques.
        disks: Vec<DiskIdentity>,
        /// Session.
        session_id: String,
    },
    /// Machine conclue.
    MachineFinished {
        /// Numéro d'entrée.
        entry: u32,
        /// `finished` ou `failed`.
        status: EntryStatus,
        /// Certificats.
        certificates: Vec<CertificateRef>,
    },
    /// Entrée trouvée « en cours » au démarrage suivant.
    MachineInterrupted {
        /// Numéro d'entrée.
        entry: u32,
    },
    /// Certificat régénéré après son enregistrement (même machine, même
    /// clé de signature) : nouvelle empreinte prise en compte à la clôture.
    CertificateUpdated {
        /// Numéro d'entrée.
        entry: u32,
        /// `certificate_id` concerné.
        certificate_id: String,
        /// Empreinte enregistrée auparavant.
        previous_sha256: String,
        /// Empreinte du fichier actuel (`sha256` est pris par la ligne).
        certificate_sha256: String,
    },
    /// Entrée remplacée par une plus récente.
    MachineReplaced {
        /// Entrée remplacée.
        entry: u32,
        /// Entrée qui la remplace.
        by: u32,
    },
    /// Clôture.
    LotClosed {
        /// Certificats listés au manifeste.
        certificates: u32,
        /// Empreinte SHA-256 du manifeste signé.
        manifest_sha256: String,
    },
    /// Abandon : aucun certificat, aucun livrable ; le lot est archivé
    /// tel quel avec ses entrées en échec ou interrompues.
    LotAbandoned {
        /// Entrées (hors remplacées) présentes au moment de l'abandon.
        entries: u32,
    },
}

/// Une ligne du journal. `sha256` couvre `seq`, `at`, `prev` et l'événement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalLine {
    /// Numéro de ligne, à partir de 1.
    pub seq: u64,
    /// Horodatage local.
    pub at: String,
    /// Empreinte de la ligne précédente (64 zéros pour la première).
    pub prev: String,
    /// Événement.
    #[serde(flatten)]
    pub event: Event,
    /// Empreinte de cette ligne.
    pub sha256: String,
}

impl JournalLine {
    /// Empreinte attendue : SHA-256 du JSON canonique sans `sha256`.
    pub fn compute_sha256(&self) -> Result<String, String> {
        let mut value = serde_json::to_value(self).map_err(|e| e.to_string())?;
        if let Some(obj) = value.as_object_mut() {
            obj.remove("sha256");
        }
        Ok(sha256_hex(canonical::to_string(&value).as_bytes()))
    }
}

const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn now_stamp() -> String {
    crate::clock::stamp()
}

/// Nouvel identifiant `L-AAAAMMJJ-XXXX`.
pub fn new_lot_id() -> String {
    let suffix: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(4)
        .map(char::from)
        .collect();
    format!(
        "{LOT_ID_PREFIX}-{}-{}",
        crate::clock::now().format("%Y%m%d"),
        suffix.to_uppercase()
    )
}

fn create_private_dir(path: &Path) -> Result<(), String> {
    match fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
    {
        Ok(()) | Err(_) if path.is_dir() => Ok(()),
        Err(e) => Err(format!("{} : {e}", path.display())),
        Ok(()) => Ok(()),
    }
}

fn fsync_dir(dir: &Path) -> Result<(), String> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| format!("fsync {} : {e}", dir.display()))
}

/// Écriture sûre : fichier temporaire `0600`, `fsync`, renommage, `fsync`
/// du dossier. Le fichier final est soit absent, soit complet.
pub fn write_atomic_sync(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("{} : pas de dossier", path.display()))?;
    let tmp = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| format!("{} : {e}", tmp.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{} : {e}", tmp.display()))?;
    drop(file);
    fs::rename(&tmp, path).map_err(|e| format!("{} : {e}", path.display()))?;
    fsync_dir(dir)
}

/// Un lot ouvert ou clôturé, avec son dossier.
#[derive(Debug, Clone)]
pub struct Lot {
    dir: PathBuf,
    state: LotState,
}

impl Lot {
    /// Ouvre un nouveau lot sous `root` (`lots/`).
    pub fn open(
        root: &Path,
        operator: &str,
        reference: &str,
        software: &str,
        scope: LotScope,
    ) -> Result<Lot, String> {
        let operator = operator.trim();
        if operator.is_empty() {
            return Err("Nom de l'opérateur requis".into());
        }
        let id = new_lot_id();
        let dir = root.join(&id);
        if dir.exists() {
            return Err(format!("{} existe déjà", dir.display()));
        }
        create_private_dir(&dir.join(CERTIFICATES_DIR))?;
        create_private_dir(&dir.join(SESSIONS_DIR))?;
        let state = LotState {
            version: SCHEMA_VERSION,
            id,
            operator: operator.to_string(),
            reference: reference.trim().to_string(),
            software: software.to_string(),
            opened_at: now_stamp(),
            closed_at: None,
            status: LotStatus::Open,
            scope,
            entries: Vec::new(),
            next_entry_id: 1,
        };
        let lot = Lot { dir, state };
        lot.append(Event::LotOpened {
            operator: lot.state.operator.clone(),
            reference: lot.state.reference.clone(),
            software: software.to_string(),
            scope,
        })?;
        lot.save()?;
        Ok(lot)
    }

    /// Charge le lot du dossier `dir`.
    pub fn load(dir: &Path) -> Result<Lot, String> {
        let path = dir.join(LOT_FILE);
        let text = fs::read_to_string(&path).map_err(|e| format!("{} : {e}", path.display()))?;
        let state: LotState =
            serde_json::from_str(&text).map_err(|e| format!("{} : {e}", path.display()))?;
        if state.version != SCHEMA_VERSION {
            return Err(format!(
                "{} : version {} non supportée",
                path.display(),
                state.version
            ));
        }
        Ok(Lot {
            dir: dir.to_path_buf(),
            state,
        })
    }

    /// Le lot ouvert sous `root`, s'il y en a un. Plusieurs lots ouverts
    /// (ne devrait pas arriver) : le plus récent, les autres sont signalés.
    pub fn find_open(root: &Path) -> Result<Option<Lot>, String> {
        let Ok(entries) = fs::read_dir(root) else {
            return Ok(None);
        };
        let mut open: Vec<Lot> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join(LOT_FILE).is_file())
            .filter_map(|e| Lot::load(&e.path()).ok())
            .filter(|lot| lot.state.status == LotStatus::Open)
            .collect();
        open.sort_by(|a, b| a.state.opened_at.cmp(&b.state.opened_at));
        if open.len() > 1 {
            eprintln!(
                "{} lots ouverts sous {} : le plus récent est repris",
                open.len(),
                root.display()
            );
        }
        Ok(open.pop())
    }

    /// Identifiant.
    pub fn id(&self) -> &str {
        &self.state.id
    }

    /// État courant.
    pub fn state(&self) -> &LotState {
        &self.state
    }

    /// Dossier du lot.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Dossier des certificats.
    pub fn certificates_dir(&self) -> PathBuf {
        self.dir.join(CERTIFICATES_DIR)
    }

    /// Dossier des journaux de session.
    pub fn sessions_dir(&self) -> PathBuf {
        self.dir.join(SESSIONS_DIR)
    }

    /// Entrées qui comptent (ni remplacées) ; machines distinctes par
    /// numéro de série, ou par entrée si la série est vide.
    /// Disques distincts (numéros de série ; sans série, chacun compte).
    pub fn disks_count(&self) -> u32 {
        let mut serials = std::collections::HashSet::new();
        let mut count = 0u32;
        for d in self
            .state
            .entries
            .iter()
            .filter(|e| e.status != EntryStatus::Replaced)
            .flat_map(|e| e.disks.iter())
        {
            if d.serial.is_empty() || serials.insert(d.serial.clone()) {
                count += 1;
            }
        }
        count
    }

    /// Unités comptées selon la portée : machines ou disques.
    pub fn units_count(&self) -> u32 {
        match self.state.scope {
            LotScope::Machines => self.machines_count(),
            LotScope::Disks => self.disks_count(),
        }
    }

    pub fn machines_count(&self) -> u32 {
        let mut serials = std::collections::HashSet::new();
        let mut count = 0u32;
        for e in self
            .state
            .entries
            .iter()
            .filter(|e| e.status != EntryStatus::Replaced)
        {
            if e.machine.serial.is_empty() || serials.insert(e.machine.serial.clone()) {
                count += 1;
            }
        }
        count
    }

    /// Entrée précédente de la même machine (série DMI identique non vide,
    /// ou mêmes numéros de série de disques), non remplacée.
    pub fn find_previous(
        &self,
        machine: &MachineIdentity,
        disks: &[DiskIdentity],
    ) -> Option<&MachineEntry> {
        let disk_serials: Vec<&str> = disks
            .iter()
            .map(|d| d.serial.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        self.state.entries.iter().rev().find(|e| {
            if e.status == EntryStatus::Replaced {
                return false;
            }
            let same_machine = !machine.serial.is_empty() && e.machine.serial == machine.serial;
            let same_disks = !disk_serials.is_empty()
                && disk_serials
                    .iter()
                    .all(|s| e.disks.iter().any(|d| d.serial == *s));
            match self.state.scope {
                LotScope::Machines => same_machine || same_disks,
                // Sur un banc, la machine est toujours la même : seuls les
                // disques comptent.
                LotScope::Disks => same_disks,
            }
        })
    }

    fn ensure_open(&self) -> Result<(), String> {
        if self.state.status != LotStatus::Open {
            return Err(format!("Lot {} clôturé", self.state.id));
        }
        Ok(())
    }

    /// Enregistre le lancement d'un effacement sur une machine.
    pub fn start_machine(
        &mut self,
        machine: MachineIdentity,
        disks: Vec<DiskIdentity>,
        session_id: &str,
    ) -> Result<u32, String> {
        self.ensure_open()?;
        let id = self.state.next_entry_id;
        self.state.next_entry_id += 1;
        self.append(Event::MachineStarted {
            entry: id,
            machine: machine.clone(),
            disks: disks.clone(),
            session_id: session_id.to_string(),
        })?;
        self.state.entries.push(MachineEntry {
            id,
            machine,
            disks,
            session_id: session_id.to_string(),
            started_at: now_stamp(),
            finished_at: None,
            status: EntryStatus::InProgress,
            certificates: Vec::new(),
            replaced_by: None,
        });
        self.save()?;
        Ok(id)
    }

    /// Marque `old` comme remplacée par `by`.
    pub fn replace(&mut self, old: u32, by: u32) -> Result<(), String> {
        self.ensure_open()?;
        let entry = self
            .state
            .entries
            .iter_mut()
            .find(|e| e.id == old)
            .ok_or_else(|| format!("entrée {old} inconnue"))?;
        entry.status = EntryStatus::Replaced;
        entry.replaced_by = Some(by);
        self.append(Event::MachineReplaced { entry: old, by })?;
        self.save()
    }

    /// Conclut une machine (`Finished` ou `Failed`) avec ses certificats.
    pub fn finish_machine(
        &mut self,
        id: u32,
        status: EntryStatus,
        certificates: Vec<CertificateRef>,
    ) -> Result<(), String> {
        self.ensure_open()?;
        if !matches!(status, EntryStatus::Finished | EntryStatus::Failed) {
            return Err("statut de fin attendu : finished ou failed".into());
        }
        let entry = self
            .state
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("entrée {id} inconnue"))?;
        entry.status = status;
        entry.finished_at = Some(now_stamp());
        entry.certificates = certificates.clone();
        self.append(Event::MachineFinished {
            entry: id,
            status,
            certificates,
        })?;
        self.save()
    }

    /// Remplace l'empreinte d'un certificat régénéré après enregistrement
    /// (l'appelant a vérifié signature et identité). Journalisé.
    pub fn update_certificate(
        &mut self,
        id: u32,
        certificate_id: &str,
        sha256: &str,
    ) -> Result<(), String> {
        self.ensure_open()?;
        let entry = self
            .state
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("entrée {id} inconnue"))?;
        let cert = entry
            .certificates
            .iter_mut()
            .find(|c| c.certificate_id == certificate_id)
            .ok_or_else(|| format!("certificat {certificate_id} inconnu de l'entrée {id}"))?;
        if cert.sha256 == sha256 {
            return Ok(());
        }
        let previous_sha256 = std::mem::replace(&mut cert.sha256, sha256.to_string());
        self.append(Event::CertificateUpdated {
            entry: id,
            certificate_id: certificate_id.to_string(),
            previous_sha256,
            certificate_sha256: sha256.to_string(),
        })?;
        self.save()
    }

    /// Au démarrage : les entrées restées « en cours » passent à
    /// « interrompue ». Retourne leurs numéros.
    pub fn mark_interrupted(&mut self) -> Result<Vec<u32>, String> {
        let ids: Vec<u32> = self
            .state
            .entries
            .iter()
            .filter(|e| e.status == EntryStatus::InProgress)
            .map(|e| e.id)
            .collect();
        if ids.is_empty() {
            return Ok(ids);
        }
        self.ensure_open()?;
        for id in &ids {
            self.append(Event::MachineInterrupted { entry: *id })?;
            if let Some(e) = self.state.entries.iter_mut().find(|e| e.id == *id) {
                e.status = EntryStatus::Interrupted;
                e.finished_at = Some(now_stamp());
            }
        }
        self.save()?;
        Ok(ids)
    }

    /// Certificats des entrées qui comptent, dans l'ordre du lot.
    pub fn certificates(&self) -> Vec<&CertificateRef> {
        self.state
            .entries
            .iter()
            .filter(|e| e.status != EntryStatus::Replaced)
            .flat_map(|e| e.certificates.iter())
            .collect()
    }

    /// Empreinte SHA-256 du fichier journal.
    pub fn journal_sha256(&self) -> Result<String, String> {
        let path = self.dir.join(JOURNAL_FILE);
        fs::read(&path)
            .map(|bytes| sha256_hex(&bytes))
            .map_err(|e| format!("{} : {e}", path.display()))
    }

    /// Clôture : plus aucune écriture ensuite. `manifest_sha256` est
    /// l'empreinte du manifeste signé déjà écrit dans le dossier.
    pub fn close(&mut self, manifest_sha256: &str) -> Result<(), String> {
        self.ensure_open()?;
        let certificates = u32::try_from(self.certificates().len()).unwrap_or(u32::MAX);
        self.append(Event::LotClosed {
            certificates,
            manifest_sha256: manifest_sha256.to_string(),
        })?;
        self.state.status = LotStatus::Closed;
        self.state.closed_at = Some(now_stamp());
        self.save()
    }

    /// Abandon d'un lot sans certificat : journalisé, plus aucune écriture
    /// ensuite. Refusé dès qu'un certificat est enregistré (il faut alors
    /// clôturer, pour que le certificat figure dans un manifeste).
    pub fn abandon(&mut self) -> Result<(), String> {
        self.ensure_open()?;
        let certificates = self.certificates().len();
        if certificates > 0 {
            return Err(format!(
                "Lot {} : {certificates} certificat(s) enregistré(s), abandon impossible",
                self.state.id
            ));
        }
        let entries = self
            .state
            .entries
            .iter()
            .filter(|e| e.status != EntryStatus::Replaced)
            .count();
        self.append(Event::LotAbandoned {
            entries: u32::try_from(entries).unwrap_or(u32::MAX),
        })?;
        self.state.status = LotStatus::Abandoned;
        self.state.closed_at = Some(now_stamp());
        self.save()
    }

    /// Déplace un lot clôturé ou abandonné sous `archive_root` (même
    /// support : renommage).
    pub fn archive(self, archive_root: &Path) -> Result<PathBuf, String> {
        if self.state.status == LotStatus::Open {
            return Err(format!("Lot {} non clôturé", self.state.id));
        }
        create_private_dir(archive_root)?;
        let target = archive_root.join(&self.state.id);
        fs::rename(&self.dir, &target)
            .map_err(|e| format!("{} → {} : {e}", self.dir.display(), target.display()))?;
        fsync_dir(archive_root)?;
        Ok(target)
    }

    fn append(&self, event: Event) -> Result<(), String> {
        let path = self.dir.join(JOURNAL_FILE);
        let (seq, prev) = match read_journal(&path) {
            Ok(lines) => lines
                .last()
                .map(|l| (l.seq + 1, l.sha256.clone()))
                .unwrap_or((1, GENESIS.to_string())),
            Err(e) if !path.exists() => {
                let _ = e;
                (1, GENESIS.to_string())
            }
            Err(e) => return Err(e),
        };
        let mut line = JournalLine {
            seq,
            at: now_stamp(),
            prev,
            event,
            sha256: String::new(),
        };
        line.sha256 = line.compute_sha256()?;
        let mut text = serde_json::to_string(&line).map_err(|e| e.to_string())?;
        text.push('\n');
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("{} : {e}", path.display()))?;
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("{} : {e}", path.display()))
    }

    fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.state).map_err(|e| e.to_string())?;
        write_atomic_sync(&self.dir.join(LOT_FILE), json.as_bytes())
    }
}

/// Relit le journal et vérifie la chaîne : numéros consécutifs, chaque
/// `prev` égal à l'empreinte de la ligne précédente, chaque empreinte
/// recalculée identique. Erreur à la première rupture.
pub fn read_journal(path: &Path) -> Result<Vec<JournalLine>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{} : {e}", path.display()))?;
    let mut lines = Vec::new();
    let mut prev = GENESIS.to_string();
    for (n, raw) in text.lines().enumerate() {
        if raw.trim().is_empty() {
            continue;
        }
        let line: JournalLine =
            serde_json::from_str(raw).map_err(|e| format!("journal ligne {} : {e}", n + 1))?;
        let expected_seq = lines.len() as u64 + 1;
        if line.seq != expected_seq {
            return Err(format!(
                "journal ligne {} : numéro {} attendu, {} lu",
                n + 1,
                expected_seq,
                line.seq
            ));
        }
        if line.prev != prev {
            return Err(format!("journal ligne {} : chaîne rompue", n + 1));
        }
        if line.compute_sha256()? != line.sha256 {
            return Err(format!("journal ligne {} : empreinte altérée", n + 1));
        }
        prev = line.sha256.clone();
        lines.push(line);
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn machine(serial: &str) -> MachineIdentity {
        MachineIdentity {
            manufacturer: "Dell".into(),
            model: "Latitude".into(),
            serial: serial.into(),
            hostname: "live".into(),
        }
    }

    fn disk(serial: &str) -> DiskIdentity {
        DiskIdentity {
            name: "nvme0n1".into(),
            model: "NVMe".into(),
            serial: serial.into(),
            size_bytes: Some(512_000_000_000),
        }
    }

    fn cert(id: &str) -> CertificateRef {
        CertificateRef {
            certificate_id: id.into(),
            file: format!("{id}.json"),
            sha256: "ab".repeat(32),
        }
    }

    #[test]
    fn abandon_only_without_certificates_then_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("lots");
        let mut lot = Lot::open(&root, "Jean", "", "secmet 0.3.9", LotScope::Machines).unwrap();
        let entry = lot
            .start_machine(machine("M1"), vec![disk("D1")], "S1")
            .unwrap();
        lot.finish_machine(entry, EntryStatus::Failed, vec![])
            .unwrap();
        lot.abandon().unwrap();
        assert_eq!(lot.state().status, LotStatus::Abandoned);
        assert!(lot.state().closed_at.is_some());
        assert!(lot.abandon().is_err(), "plus d'écriture après l'abandon");
        let lines = read_journal(&lot.dir().join(JOURNAL_FILE)).unwrap();
        assert_eq!(
            lines.last().unwrap().event,
            Event::LotAbandoned { entries: 1 }
        );
        assert!(Lot::find_open(&root).unwrap().is_none());
        let archived = lot.archive(&tmp.path().join("archive")).unwrap();
        assert_eq!(
            Lot::load(&archived).unwrap().state().status,
            LotStatus::Abandoned
        );

        let mut with_cert =
            Lot::open(&root, "Jean", "", "secmet 0.3.9", LotScope::Machines).unwrap();
        let entry = with_cert
            .start_machine(machine("M2"), vec![disk("D2")], "S2")
            .unwrap();
        with_cert
            .finish_machine(entry, EntryStatus::Finished, vec![cert("C1")])
            .unwrap();
        assert!(
            with_cert.abandon().is_err(),
            "un certificat impose la clôture"
        );
        assert_eq!(with_cert.state().status, LotStatus::Open);
    }

    #[test]
    fn full_lifecycle_and_chained_journal() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("lots");
        let mut lot = Lot::open(
            &root,
            "Jean",
            "client X",
            "secmet 0.3.1",
            LotScope::Machines,
        )
        .unwrap();
        assert!(lot.id().starts_with("L-"));
        assert!(lot.certificates_dir().is_dir());

        let e1 = lot
            .start_machine(machine("PC1"), vec![disk("D1")], "SEC-1")
            .unwrap();
        lot.finish_machine(e1, EntryStatus::Finished, vec![cert("SEC-1-nvme0n1")])
            .unwrap();
        let e2 = lot
            .start_machine(machine("PC2"), vec![disk("D2")], "SEC-2")
            .unwrap();
        lot.finish_machine(e2, EntryStatus::Failed, vec![]).unwrap();
        assert_eq!(lot.machines_count(), 2);
        assert_eq!(lot.certificates().len(), 1);

        // Rechargement : même état, journal intact.
        let reloaded = Lot::load(lot.dir()).unwrap();
        assert_eq!(reloaded.state(), lot.state());
        let lines = read_journal(&lot.dir().join(JOURNAL_FILE)).unwrap();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0].prev, GENESIS);
        assert_eq!(lines[4].prev, lines[3].sha256);

        // Clôture puis archivage.
        let found = Lot::find_open(&root).unwrap().unwrap();
        assert_eq!(found.id(), lot.id());
        lot.close(&"cd".repeat(32)).unwrap();
        assert!(lot.start_machine(machine("PC3"), vec![], "SEC-3").is_err());
        assert!(Lot::find_open(&root).unwrap().is_none());
        let archived = lot.archive(&tmp.path().join("archive")).unwrap();
        assert!(archived.join(LOT_FILE).is_file());
        assert!(!root.join(archived.file_name().unwrap()).exists());
        let lines = read_journal(&archived.join(JOURNAL_FILE)).unwrap();
        assert!(matches!(
            lines.last().unwrap().event,
            Event::LotClosed {
                certificates: 1,
                ..
            }
        ));
    }

    #[test]
    fn regenerated_certificate_is_journaled() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lot = Lot::open(tmp.path(), "Jean", "", "secmet", LotScope::Machines).unwrap();
        let e = lot
            .start_machine(machine("PC1"), vec![disk("D1")], "SEC-1")
            .unwrap();
        lot.finish_machine(e, EntryStatus::Finished, vec![cert("C1")])
            .unwrap();
        // Même empreinte : rien n'est journalisé.
        lot.update_certificate(e, "C1", &"ab".repeat(32)).unwrap();
        assert_eq!(
            read_journal(&lot.dir().join(JOURNAL_FILE)).unwrap().len(),
            3
        );
        // Nouvelle empreinte : état et journal mis à jour, chaîne intacte.
        lot.update_certificate(e, "C1", &"cd".repeat(32)).unwrap();
        assert_eq!(lot.certificates()[0].sha256, "cd".repeat(32));
        let lines = read_journal(&lot.dir().join(JOURNAL_FILE)).unwrap();
        assert_eq!(lines.len(), 4);
        assert!(matches!(
            &lines[3].event,
            Event::CertificateUpdated { entry, certificate_id, previous_sha256, .. }
                if *entry == e && certificate_id == "C1" && *previous_sha256 == "ab".repeat(32)
        ));
        let reloaded = Lot::load(lot.dir()).unwrap();
        assert_eq!(reloaded.certificates()[0].sha256, "cd".repeat(32));
        assert!(lot.update_certificate(e, "C9", "00").is_err());
    }

    #[test]
    fn disks_scope_ignores_the_bench_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lot = Lot::open(tmp.path(), "Jean", "", "secmet", LotScope::Disks).unwrap();
        let bench = machine("BANC-1");
        let e1 = lot
            .start_machine(bench.clone(), vec![disk("D1"), disk("D2")], "SEC-1")
            .unwrap();
        lot.finish_machine(e1, EntryStatus::Finished, vec![cert("C1"), cert("C2")])
            .unwrap();
        // Même banc, autres disques : pas un doublon.
        assert!(lot.find_previous(&bench, &[disk("D3")]).is_none());
        // Un disque qui repasse : doublon.
        assert!(lot.find_previous(&bench, &[disk("D2")]).is_some());
        let e2 = lot
            .start_machine(bench.clone(), vec![disk("D3")], "SEC-2")
            .unwrap();
        lot.finish_machine(e2, EntryStatus::Finished, vec![cert("C3")])
            .unwrap();
        assert_eq!(lot.machines_count(), 1);
        assert_eq!(lot.disks_count(), 3);
        assert_eq!(lot.units_count(), 3);
        let reloaded = Lot::load(lot.dir()).unwrap();
        assert_eq!(reloaded.state().scope, LotScope::Disks);
        // Un lot « machines » voit le même banc comme un doublon.
        let mut m = Lot::open(tmp.path(), "Jean", "", "secmet", LotScope::Machines).unwrap();
        m.start_machine(bench.clone(), vec![disk("D1")], "SEC-9")
            .unwrap();
        assert!(m.find_previous(&bench, &[disk("D7")]).is_some());
        assert_eq!(m.units_count(), 1);
    }

    #[test]
    fn journal_tampering_is_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lot = Lot::open(tmp.path(), "Jean", "", "secmet", LotScope::Machines).unwrap();
        let e = lot
            .start_machine(machine("PC1"), vec![disk("D1")], "SEC-1")
            .unwrap();
        lot.finish_machine(e, EntryStatus::Finished, vec![cert("C1")])
            .unwrap();
        let path = lot.dir().join(JOURNAL_FILE);
        let original = fs::read_to_string(&path).unwrap();

        // Ligne modifiée.
        fs::write(&path, original.replace("\"Jean\"", "\"Paul\"")).unwrap();
        assert!(read_journal(&path)
            .unwrap_err()
            .contains("empreinte altérée"));
        // Ligne supprimée au milieu.
        let mut lines: Vec<&str> = original.lines().collect();
        lines.remove(1);
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        assert!(read_journal(&path).is_err());
        // Intact.
        fs::write(&path, &original).unwrap();
        assert_eq!(read_journal(&path).unwrap().len(), 3);
    }

    #[test]
    fn interrupted_entries_and_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lot = Lot::open(tmp.path(), "Jean", "", "secmet", LotScope::Machines).unwrap();
        let e1 = lot
            .start_machine(machine("PC1"), vec![disk("D1")], "SEC-1")
            .unwrap();
        // Coupure : rien de conclu. Au démarrage suivant :
        let mut again = Lot::load(lot.dir()).unwrap();
        assert_eq!(again.mark_interrupted().unwrap(), vec![e1]);
        assert_eq!(again.mark_interrupted().unwrap(), Vec::<u32>::new());
        assert_eq!(again.state().entries[0].status, EntryStatus::Interrupted);

        // Même machine refaite : détectée par la série DMI, puis remplacée.
        let previous = again
            .find_previous(&machine("PC1"), &[disk("D9")])
            .unwrap()
            .id;
        assert_eq!(previous, e1);
        let e2 = again
            .start_machine(machine("PC1"), vec![disk("D1")], "SEC-2")
            .unwrap();
        again.replace(previous, e2).unwrap();
        assert_eq!(again.state().entries[0].status, EntryStatus::Replaced);
        assert_eq!(again.state().entries[0].replaced_by, Some(e2));
        assert_eq!(again.machines_count(), 1);
        // Détection par les disques quand la série machine manque.
        assert_eq!(
            again.find_previous(&machine(""), &[disk("D1")]).unwrap().id,
            e2
        );
        assert!(again.find_previous(&machine(""), &[disk("ZZ")]).is_none());
    }

    #[test]
    fn open_requires_operator_and_rejects_bad_finish_status() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Lot::open(tmp.path(), "  ", "", "secmet", LotScope::Machines).is_err());
        let mut lot = Lot::open(tmp.path(), "Jean", "", "secmet", LotScope::Machines).unwrap();
        let e = lot.start_machine(machine("PC1"), vec![], "SEC-1").unwrap();
        assert!(lot
            .finish_machine(e, EntryStatus::Interrupted, vec![])
            .is_err());
        assert!(lot
            .finish_machine(99, EntryStatus::Finished, vec![])
            .is_err());
    }
}
