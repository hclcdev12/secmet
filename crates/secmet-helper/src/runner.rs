//! Exécution d'un plan du moteur : canaris, commandes firmware, écriture
//! hôte, validation par échantillons, signatures résiduelles, progression.
//!
//! Le runner ne connaît la cible qu'à travers [`Target`] : le vrai disque
//! ([`DiskTarget`], `O_DIRECT`, tampons alignés) ou un disque en mémoire
//! ([`memory::MemoryDisk`]) pour les tests. Toute la logique de décision
//! (ordre des étapes, critères d'échec) vient du plan ; ici on l'applique
//! étape par étape, en passant au repli suivant quand une étape échoue.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use secmet_core::canary::{self, SessionSeed, VerificationTarget};
use secmet_core::disposition::{classify_run, FinalVerdict, StepOutcome, StepTrace};
use secmet_core::engine::{ErasePlan, FailureCriterion, PlannedStep};
use secmet_core::taxonomy::EraseMethod;
use secmet_device::signatures;
use serde::{Deserialize, Serialize};

use crate::ipc::{Event, Progress};

/// Taille d'un morceau d'écriture séquentielle.
pub const CHUNK_BYTES: usize = 4 << 20;
/// Intervalle minimal entre deux événements de progression.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

/// Observateur d'une commande firmware longue.
pub trait Observer {
    /// Progression rapportée par le disque (0 à 100).
    fn progress(&mut self, percent: u8);
    /// Estimation du reste en secondes (`None` : inconnue), pour les
    /// commandes qui n'exposent pas de pourcentage mais une durée annoncée.
    fn eta(&mut self, _seconds: Option<u64>) {}
    /// L'opérateur a demandé l'arrêt.
    fn cancelled(&self) -> bool;
}

/// Pourquoi une étape n'a pas abouti.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepFailure {
    /// Un critère du plan est violé.
    Criterion {
        /// Le critère.
        criterion: FailureCriterion,
        /// Détail.
        detail: String,
    },
    /// Arrêt demandé par l'opérateur.
    Cancelled,
    /// E/S ou incohérence (hors critères du plan).
    Io(String),
}

impl StepFailure {
    fn criterion(criterion: FailureCriterion, detail: impl Into<String>) -> Self {
        Self::Criterion {
            criterion,
            detail: detail.into(),
        }
    }
}

impl From<io::Error> for StepFailure {
    fn from(e: io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// Une cible d'effacement.
pub trait Target {
    /// Taille de bloc logique.
    fn block_size(&self) -> u64;
    /// Nombre de blocs.
    fn block_count(&self) -> u64;
    /// Lit `buf.len() / block_size` blocs à partir de `lba`.
    fn read_blocks(&mut self, lba: u64, buf: &mut [u8]) -> io::Result<()>;
    /// Écrit `data.len() / block_size` blocs à partir de `lba`.
    fn write_blocks(&mut self, lba: u64, data: &[u8]) -> io::Result<()>;
    /// Vide les tampons jusqu'au média.
    fn flush(&mut self) -> io::Result<()>;
    /// Exécute une commande firmware (bloquante ou suivie par sondage).
    fn run_firmware(
        &mut self,
        method: EraseMethod,
        observer: &mut dyn Observer,
    ) -> Result<(), StepFailure>;
    /// La commande firmware `method` peut-elle être interrompue en cours ?
    fn firmware_cancellable(&self, method: EraseMethod) -> bool;
}

/// Issue d'une étape ou d'un job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Terminé et validé.
    Success,
    /// Un critère violé.
    Failed,
    /// Arrêté par l'opérateur.
    Cancelled,
}

/// Résumé d'une validation par échantillons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ValidationSummary {
    /// Blocs examinés.
    pub checked: usize,
    /// LBA illisibles (16 premiers).
    pub read_errors: Vec<u64>,
    /// Canaris survivants (16 premiers).
    pub canaries_intact: Vec<u64>,
    /// Motif attendu absent (16 premiers).
    pub pattern_mismatch: Vec<u64>,
    /// Non remis à zéro (16 premiers).
    pub not_zeroed: Vec<u64>,
    /// Totaux, au-delà des 16 premiers listés.
    pub anomalies: usize,
}

impl From<&canary::ValidationReport> for ValidationSummary {
    fn from(r: &canary::ValidationReport) -> Self {
        let head = |v: &[u64]| v.iter().take(16).copied().collect::<Vec<_>>();
        Self {
            checked: r.checked,
            read_errors: head(&r.read_errors),
            canaries_intact: head(&r.canaries_intact),
            pattern_mismatch: head(&r.pattern_mismatch),
            not_zeroed: head(&r.not_zeroed),
            anomalies: r.read_errors.len()
                + r.canaries_intact.len()
                + r.pattern_mismatch.len()
                + r.not_zeroed.len(),
        }
    }
}

/// Rapport d'une étape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepReport {
    /// Index dans le plan.
    pub index: usize,
    /// Méthode.
    pub method: String,
    /// Niveau NIST certifié en cas de succès.
    pub level: String,
    /// Issue.
    pub outcome: Outcome,
    /// Critère violé.
    pub criterion: Option<String>,
    /// Détail.
    pub detail: String,
    /// Validation par échantillons.
    pub validation: Option<ValidationSummary>,
    /// Signatures encore présentes.
    pub signatures: Vec<String>,
    /// Durée.
    pub seconds: u64,
    /// L'étape portait sur tout le contrôleur NVMe (sinon : le seul
    /// namespace ouvert ; sans objet hors NVMe).
    #[serde(default)]
    pub whole_controller: bool,
}

/// Rapport d'un job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobReport {
    /// Issue globale.
    pub outcome: Outcome,
    /// Niveau NIST atteint (étape réussie).
    pub achieved_level: Option<String>,
    /// Méthode réussie.
    pub method: Option<String>,
    /// Politique appliquée.
    pub policy: String,
    /// Seed de session (hex), pour recalculer tout motif.
    pub seed: String,
    /// Étapes tentées.
    pub steps: Vec<StepReport>,
    /// Durée totale.
    pub seconds: u64,
    /// Sort du support (`reusable`, `destroy_recommended`, `destroy_required`,
    /// `retry`) : [`secmet_core::disposition`].
    #[serde(default)]
    pub disposition: String,
    /// Raisons du sort (codes de `DispositionReason`).
    #[serde(default)]
    pub disposition_reasons: Vec<String>,
    /// Consignes avant relance (codes de `Guidance`).
    #[serde(default)]
    pub disposition_guidance: Vec<String>,
}

impl JobReport {
    /// Porte un verdict final sur le rapport.
    pub fn set_verdict(&mut self, verdict: &FinalVerdict) {
        self.disposition = verdict.disposition.code().to_string();
        self.disposition_reasons = verdict.reason_codes();
        self.disposition_guidance = verdict.guidance_codes();
    }
}

/// Poignée d'un job : annulation, état courant, rapport final.
#[derive(Debug, Default)]
pub struct JobHandle {
    cancel: AtomicBool,
    cancellable: AtomicBool,
    finished: AtomicBool,
    status: Mutex<Progress>,
    report: Mutex<Option<serde_json::Value>>,
}

impl JobHandle {
    /// Nouvelle poignée, annulable par défaut.
    pub fn new() -> Self {
        let handle = Self::default();
        handle.cancellable.store(true, Ordering::SeqCst);
        handle
    }

    /// Demande l'arrêt ; renvoie si la phase courante peut l'honorer.
    pub fn request_cancel(&self) -> bool {
        self.cancel.store(true, Ordering::SeqCst);
        self.cancellable.load(Ordering::SeqCst)
    }

    /// Arrêt demandé ?
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// La phase courante peut-elle s'interrompre ?
    pub fn is_cancellable(&self) -> bool {
        self.cancellable.load(Ordering::SeqCst)
    }

    fn set_cancellable(&self, value: bool) {
        self.cancellable.store(value, Ordering::SeqCst);
    }

    /// Terminé ?
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    /// Enregistre le rapport final.
    pub fn finish(&self, report: serde_json::Value) {
        *self.report.lock().unwrap_or_else(|e| e.into_inner()) = Some(report);
        self.finished.store(true, Ordering::SeqCst);
    }

    /// Rapport final, s'il existe.
    pub fn report(&self) -> Option<serde_json::Value> {
        self.report
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Dernière progression.
    pub fn status(&self) -> Progress {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn set_status(&self, progress: Progress) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = progress;
    }
}

/// Destination des événements d'un job.
pub trait EventSink {
    /// Émet un événement ; ne bloque jamais l'exécuteur.
    fn emit(&mut self, event: Event);
}

impl EventSink for Sender<Event> {
    fn emit(&mut self, event: Event) {
        let _ = self.send(event);
    }
}

impl EventSink for Vec<Event> {
    fn emit(&mut self, event: Event) {
        self.push(event);
    }
}

/// Suivi d'une phase : état partagé, événements cadencés, débit.
struct Phase<'a> {
    handle: &'a JobHandle,
    sink: &'a mut dyn EventSink,
    progress: Progress,
    started: Instant,
    last_emit: Instant,
    last_bytes: u64,
    /// Estimation imposée par la cible (phase sans octets comptés).
    eta_override: Option<u64>,
    /// La phase va à son terme même si l'arrêt est demandé : relecture
    /// après une commande firmware achevée (le disque est effacé, la preuve
    /// ne doit pas être perdue).
    ignore_cancel: bool,
}

impl<'a> Phase<'a> {
    fn new(
        handle: &'a JobHandle,
        sink: &'a mut dyn EventSink,
        step: usize,
        method: &str,
        phase: &str,
        bytes_total: u64,
    ) -> Self {
        let progress = Progress {
            step,
            method: method.to_string(),
            phase: phase.to_string(),
            bytes_total,
            ..Progress::default()
        };
        let now = Instant::now();
        let mut this = Self {
            handle,
            sink,
            progress,
            started: now,
            last_emit: now,
            last_bytes: 0,
            eta_override: None,
            ignore_cancel: false,
        };
        this.publish(true);
        this
    }

    fn bytes(&mut self, done: u64) {
        self.progress.bytes_done = done;
        let total = self.progress.bytes_total;
        self.progress.percent = done
            .saturating_mul(100)
            .checked_div(total)
            .map_or(0, |p| p.min(100) as u8);
        self.publish(done >= total && total > 0);
    }

    fn percent(&mut self, percent: u8) {
        self.progress.percent = percent.min(100);
        self.publish(percent >= 100);
    }

    fn publish(&mut self, force: bool) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_emit);
        if !force && elapsed < PROGRESS_INTERVAL {
            return;
        }
        let secs = elapsed.as_secs_f64();
        if secs > 0.0 {
            let delta = self.progress.bytes_done.saturating_sub(self.last_bytes);
            self.progress.bandwidth_bps = (delta as f64 / secs) as u64;
        }
        let remaining = self
            .progress
            .bytes_total
            .saturating_sub(self.progress.bytes_done);
        self.progress.eta_seconds = if self.progress.bytes_total == 0 {
            self.eta_override
        } else if remaining == 0 {
            Some(0)
        } else {
            let since_start = now.saturating_duration_since(self.started).as_secs_f64();
            let average = if since_start > 0.0 {
                self.progress.bytes_done as f64 / since_start
            } else {
                0.0
            };
            (average > 0.0).then(|| (remaining as f64 / average) as u64)
        };
        self.last_emit = now;
        self.last_bytes = self.progress.bytes_done;
        self.progress.cancellable = self.handle.is_cancellable();
        self.handle.set_status(self.progress.clone());
        self.sink.emit(Event::Progress(self.progress.clone()));
    }
}

impl Observer for Phase<'_> {
    fn progress(&mut self, percent: u8) {
        self.percent(percent);
    }

    fn eta(&mut self, seconds: Option<u64>) {
        self.eta_override = seconds;
    }

    fn cancelled(&self) -> bool {
        !self.ignore_cancel && self.handle.is_cancelled()
    }
}

/// Blocs échantillonnés pour `step`.
pub fn sample_for(
    target: &dyn Target,
    seed: &SessionSeed,
    step: &PlannedStep,
) -> Result<Vec<u64>, StepFailure> {
    canary::sample_lbas(seed, target.block_count(), &step.verification.sampling)
        .map_err(|e| StepFailure::Io(e.to_string()))
}

/// Écrit le canari de chaque LBA de `sample`, puis vide les tampons.
fn write_canaries(
    target: &mut dyn Target,
    seed: &SessionSeed,
    sample: &[u64],
    phase: &mut Phase<'_>,
) -> Result<(), StepFailure> {
    let block_size = target.block_size() as usize;
    let mut block = vec![0u8; block_size];
    for (i, &lba) in sample.iter().enumerate() {
        if phase.cancelled() {
            return Err(StepFailure::Cancelled);
        }
        canary::fill_canary(seed, lba, &mut block);
        target.write_blocks(lba, &block)?;
        phase.bytes((i as u64 + 1) * block_size as u64);
    }
    target.flush()?;
    Ok(())
}

/// Relit `sample` et compte les canaris intacts (pour `PrepareCanaries`).
pub fn count_canaries_present(
    target: &mut dyn Target,
    seed: &SessionSeed,
    sample: &[u64],
) -> Result<usize, StepFailure> {
    let block_size = target.block_size() as usize;
    let mut block = vec![0u8; block_size];
    let mut present = 0;
    for &lba in sample {
        target.read_blocks(lba, &mut block)?;
        if canary::classify(seed, lba, &block, 0) == canary::BlockContent::CanaryIntact {
            present += 1;
        }
    }
    Ok(present)
}

/// `PrepareCanaries` : écrit les canaris de `sample`, vide les tampons et
/// les relit ; renvoie le nombre de canaris relus intacts.
pub fn prepare_canaries(
    target: &mut dyn Target,
    seed: &SessionSeed,
    sample: &[u64],
    handle: &JobHandle,
    sink: &mut dyn EventSink,
) -> Result<usize, StepFailure> {
    let total = sample.len() as u64 * target.block_size();
    let mut phase = Phase::new(handle, sink, 0, "PREPARE_CANARIES", "canaries", total);
    write_canaries(target, seed, sample, &mut phase)?;
    count_canaries_present(target, seed, sample)
}

fn read_sample(
    target: &mut dyn Target,
    sample: &[u64],
    phase: &mut Phase<'_>,
) -> Result<Vec<canary::SampleRead>, StepFailure> {
    let block_size = target.block_size() as usize;
    let mut reads = Vec::with_capacity(sample.len());
    for (i, &lba) in sample.iter().enumerate() {
        if phase.cancelled() {
            return Err(StepFailure::Cancelled);
        }
        let mut block = vec![0u8; block_size];
        let data = target.read_blocks(lba, &mut block).ok().map(|()| block);
        reads.push(canary::SampleRead { lba, data });
        phase.bytes((i as u64 + 1) * block_size as u64);
    }
    Ok(reads)
}

/// Lit l'échantillon et le valide contre `expected`.
fn verify(
    target: &mut dyn Target,
    seed: &SessionSeed,
    sample: &[u64],
    expected: VerificationTarget,
    phase: &mut Phase<'_>,
) -> Result<canary::ValidationReport, StepFailure> {
    let reads = read_sample(target, sample, phase)?;
    canary::validate(seed, target.block_size(), sample, &reads, expected)
        .map_err(|e| StepFailure::Io(e.to_string()))
}

fn failure_of(report: &canary::ValidationReport) -> Option<StepFailure> {
    let first = |v: &[u64]| v.first().map(|l| l.to_string()).unwrap_or_default();
    if !report.read_errors.is_empty() {
        return Some(StepFailure::criterion(
            FailureCriterion::SampleReadError,
            format!(
                "{} bloc(s) illisible(s), premier LBA {}",
                report.read_errors.len(),
                first(&report.read_errors)
            ),
        ));
    }
    if !report.canaries_intact.is_empty() {
        return Some(StepFailure::criterion(
            FailureCriterion::CanaryStillPresent,
            format!(
                "{} canari(s) encore lisible(s), premier LBA {}",
                report.canaries_intact.len(),
                first(&report.canaries_intact)
            ),
        ));
    }
    if !report.pattern_mismatch.is_empty() {
        return Some(StepFailure::criterion(
            FailureCriterion::PatternMismatch,
            format!(
                "{} bloc(s) sans le motif attendu, premier LBA {}",
                report.pattern_mismatch.len(),
                first(&report.pattern_mismatch)
            ),
        ));
    }
    if !report.not_zeroed.is_empty() {
        return Some(StepFailure::criterion(
            FailureCriterion::NotZeroed,
            format!(
                "{} bloc(s) non remis à zéro, premier LBA {}",
                report.not_zeroed.len(),
                first(&report.not_zeroed)
            ),
        ));
    }
    None
}

/// Signatures encore présentes en tête et en queue de la cible.
pub fn residual_signatures(target: &mut dyn Target) -> Result<Vec<String>, StepFailure> {
    let block_size = target.block_size();
    let count = target.block_count();
    if count == 0 {
        return Ok(Vec::new());
    }
    let head_blocks = ((signatures::HEAD_BYTES as u64) / block_size).clamp(1, count);
    let mut head = vec![0u8; (head_blocks * block_size) as usize];
    target.read_blocks(0, &mut head)?;
    let mut tail = vec![0u8; block_size as usize];
    target.read_blocks(count - 1, &mut tail)?;
    let tail_offset = (count - 1) * block_size;
    Ok(
        signatures::scan_buffers(&head, &tail, tail_offset, block_size)
            .iter()
            .map(|s| s.kind.label().to_string())
            .collect(),
    )
}

/// Ce qu'une passe d'écriture hôte dépose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fill {
    Pass(u8),
    Zeros,
}

fn overwrite_pass(
    target: &mut dyn Target,
    seed: &SessionSeed,
    fill: Fill,
    phase: &mut Phase<'_>,
) -> Result<(), StepFailure> {
    let block_size = target.block_size();
    let count = target.block_count();
    let chunk_blocks = ((CHUNK_BYTES as u64) / block_size).max(1);
    let mut buffer = vec![0u8; (chunk_blocks * block_size) as usize];
    let mut lba = 0u64;
    while lba < count {
        if phase.cancelled() {
            return Err(StepFailure::Cancelled);
        }
        let blocks = chunk_blocks.min(count - lba);
        let bytes = (blocks * block_size) as usize;
        match fill {
            Fill::Pass(pass) => {
                canary::fill_pass_range(seed, pass, lba, block_size, &mut buffer[..bytes]);
            }
            Fill::Zeros => buffer[..bytes].fill(0),
        }
        target.write_blocks(lba, &buffer[..bytes])?;
        lba += blocks;
        phase.bytes(lba * block_size);
    }
    target.flush()?;
    Ok(())
}

struct StepContext<'a> {
    seed: &'a SessionSeed,
    handle: &'a JobHandle,
    sink: &'a mut dyn EventSink,
    index: usize,
    /// Dernière validation effectuée, conservée même si elle a échoué.
    last_validation: Option<ValidationSummary>,
}

fn run_firmware_step(
    target: &mut dyn Target,
    step: &PlannedStep,
    sample: &[u64],
    ctx: &mut StepContext<'_>,
) -> Result<canary::ValidationReport, StepFailure> {
    let label = step.method.label();
    let block_size = target.block_size();
    if step.verification.write_canaries {
        let mut phase = Phase::new(
            ctx.handle,
            ctx.sink,
            ctx.index,
            &label,
            "canaries",
            sample.len() as u64 * block_size,
        );
        write_canaries(target, ctx.seed, sample, &mut phase)?;
    }
    {
        ctx.handle
            .set_cancellable(target.firmware_cancellable(step.method));
        let mut phase = Phase::new(ctx.handle, ctx.sink, ctx.index, &label, "firmware", 0);
        let result = target.run_firmware(step.method, &mut phase);
        ctx.handle.set_cancellable(true);
        result?;
        target.flush()?;
    }
    let mut phase = Phase::new(
        ctx.handle,
        ctx.sink,
        ctx.index,
        &label,
        "verify",
        sample.len() as u64 * block_size,
    );
    // Un `Cancel` arrivé pendant la commande firmware (non interruptible)
    // ne doit pas jeter la validation d'un effacement qui a abouti : la
    // relecture va à son terme, l'arrêt ne vaut que pour les replis.
    phase.ignore_cancel = true;
    verify(
        target,
        ctx.seed,
        sample,
        step.verification.target,
        &mut phase,
    )
}

fn run_host_step(
    target: &mut dyn Target,
    step: &PlannedStep,
    passes: u8,
    final_zero: bool,
    sample: &[u64],
    ctx: &mut StepContext<'_>,
) -> Result<canary::ValidationReport, StepFailure> {
    let label = step.method.label();
    let block_size = target.block_size();
    let total = target.block_count() * block_size;
    let sample_bytes = sample.len() as u64 * block_size;
    for pass in 0..passes {
        let name = format!("pass {}/{passes}", pass + 1);
        let mut phase = Phase::new(ctx.handle, ctx.sink, ctx.index, &label, &name, total);
        overwrite_pass(target, ctx.seed, Fill::Pass(pass), &mut phase)?;
    }
    let last = passes.saturating_sub(1);
    {
        let mut phase = Phase::new(
            ctx.handle,
            ctx.sink,
            ctx.index,
            &label,
            "verify",
            sample_bytes,
        );
        let report = verify(
            target,
            ctx.seed,
            sample,
            VerificationTarget::PatternOfPass { pass: last },
            &mut phase,
        )?;
        ctx.last_validation = Some(ValidationSummary::from(&report));
        if let Some(failure) = failure_of(&report) {
            return Err(failure);
        }
        if !final_zero {
            return Ok(report);
        }
    }
    let mut phase = Phase::new(ctx.handle, ctx.sink, ctx.index, &label, "zero", total);
    overwrite_pass(target, ctx.seed, Fill::Zeros, &mut phase)?;
    let mut phase = Phase::new(
        ctx.handle,
        ctx.sink,
        ctx.index,
        &label,
        "verify",
        sample_bytes,
    );
    verify(
        target,
        ctx.seed,
        sample,
        VerificationTarget::ZerosAfterPasses { passes },
        &mut phase,
    )
}

fn run_step(
    target: &mut dyn Target,
    step: &PlannedStep,
    ctx: &mut StepContext<'_>,
) -> (StepReport, StepTrace) {
    let started = Instant::now();
    let label = step.method.label();
    ctx.sink.emit(Event::StepStarted {
        step: ctx.index,
        method: label.clone(),
        level: step.level.to_string(),
    });
    let mut report = StepReport {
        index: ctx.index,
        method: label.clone(),
        level: step.level.to_string(),
        outcome: Outcome::Failed,
        criterion: None,
        detail: String::new(),
        whole_controller: step.whole_controller,
        validation: None,
        signatures: Vec::new(),
        seconds: 0,
    };

    let result = sample_for(target, ctx.seed, step).and_then(|sample| {
        let validation = match step.method {
            EraseMethod::HostOverwrite { passes, final_zero } => {
                run_host_step(target, step, passes, final_zero, &sample, ctx)
            }
            _ => run_firmware_step(target, step, &sample, ctx),
        }?;
        ctx.last_validation = Some(ValidationSummary::from(&validation));
        if let Some(failure) = failure_of(&validation) {
            return Err(failure);
        }
        if step.verification.require_no_signatures {
            let mut phase = Phase::new(ctx.handle, ctx.sink, ctx.index, &label, "signatures", 0);
            phase.percent(0);
            let found = residual_signatures(target)?;
            phase.percent(100);
            if !found.is_empty() {
                report.signatures = found.clone();
                return Err(StepFailure::criterion(
                    FailureCriterion::SignatureStillPresent,
                    found.join(", "),
                ));
            }
        }
        Ok(())
    });

    report.validation = ctx.last_validation.take();
    let trace_outcome = match result {
        Ok(()) => {
            report.outcome = Outcome::Success;
            report.detail = "validé".to_string();
            StepOutcome::Success
        }
        Err(StepFailure::Cancelled) => {
            report.outcome = Outcome::Cancelled;
            report.detail = "arrêté par l'opérateur".to_string();
            StepOutcome::Cancelled
        }
        Err(StepFailure::Criterion { criterion, detail }) => {
            report.criterion = Some(format!("{criterion:?}"));
            report.detail = detail;
            StepOutcome::Failed(Some(criterion))
        }
        Err(StepFailure::Io(detail)) => {
            report.detail = detail;
            StepOutcome::Failed(None)
        }
    };
    report.seconds = started.elapsed().as_secs();
    ctx.sink.emit(Event::StepFinished {
        step: ctx.index,
        outcome: format!("{:?}", report.outcome).to_lowercase(),
        criterion: report.criterion.clone(),
        detail: report.detail.clone(),
    });
    (
        report,
        StepTrace {
            method: step.method,
            outcome: trace_outcome,
        },
    )
}

/// Exécute `plan` sur `target` : étape primaire puis replis, jusqu'au
/// premier succès validé. Le rapport liste chaque tentative.
pub fn run_plan(
    target: &mut dyn Target,
    plan: &ErasePlan,
    seed: &SessionSeed,
    handle: &JobHandle,
    sink: &mut dyn EventSink,
) -> JobReport {
    let started = Instant::now();
    let mut report = JobReport {
        outcome: Outcome::Failed,
        achieved_level: None,
        method: None,
        policy: plan.policy_id.to_string(),
        seed: seed.to_hex(),
        steps: Vec::new(),
        seconds: 0,
        disposition: String::new(),
        disposition_reasons: Vec::new(),
        disposition_guidance: Vec::new(),
    };
    let mut traces = Vec::new();
    for (index, step) in plan.steps().enumerate() {
        if handle.is_cancelled() {
            report.outcome = Outcome::Cancelled;
            break;
        }
        let mut ctx = StepContext {
            seed,
            handle,
            sink,
            index,
            last_validation: None,
        };
        let (step_report, trace) = run_step(target, step, &mut ctx);
        let outcome = step_report.outcome;
        report.steps.push(step_report);
        traces.push(trace);
        match outcome {
            Outcome::Success => {
                report.outcome = Outcome::Success;
                report.achieved_level = Some(step.level.to_string());
                report.method = Some(step.method.label());
                break;
            }
            Outcome::Cancelled => {
                report.outcome = Outcome::Cancelled;
                break;
            }
            Outcome::Failed => {
                sink.emit(Event::Log {
                    message: format!(
                        "étape {index} ({}) échouée, passage au repli suivant",
                        step.method.label()
                    ),
                });
            }
        }
    }
    report.seconds = started.elapsed().as_secs();
    // Verdict final : arrêt avant la première étape compris.
    let verdict = if traces.is_empty() && report.outcome == Outcome::Cancelled {
        FinalVerdict::cancelled()
    } else {
        classify_run(plan, &traces)
    };
    report.set_verdict(&verdict);
    report
}

/// Cible mémoire : un disque simulé pour les tests et les répétitions à blanc.
pub mod memory {
    use super::*;
    use std::collections::BTreeSet;

    /// Comportement simulé du firmware.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FakeFirmware {
        /// Remet tout à zéro (sanitize honnête).
        Zeroes,
        /// Acquitte sans rien faire (firmware menteur).
        Ignores,
        /// Rend un échec.
        Fails,
        /// Efface tout sauf un bloc sur `keep_every` (effacement partiel).
        Partial {
            /// Période des blocs conservés.
            keep_every: u64,
        },
    }

    /// Disque en mémoire.
    #[derive(Debug, Clone)]
    pub struct MemoryDisk {
        block_size: u64,
        data: Vec<u8>,
        /// Firmware simulé.
        pub firmware: FakeFirmware,
        /// Les écritures sont acquittées mais ignorées (contrôleur paresseux).
        pub drop_writes: bool,
        /// LBA dont la lecture échoue.
        pub unreadable: BTreeSet<u64>,
        /// Nombre d'écritures reçues.
        pub writes: u64,
        /// Nombre de `flush`.
        pub flushes: u64,
    }

    impl MemoryDisk {
        /// Disque de `blocks` blocs de `block_size`, rempli de `fill`.
        pub fn new(block_size: u64, blocks: u64, fill: u8) -> Self {
            Self {
                block_size,
                data: vec![fill; (block_size * blocks) as usize],
                firmware: FakeFirmware::Zeroes,
                drop_writes: false,
                unreadable: BTreeSet::new(),
                writes: 0,
                flushes: 0,
            }
        }

        /// Contenu brut.
        pub fn data(&self) -> &[u8] {
            &self.data
        }

        /// Écrit `bytes` à `offset` sans passer par le compteur (préparation).
        pub fn poke(&mut self, offset: usize, bytes: &[u8]) {
            if let Some(slot) = self.data.get_mut(offset..offset + bytes.len()) {
                slot.copy_from_slice(bytes);
            }
        }

        fn range(&self, lba: u64, len: usize) -> io::Result<std::ops::Range<usize>> {
            if len % self.block_size as usize != 0 {
                return Err(io::Error::other("longueur non multiple du bloc"));
            }
            let start = (lba * self.block_size) as usize;
            let end = start + len;
            if end > self.data.len() {
                return Err(io::Error::other("hors du disque"));
            }
            Ok(start..end)
        }
    }

    impl Target for MemoryDisk {
        fn block_size(&self) -> u64 {
            self.block_size
        }

        fn block_count(&self) -> u64 {
            self.data.len() as u64 / self.block_size
        }

        fn read_blocks(&mut self, lba: u64, buf: &mut [u8]) -> io::Result<()> {
            let range = self.range(lba, buf.len())?;
            let blocks = buf.len() as u64 / self.block_size;
            if (lba..lba + blocks).any(|l| self.unreadable.contains(&l)) {
                return Err(io::Error::other("erreur de lecture simulée"));
            }
            buf.copy_from_slice(&self.data[range]);
            Ok(())
        }

        fn write_blocks(&mut self, lba: u64, data: &[u8]) -> io::Result<()> {
            let range = self.range(lba, data.len())?;
            self.writes += 1;
            if !self.drop_writes {
                self.data[range].copy_from_slice(data);
            }
            Ok(())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }

        fn run_firmware(
            &mut self,
            method: EraseMethod,
            observer: &mut dyn Observer,
        ) -> Result<(), StepFailure> {
            if !method.is_firmware() {
                return Err(StepFailure::Io("pas une commande firmware".into()));
            }
            observer.progress(0);
            match self.firmware {
                FakeFirmware::Zeroes => self.data.fill(0),
                FakeFirmware::Ignores => {}
                FakeFirmware::Fails => {
                    return Err(StepFailure::criterion(
                        FailureCriterion::FirmwareReportedFailure,
                        "statut d'échec simulé",
                    ))
                }
                FakeFirmware::Partial { keep_every } => {
                    let bs = self.block_size as usize;
                    for (i, block) in self.data.chunks_mut(bs).enumerate() {
                        if keep_every == 0 || (i as u64) % keep_every != 0 {
                            block.fill(0);
                        }
                    }
                }
            }
            observer.progress(100);
            Ok(())
        }

        fn firmware_cancellable(&self, _method: EraseMethod) -> bool {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::memory::{FakeFirmware, MemoryDisk};
    use super::*;
    use secmet_core::engine::{decide, Preferences, Verdict};
    use secmet_core::taxonomy::{
        AtaCaps, HealthFacts, MediaType, NvmeCaps, Policy, ProtectionFlags, TargetCapabilities,
        Transport,
    };

    const BLOCKS: u64 = 4096;

    fn seed() -> SessionSeed {
        SessionSeed::from_bytes([0x5a; 32])
    }

    fn caps(nvme: bool, host_only: bool) -> TargetCapabilities {
        TargetCapabilities {
            name: "sdz".into(),
            serial: "SN".into(),
            media: MediaType::Ssd,
            transport: if nvme {
                Transport::Nvme
            } else {
                Transport::Sata
            },
            block_size: 512,
            block_count: BLOCKS,
            nvme: (nvme && !host_only).then_some(NvmeCaps {
                sanitize_block_erase: true,
                format_supported: true,
                format_crypto_erase: true,
                ..NvmeCaps::default()
            }),
            ata: (!nvme && !host_only).then_some(AtaCaps {
                sanitize_supported: true,
                sanitize_block_erase: true,
                security_supported: true,
                enhanced_erase_supported: true,
                ..AtaCaps::default()
            }),
            protection: ProtectionFlags::default(),
            health: HealthFacts::default(),
            inspection_failed: host_only,
        }
    }

    fn plan_for(caps: &TargetCapabilities, prefs: Preferences) -> ErasePlan {
        match decide(caps, &Policy::nist_800_88_rev1(), &prefs).unwrap() {
            Verdict::Ready(plan) => *plan,
            other => panic!("plan attendu, obtenu {other:?}"),
        }
    }

    fn small_sampling(plan: &mut ErasePlan) {
        // 4096 blocs : un échantillon de 100 suffit et garde les tests rapides.
        let rules = canary::SamplingRules {
            random_samples: 100,
            min_samples: 100,
        };
        plan.primary.verification.sampling = rules;
        for f in &mut plan.fallbacks {
            f.verification.sampling = rules;
        }
    }

    fn with_gpt(disk: &mut MemoryDisk) {
        disk.poke(510, &[0x55, 0xaa]);
        disk.poke(450, &[0xee]);
        disk.poke(512, b"EFI PART");
    }

    #[test]
    fn host_overwrite_with_final_zero_validates_and_clears_signatures() {
        let mut disk = MemoryDisk::new(512, BLOCKS, 0xcc);
        with_gpt(&mut disk);
        let mut plan = plan_for(&caps(false, true), Preferences::default());
        small_sampling(&mut plan);
        assert!(matches!(
            plan.primary.method,
            EraseMethod::HostOverwrite { .. }
        ));
        let handle = JobHandle::new();
        let mut events = Vec::new();
        let report = run_plan(&mut disk, &plan, &seed(), &handle, &mut events);
        assert_eq!(report.outcome, Outcome::Success, "{report:?}");
        assert_eq!(report.achieved_level.as_deref(), Some("Clear"));
        assert_eq!(report.method.as_deref(), Some("HOST_OVERWRITE_1_ZERO"));
        assert!(disk.data().iter().all(|&b| b == 0));
        assert_eq!(report.steps.len(), 1);
        assert!(report.steps[0].signatures.is_empty());
        let phases: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                Event::Progress(p) => Some(p.phase.clone()),
                _ => None,
            })
            .collect();
        for expected in ["pass 1/1", "verify", "zero", "signatures"] {
            assert!(phases.iter().any(|p| p == expected), "{phases:?}");
        }
        assert!(matches!(events.last(), Some(Event::StepFinished { .. })));
        assert_eq!(handle.status().percent, 100);
    }

    #[test]
    fn multi_pass_without_zero_leaves_the_last_pattern() {
        let mut disk = MemoryDisk::new(512, BLOCKS, 0x11);
        let prefs = Preferences {
            host_passes: 3,
            host_final_zero: false,
            ..Preferences::default()
        };
        let mut plan = plan_for(&caps(false, true), prefs);
        small_sampling(&mut plan);
        let report = run_plan(
            &mut disk,
            &plan,
            &seed(),
            &JobHandle::new(),
            &mut Vec::new(),
        );
        assert_eq!(report.outcome, Outcome::Success, "{report:?}");
        let mut expected = vec![0u8; 512];
        canary::fill_pass_block(&seed(), 2, 7, 512, &mut expected);
        assert_eq!(&disk.data()[7 * 512..8 * 512], &expected[..]);
    }

    #[test]
    fn lazy_controller_that_drops_writes_fails_on_pattern_mismatch() {
        let mut disk = MemoryDisk::new(512, BLOCKS, 0x11);
        disk.drop_writes = true;
        let mut plan = plan_for(&caps(false, true), Preferences::default());
        small_sampling(&mut plan);
        let report = run_plan(
            &mut disk,
            &plan,
            &seed(),
            &JobHandle::new(),
            &mut Vec::new(),
        );
        assert_eq!(report.outcome, Outcome::Failed);
        assert_eq!(
            report.steps[0].criterion.as_deref(),
            Some("PatternMismatch")
        );
        assert!(report.steps[0].validation.as_ref().unwrap().anomalies > 0);
    }

    #[test]
    fn honest_firmware_passes_lying_firmware_falls_back_to_host() {
        let mut plan = plan_for(&caps(true, false), Preferences::default());
        small_sampling(&mut plan);
        assert_eq!(plan.primary.method, EraseMethod::NvmeSanitizeBlockErase);
        assert!(plan.primary.verification.write_canaries);

        let mut honest = MemoryDisk::new(512, BLOCKS, 0x42);
        let report = run_plan(
            &mut honest,
            &plan,
            &seed(),
            &JobHandle::new(),
            &mut Vec::new(),
        );
        assert_eq!(report.outcome, Outcome::Success, "{report:?}");
        assert_eq!(report.achieved_level.as_deref(), Some("Purge"));
        assert_eq!(report.steps.len(), 1);
        assert!(honest.writes >= 100, "les canaris ont été écrits");

        let mut liar = MemoryDisk::new(512, BLOCKS, 0x42);
        liar.firmware = FakeFirmware::Ignores;
        let mut events = Vec::new();
        let report = run_plan(&mut liar, &plan, &seed(), &JobHandle::new(), &mut events);
        assert_eq!(report.outcome, Outcome::Success, "{report:?}");
        assert_eq!(report.achieved_level.as_deref(), Some("Clear"));
        assert_eq!(
            report.steps[0].criterion.as_deref(),
            Some("CanaryStillPresent")
        );
        // Tous les firmware menteurs sont tentés avant l'écriture hôte.
        let last = report.steps.last().unwrap();
        assert_eq!(last.outcome, Outcome::Success);
        assert!(last.method.starts_with("HOST_OVERWRITE"));
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Log { message } if message.contains("repli"))));
    }

    #[test]
    fn partial_firmware_erase_is_caught_by_the_sample() {
        let mut plan = plan_for(&caps(false, false), Preferences::default());
        small_sampling(&mut plan);
        let mut disk = MemoryDisk::new(512, BLOCKS, 0x42);
        disk.firmware = FakeFirmware::Partial { keep_every: 2 };
        let report = run_plan(
            &mut disk,
            &plan,
            &seed(),
            &JobHandle::new(),
            &mut Vec::new(),
        );
        assert_eq!(
            report.steps[0].criterion.as_deref(),
            Some("CanaryStillPresent")
        );
        assert_eq!(report.outcome, Outcome::Success, "repli hôte");
    }

    #[test]
    fn firmware_failure_status_and_read_errors_are_reported() {
        let mut plan = plan_for(
            &caps(true, false),
            Preferences {
                allow_host_fallback: false,
                ..Preferences::default()
            },
        );
        small_sampling(&mut plan);
        let mut disk = MemoryDisk::new(512, BLOCKS, 0x42);
        disk.firmware = FakeFirmware::Fails;
        let report = run_plan(
            &mut disk,
            &plan,
            &seed(),
            &JobHandle::new(),
            &mut Vec::new(),
        );
        assert_eq!(report.outcome, Outcome::Failed);
        assert!(report
            .steps
            .iter()
            .all(|s| s.criterion.as_deref() == Some("FirmwareReportedFailure")));

        let mut unreadable = MemoryDisk::new(512, BLOCKS, 0x42);
        unreadable.unreadable.insert(0);
        let report = run_plan(
            &mut unreadable,
            &plan,
            &seed(),
            &JobHandle::new(),
            &mut Vec::new(),
        );
        assert_eq!(report.outcome, Outcome::Failed);
        assert_eq!(
            report.steps[0].criterion.as_deref(),
            Some("SampleReadError")
        );
        assert_eq!(
            report.steps[0].validation.as_ref().unwrap().read_errors,
            [0]
        );
    }

    #[test]
    fn residual_gpt_after_a_firmware_erase_fails_the_step() {
        // Firmware « Ignores » mais sans canari survivant : signatures seules.
        let mut plan = plan_for(
            &caps(true, false),
            Preferences {
                allow_host_fallback: false,
                ..Preferences::default()
            },
        );
        small_sampling(&mut plan);
        plan.primary.verification.write_canaries = false;
        plan.fallbacks.clear();
        let mut disk = MemoryDisk::new(512, BLOCKS, 0);
        with_gpt(&mut disk);
        disk.firmware = FakeFirmware::Ignores;
        let report = run_plan(
            &mut disk,
            &plan,
            &seed(),
            &JobHandle::new(),
            &mut Vec::new(),
        );
        assert_eq!(report.outcome, Outcome::Failed);
        assert_eq!(
            report.steps[0].criterion.as_deref(),
            Some("SignatureStillPresent")
        );
        assert!(report.steps[0]
            .signatures
            .contains(&"PTTYPE=gpt".to_string()));
    }

    #[test]
    fn cancellation_stops_between_chunks_and_is_reported() {
        let mut disk = MemoryDisk::new(512, BLOCKS, 0x11);
        let mut plan = plan_for(&caps(false, true), Preferences::default());
        small_sampling(&mut plan);
        let handle = JobHandle::new();
        assert!(handle.request_cancel());
        let report = run_plan(&mut disk, &plan, &seed(), &handle, &mut Vec::new());
        assert_eq!(report.outcome, Outcome::Cancelled);
        assert!(report.steps.is_empty() || report.steps[0].outcome == Outcome::Cancelled);
        assert!(disk.writes == 0);
    }

    #[test]
    fn prepare_canaries_writes_and_counts_them() {
        let mut disk = MemoryDisk::new(512, BLOCKS, 0x11);
        let plan = {
            let mut p = plan_for(&caps(true, false), Preferences::default());
            small_sampling(&mut p);
            p
        };
        let sample = sample_for(&disk, &seed(), &plan.primary).unwrap();
        let handle = JobHandle::new();
        let mut sink = Vec::new();
        let mut phase = Phase::new(&handle, &mut sink, 0, "x", "canaries", 0);
        write_canaries(&mut disk, &seed(), &sample, &mut phase).unwrap();
        assert_eq!(
            count_canaries_present(&mut disk, &seed(), &sample).unwrap(),
            sample.len()
        );
        assert_eq!(disk.flushes, 1);
        disk.data_fill_zero();
        assert_eq!(
            count_canaries_present(&mut disk, &seed(), &sample).unwrap(),
            0
        );
    }

    impl MemoryDisk {
        fn data_fill_zero(&mut self) {
            let len = self.data().len();
            self.poke(0, &vec![0u8; len]);
        }
    }

    #[test]
    fn job_handle_tracks_cancel_and_report() {
        let handle = JobHandle::new();
        assert!(!handle.is_cancelled() && !handle.is_finished());
        handle.set_cancellable(false);
        assert!(!handle.request_cancel());
        assert!(handle.is_cancelled());
        assert!(handle.report().is_none());
        handle.finish(serde_json::json!({"outcome": "failed"}));
        assert!(handle.is_finished());
        assert_eq!(handle.report().unwrap()["outcome"], "failed");
    }
}
