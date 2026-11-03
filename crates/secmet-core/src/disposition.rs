//! Sort du support à l'issue de l'opération : le moteur dit « non ».
//!
//! Le plan ([`crate::engine::ErasePlan`]) décrit ce qu'on va tenter ; le
//! runner rend ce qui s'est passé étape par étape. Ce module en tire une
//! conclusion que l'opérateur, le certificat et le lot peuvent porter :
//!
//! | Disposition           | Sens                                                              |
//! |-----------------------|-------------------------------------------------------------------|
//! | `Reusable`            | effacé et vérifié ; réemploi possible                             |
//! | `DestroyRecommended`  | effacé et vérifié, mais le support ne mérite plus confiance        |
//! | `DestroyRequired`     | non effacé et plus aucune voie logicielle : données peut-être présentes |
//! | `Retry`               | non effacé pour une cause étrangère au support : relancer après la consigne |
//!
//! Règles, dans l'ordre :
//! 1. un support refusé parce que son média est en lecture seule (NVMe) ne
//!    sera jamais effacé par logiciel : destruction requise ;
//! 2. un échec avec **preuve matérielle** — secteurs illisibles à la
//!    relecture, écritures acquittées mais non persistantes, firmware
//!    acquittant sans effet sans qu'aucune écriture hôte n'ait abouti — est
//!    terminal : destruction requise ;
//! 3. un échec sur un support dont la santé recommandait déjà la destruction
//!    est terminal aussi : relancer n'apporterait rien ;
//! 4. tout autre échec (arrêt, erreur de liaison, firmware refusant sans
//!    repli abouti) se relance, avec les consignes qui auraient rendu
//!    d'autres méthodes disponibles ;
//! 5. un succès porte les réserves de santé du plan, plus ce que l'exécution
//!    a appris : firmware menteur contourné par écriture hôte, écriture hôte
//!    (*Clear*) sur un disque à secteurs réalloués que cette écriture
//!    n'atteint pas.
//!
//! La destruction n'est jamais exécutée par le logiciel (règle 2 de la crate).

use crate::engine::{
    Blocker, DestroyReason, ErasePlan, FailureCriterion, Guidance, RefusalReason, Verdict,
};
use crate::taxonomy::{EraseMethod, NistLevel};

/// Sort du support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Disposition {
    /// Effacé et vérifié : réemploi possible.
    Reusable,
    /// Effacé et vérifié ; destruction physique recommandée.
    DestroyRecommended,
    /// Non effacé, aucune voie logicielle restante ; destruction physique requise.
    DestroyRequired,
    /// Non effacé, cause étrangère au support ; à relancer.
    Retry,
}

impl Disposition {
    /// Code stable (certificat, journaux, lot).
    pub const fn code(self) -> &'static str {
        match self {
            Self::Reusable => "reusable",
            Self::DestroyRecommended => "destroy_recommended",
            Self::DestroyRequired => "destroy_required",
            Self::Retry => "retry",
        }
    }

    /// Lecture d'un code produit par [`Self::code`].
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "reusable" => Some(Self::Reusable),
            "destroy_recommended" => Some(Self::DestroyRecommended),
            "destroy_required" => Some(Self::DestroyRequired),
            "retry" => Some(Self::Retry),
            _ => None,
        }
    }

    /// Le support doit quitter le circuit de réemploi.
    pub const fn destroy(self) -> bool {
        matches!(self, Self::DestroyRecommended | Self::DestroyRequired)
    }

    /// Une nouvelle tentative a un sens.
    pub const fn retryable(self) -> bool {
        matches!(self, Self::Retry)
    }
}

/// Pourquoi ce sort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DispositionReason {
    /// Santé du support (SMART, page Health NVMe), connue avant l'effacement.
    Health(DestroyReason),
    /// Secteurs illisibles à la relecture : des données peuvent y subsister
    /// sans qu'aucune écriture ne les atteigne.
    UnreadableSectors,
    /// Écritures acquittées mais non persistantes (motif absent, non remis à
    /// zéro ou signatures survivantes après écriture hôte).
    WritesNotPersisted,
    /// Commande firmware acquittée sans effet (canari survivant) : le
    /// sanitize de ce disque ne mérite plus confiance.
    FirmwareIneffective,
    /// Média NVMe en lecture seule : aucune écriture ne sera acceptée.
    ReadOnlyMedia,
    /// Verrou ATA Security sans mot de passe connu : décision de l'opérateur.
    LockedWithoutPassword,
    /// Écriture hôte (*Clear*) sur un disque à secteurs réalloués : ces
    /// secteurs échappent à l'écriture.
    ReallocatedNotOverwritten,
    /// Arrêt demandé par l'opérateur.
    Cancelled,
    /// Erreur d'E/S hors critère du plan (liaison, descripteur) : relancer.
    IoError,
    /// Le firmware a rendu un échec (statut, sense, mode Failure, commande
    /// non attestée) et aucun repli n'a abouti.
    FirmwareFailure,
    /// Aucune méthode exécutable (inspection impossible, frozen, pont USB,
    /// repli hôte interdit).
    NoMethod,
    /// Blocage levable (verrou, compteur expiré, sanitize en cours ou en échec).
    Blocked,
    /// Cible protégée (système, montée, swap, membre d'un volume).
    Protected,
}

impl DispositionReason {
    /// Code stable : nom de la variante, raison de santé à plat
    /// (`SmartFailing`, `UnreadableSectors`…).
    pub fn code(self) -> String {
        match self {
            Self::Health(reason) => format!("{reason:?}"),
            other => format!("{other:?}"),
        }
    }
}

/// Verdict final : sort, raisons triées, consignes dédoublonnées.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FinalVerdict {
    /// Sort du support.
    pub disposition: Disposition,
    /// Raisons, triées, sans doublon.
    pub reasons: Vec<DispositionReason>,
    /// Consignes utiles avant de relancer (vides hors `Retry`).
    pub guidance: Vec<Guidance>,
}

impl FinalVerdict {
    fn new(disposition: Disposition, mut reasons: Vec<DispositionReason>) -> Self {
        reasons.sort();
        reasons.dedup();
        Self {
            disposition,
            reasons,
            guidance: Vec::new(),
        }
    }

    fn with_guidance(mut self, mut guidance: Vec<Guidance>) -> Self {
        guidance.retain(|g| *g != Guidance::None);
        guidance.sort();
        guidance.dedup();
        self.guidance = guidance;
        self
    }

    /// Décision de l'opérateur : un support bloqué (verrou ATA sans mot de
    /// passe) ou refusé est sorti du circuit, à détruire.
    pub fn operator_destroy(reason: DispositionReason) -> Self {
        Self::new(Disposition::DestroyRequired, vec![reason])
    }

    /// Arrêt demandé avant la première étape.
    pub fn cancelled() -> Self {
        Self::new(Disposition::Retry, vec![DispositionReason::Cancelled])
    }

    /// Codes des raisons, dans l'ordre.
    pub fn reason_codes(&self) -> Vec<String> {
        self.reasons.iter().map(|r| r.code()).collect()
    }

    /// Codes des consignes, dans l'ordre.
    pub fn guidance_codes(&self) -> Vec<String> {
        self.guidance.iter().map(|g| format!("{g:?}")).collect()
    }
}

/// Issue d'une étape, vue par le moteur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepOutcome {
    /// Validée.
    Success,
    /// Arrêtée par l'opérateur.
    Cancelled,
    /// Un critère violé, ou (`None`) une erreur d'E/S hors critère.
    Failed(Option<FailureCriterion>),
}

/// Trace d'une étape exécutée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StepTrace {
    /// Méthode exécutée.
    pub method: EraseMethod,
    /// Issue.
    pub outcome: StepOutcome,
}

impl StepTrace {
    fn failed_on(self, criterion: FailureCriterion) -> bool {
        matches!(self.outcome, StepOutcome::Failed(Some(c)) if c == criterion)
    }

    const fn is_host(self) -> bool {
        matches!(self.method, EraseMethod::HostOverwrite { .. })
    }
}

/// Verdict d'un support que le moteur n'a pas laissé partir
/// ([`Verdict::Refused`] ou [`Verdict::Blocked`]).
pub fn classify_unrunnable(verdict: &Verdict) -> FinalVerdict {
    match verdict {
        Verdict::Refused { reasons } => {
            if reasons.contains(&RefusalReason::ReadOnlyMedia) {
                return FinalVerdict::new(
                    Disposition::DestroyRequired,
                    vec![DispositionReason::ReadOnlyMedia],
                );
            }
            let mapped = reasons.iter().map(|r| match r {
                RefusalReason::HostsSystem
                | RefusalReason::Mounted
                | RefusalReason::ActiveSwap
                | RefusalReason::VolumeMember => DispositionReason::Protected,
                RefusalReason::ReadOnlyMedia => DispositionReason::ReadOnlyMedia,
                RefusalReason::InspectionFailedNoFallback | RefusalReason::NoMethodAvailable => {
                    DispositionReason::NoMethod
                }
            });
            FinalVerdict::new(Disposition::Retry, mapped.collect())
        }
        Verdict::Blocked { reasons, guidance } => {
            let locked = reasons.iter().any(|b| {
                matches!(
                    b,
                    Blocker::SecurityLocked | Blocker::PasswordAttemptsExpired
                )
            });
            let mut mapped = vec![DispositionReason::Blocked];
            if locked {
                mapped.push(DispositionReason::LockedWithoutPassword);
            }
            FinalVerdict::new(Disposition::Retry, mapped).with_guidance(guidance.clone())
        }
        Verdict::Ready(_) => FinalVerdict::new(Disposition::Retry, Vec::new()),
    }
}

/// Verdict d'un plan exécuté : `steps` dans l'ordre d'exécution, la
/// dernière trace portant l'issue du job (succès, arrêt ou dernier échec).
pub fn classify_run(plan: &ErasePlan, steps: &[StepTrace]) -> FinalVerdict {
    let health: Vec<DispositionReason> = plan
        .recommend_destroy
        .iter()
        .map(|r| DispositionReason::Health(*r))
        .collect();
    let last = steps.last().map(|s| s.outcome);
    match last {
        Some(StepOutcome::Success) => classify_success(plan, steps, health),
        Some(StepOutcome::Cancelled) => {
            FinalVerdict::new(Disposition::Retry, vec![DispositionReason::Cancelled])
        }
        Some(StepOutcome::Failed(_)) | None => classify_failure(plan, steps, health),
    }
}

fn classify_success(
    plan: &ErasePlan,
    steps: &[StepTrace],
    mut reasons: Vec<DispositionReason>,
) -> FinalVerdict {
    let winner = steps.last().copied();
    if steps
        .iter()
        .any(|s| !s.is_host() && s.failed_on(FailureCriterion::CanaryStillPresent))
    {
        reasons.push(DispositionReason::FirmwareIneffective);
    }
    let level = winner.and_then(|w| plan.steps().find(|p| p.method == w.method).map(|p| p.level));
    let reallocated = plan.health.reallocated_sectors.unwrap_or(0) > 0;
    if winner.is_some_and(|w| w.is_host())
        && level == Some(NistLevel::Clear)
        && reallocated
        && !reasons.contains(&DispositionReason::Health(
            DestroyReason::ReallocatedAboveThreshold,
        ))
    {
        reasons.push(DispositionReason::ReallocatedNotOverwritten);
    }
    let disposition = if reasons.is_empty() {
        Disposition::Reusable
    } else {
        Disposition::DestroyRecommended
    };
    FinalVerdict::new(disposition, reasons)
}

fn classify_failure(
    plan: &ErasePlan,
    steps: &[StepTrace],
    health: Vec<DispositionReason>,
) -> FinalVerdict {
    let mut evidence = Vec::new();
    if steps
        .iter()
        .any(|s| s.failed_on(FailureCriterion::SampleReadError))
    {
        evidence.push(DispositionReason::UnreadableSectors);
    }
    if steps.iter().any(|s| {
        s.is_host()
            && (s.failed_on(FailureCriterion::PatternMismatch)
                || s.failed_on(FailureCriterion::NotZeroed)
                || s.failed_on(FailureCriterion::SignatureStillPresent))
    }) {
        evidence.push(DispositionReason::WritesNotPersisted);
    }
    let firmware_lied = steps
        .iter()
        .any(|s| !s.is_host() && s.failed_on(FailureCriterion::CanaryStillPresent));
    let host_succeeded = steps
        .iter()
        .any(|s| s.is_host() && s.outcome == StepOutcome::Success);
    if firmware_lied && !host_succeeded {
        evidence.push(DispositionReason::FirmwareIneffective);
    }
    if !evidence.is_empty() || !health.is_empty() {
        let mut reasons = evidence;
        reasons.extend(health);
        return FinalVerdict::new(Disposition::DestroyRequired, reasons);
    }

    let mut reasons = Vec::new();
    if steps.iter().any(|s| {
        s.failed_on(FailureCriterion::FirmwareReportedFailure)
            || s.failed_on(FailureCriterion::SanitizeNotCompleted)
            || s.failed_on(FailureCriterion::CommandNotAttested)
            || s.failed_on(FailureCriterion::SecurityStillEnabled)
    }) {
        reasons.push(DispositionReason::FirmwareFailure);
    }
    if steps.iter().any(|s| s.outcome == StepOutcome::Failed(None)) {
        reasons.push(DispositionReason::IoError);
    }
    if steps.is_empty() {
        reasons.push(DispositionReason::NoMethod);
    }
    let guidance: Vec<Guidance> = plan.excluded.iter().map(|e| e.reason.guidance()).collect();
    FinalVerdict::new(Disposition::Retry, reasons).with_guidance(guidance)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::engine::{decide, ExclusionReason, Preferences};
    use crate::taxonomy::{
        AtaCaps, HealthFacts, MediaType, NvmeCaps, Policy, ProtectionFlags, SanitizeState,
        TargetCapabilities, Transport,
    };

    fn base(media: MediaType, transport: Transport) -> TargetCapabilities {
        TargetCapabilities {
            name: "sdz".into(),
            serial: "SN-TEST-0001".into(),
            media,
            transport,
            block_size: 512,
            block_count: 1_000_000,
            nvme: None,
            ata: None,
            protection: ProtectionFlags::default(),
            health: HealthFacts::default(),
            inspection_failed: false,
        }
    }

    fn sata_ssd_sanitize() -> TargetCapabilities {
        TargetCapabilities {
            ata: Some(AtaCaps {
                sanitize_supported: true,
                sanitize_block_erase: true,
                ..AtaCaps::default()
            }),
            ..base(MediaType::Ssd, Transport::Sata)
        }
    }

    fn sata_hdd() -> TargetCapabilities {
        base(MediaType::Hdd, Transport::Sata)
    }

    fn plan(target: &TargetCapabilities) -> ErasePlan {
        match decide(target, &Policy::default(), &Preferences::default()).unwrap() {
            Verdict::Ready(plan) => *plan,
            other => panic!("plan attendu, obtenu {other:?}"),
        }
    }

    fn host() -> EraseMethod {
        EraseMethod::HostOverwrite {
            passes: 1,
            final_zero: true,
        }
    }

    fn trace(method: EraseMethod, outcome: StepOutcome) -> StepTrace {
        StepTrace { method, outcome }
    }

    #[test]
    fn clean_success_is_reusable() {
        let p = plan(&sata_hdd());
        let v = classify_run(&p, &[trace(host(), StepOutcome::Success)]);
        assert_eq!(v.disposition, Disposition::Reusable);
        assert!(v.reasons.is_empty());
        assert!(v.guidance.is_empty());
    }

    #[test]
    fn success_on_a_dying_disk_recommends_destruction() {
        let mut target = sata_hdd();
        target.health.smart_failing = true;
        let p = plan(&target);
        let v = classify_run(&p, &[trace(host(), StepOutcome::Success)]);
        assert_eq!(v.disposition, Disposition::DestroyRecommended);
        assert_eq!(
            v.reasons,
            [DispositionReason::Health(DestroyReason::SmartFailing)]
        );
        assert_eq!(v.reason_codes(), ["SmartFailing"]);
    }

    #[test]
    fn host_clear_on_reallocated_sectors_recommends_destruction() {
        let mut target = sata_hdd();
        target.health.reallocated_sectors = Some(3);
        let p = plan(&target);
        assert!(
            p.recommend_destroy.is_empty(),
            "sous le seuil de la politique"
        );
        let v = classify_run(&p, &[trace(host(), StepOutcome::Success)]);
        assert_eq!(v.disposition, Disposition::DestroyRecommended);
        assert_eq!(v.reasons, [DispositionReason::ReallocatedNotOverwritten]);
    }

    #[test]
    fn lying_firmware_rescued_by_host_overwrite_is_still_suspect() {
        let p = plan(&sata_ssd_sanitize());
        let steps = [
            trace(
                EraseMethod::AtaSanitizeBlockErase,
                StepOutcome::Failed(Some(FailureCriterion::CanaryStillPresent)),
            ),
            trace(host(), StepOutcome::Success),
        ];
        let v = classify_run(&p, &steps);
        assert_eq!(v.disposition, Disposition::DestroyRecommended);
        assert_eq!(v.reasons, [DispositionReason::FirmwareIneffective]);
    }

    #[test]
    fn unreadable_sectors_require_destruction() {
        let p = plan(&sata_hdd());
        let v = classify_run(
            &p,
            &[trace(
                host(),
                StepOutcome::Failed(Some(FailureCriterion::SampleReadError)),
            )],
        );
        assert_eq!(v.disposition, Disposition::DestroyRequired);
        assert_eq!(v.reasons, [DispositionReason::UnreadableSectors]);
        assert!(!v.disposition.retryable());
        assert!(v.disposition.destroy());
    }

    #[test]
    fn writes_that_do_not_persist_require_destruction() {
        let p = plan(&sata_hdd());
        for criterion in [
            FailureCriterion::PatternMismatch,
            FailureCriterion::NotZeroed,
            FailureCriterion::SignatureStillPresent,
        ] {
            let v = classify_run(&p, &[trace(host(), StepOutcome::Failed(Some(criterion)))]);
            assert_eq!(v.disposition, Disposition::DestroyRequired, "{criterion:?}");
            assert_eq!(v.reasons, [DispositionReason::WritesNotPersisted]);
        }
    }

    #[test]
    fn lying_firmware_without_working_fallback_requires_destruction() {
        let p = plan(&sata_ssd_sanitize());
        let steps = [
            trace(
                EraseMethod::AtaSanitizeBlockErase,
                StepOutcome::Failed(Some(FailureCriterion::CanaryStillPresent)),
            ),
            trace(host(), StepOutcome::Failed(None)),
        ];
        let v = classify_run(&p, &steps);
        assert_eq!(v.disposition, Disposition::DestroyRequired);
        assert_eq!(v.reasons, [DispositionReason::FirmwareIneffective]);
    }

    #[test]
    fn failure_on_a_disk_already_condemned_by_smart_is_terminal() {
        let mut target = sata_hdd();
        target.health.pending_sectors = Some(12);
        let p = plan(&target);
        let v = classify_run(&p, &[trace(host(), StepOutcome::Failed(None))]);
        assert_eq!(v.disposition, Disposition::DestroyRequired);
        assert_eq!(
            v.reasons,
            [DispositionReason::Health(
                DestroyReason::PendingOrUncorrectable
            )]
        );
    }

    #[test]
    fn transient_failures_are_retried_with_the_plan_guidance() {
        let mut target = sata_ssd_sanitize();
        target.transport = Transport::Usb;
        let p = plan(&target);
        assert!(p
            .excluded
            .iter()
            .any(|e| e.reason == ExclusionReason::UsbBridge));
        let v = classify_run(&p, &[trace(host(), StepOutcome::Failed(None))]);
        assert_eq!(v.disposition, Disposition::Retry);
        assert_eq!(v.reasons, [DispositionReason::IoError]);
        assert_eq!(v.guidance, [Guidance::AttachNatively]);
        assert_eq!(v.guidance_codes(), ["AttachNatively"]);
    }

    #[test]
    fn firmware_refusal_then_io_error_is_retried() {
        let p = plan(&sata_ssd_sanitize());
        let steps = [
            trace(
                EraseMethod::AtaSanitizeBlockErase,
                StepOutcome::Failed(Some(FailureCriterion::FirmwareReportedFailure)),
            ),
            trace(host(), StepOutcome::Failed(None)),
        ];
        let v = classify_run(&p, &steps);
        assert_eq!(v.disposition, Disposition::Retry);
        assert_eq!(
            v.reasons,
            [
                DispositionReason::IoError,
                DispositionReason::FirmwareFailure
            ]
        );
    }

    #[test]
    fn cancellation_is_retried() {
        let p = plan(&sata_hdd());
        let v = classify_run(&p, &[trace(host(), StepOutcome::Cancelled)]);
        assert_eq!(v.disposition, Disposition::Retry);
        assert_eq!(v.reasons, [DispositionReason::Cancelled]);
    }

    #[test]
    fn no_step_at_all_is_retried_as_no_method() {
        let p = plan(&sata_hdd());
        let v = classify_run(&p, &[]);
        assert_eq!(v.disposition, Disposition::Retry);
        assert_eq!(v.reasons, [DispositionReason::NoMethod]);
    }

    #[test]
    fn read_only_media_requires_destruction_before_any_attempt() {
        let target = TargetCapabilities {
            nvme: Some(NvmeCaps {
                read_only: true,
                ..NvmeCaps::default()
            }),
            ..base(MediaType::Ssd, Transport::Nvme)
        };
        let verdict = decide(&target, &Policy::default(), &Preferences::default()).unwrap();
        let v = classify_unrunnable(&verdict);
        assert_eq!(v.disposition, Disposition::DestroyRequired);
        assert_eq!(v.reasons, [DispositionReason::ReadOnlyMedia]);
    }

    #[test]
    fn protected_and_blocked_targets_are_retried_with_guidance() {
        let mut mounted = sata_hdd();
        mounted.protection.mounted = true;
        let verdict = decide(&mounted, &Policy::default(), &Preferences::default()).unwrap();
        let v = classify_unrunnable(&verdict);
        assert_eq!(v.disposition, Disposition::Retry);
        assert_eq!(v.reasons, [DispositionReason::Protected]);

        let locked = TargetCapabilities {
            ata: Some(AtaCaps {
                security_locked: true,
                sanitize_state: SanitizeState::Idle,
                ..AtaCaps::default()
            }),
            ..sata_hdd()
        };
        let verdict = decide(&locked, &Policy::default(), &Preferences::default()).unwrap();
        let v = classify_unrunnable(&verdict);
        assert_eq!(v.disposition, Disposition::Retry);
        assert_eq!(
            v.reasons,
            [
                DispositionReason::LockedWithoutPassword,
                DispositionReason::Blocked
            ]
        );
        assert_eq!(v.guidance, [Guidance::UnlockWithPassword]);
    }

    #[test]
    fn operator_can_condemn_a_locked_disk() {
        let v = FinalVerdict::operator_destroy(DispositionReason::LockedWithoutPassword);
        assert_eq!(v.disposition, Disposition::DestroyRequired);
        assert_eq!(v.reason_codes(), ["LockedWithoutPassword"]);
    }

    #[test]
    fn codes_round_trip() {
        for d in [
            Disposition::Reusable,
            Disposition::DestroyRecommended,
            Disposition::DestroyRequired,
            Disposition::Retry,
        ] {
            assert_eq!(Disposition::from_code(d.code()), Some(d));
        }
        assert_eq!(Disposition::from_code("destroy"), None);
    }
}
