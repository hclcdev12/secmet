//! Moteur de décision déterministe.
//!
//! Étapes, dans l'ordre, la première non verte l'emporte :
//! 1. **protections** : cible système, montée, swap ou membre d'un volume → refus ;
//! 2. **refus définitifs** : inspection impossible sans repli, média en lecture seule ;
//! 3. **blocages levables** : verrouillé, compteur expiré, sanitize en cours
//!    ou en échec → bloqué, avec la consigne pour débloquer ;
//! 4. **choix de la méthode** : les candidats firmware *Purge* d'abord, puis
//!    les firmware *Clear*, puis l'écriture hôte ; une méthode exclue par
//!    l'état du disque (frozen, mot de passe tiers, pont USB) est consignée
//!    dans le plan avec sa raison ;
//! 5. **dégradation** : si la meilleure méthode retenue est *Clear* alors
//!    qu'un *Purge* existait sur le papier, le plan le dit, avec la consigne
//!    qui aurait permis le *Purge* (dé-frozen, retrait du mot de passe…).
//!
//! Même cible, même politique, mêmes préférences → même plan, toujours.

use crate::canary::{SamplingRules, VerificationSpec, VerificationTarget};
use crate::taxonomy::{
    EraseMethod, NistLevel, Policy, SanitizeState, TargetCapabilities, Transport,
};
use crate::{CoreError, Result};

/// Préférences de l'opérateur, bornées par la politique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Preferences {
    /// Autoriser le repli vers une écriture hôte (*Clear*) quand aucune
    /// commande firmware n'est utilisable.
    pub allow_host_fallback: bool,
    /// Passes pseudo-aléatoires de l'écriture hôte (1 à 16).
    pub host_passes: u8,
    /// Terminer l'écriture hôte par une passe de zéros validée.
    pub host_final_zero: bool,
    /// Envoyer des commandes firmware à travers un pont USB. Par défaut non :
    /// les ponts acquittent parfois sans transmettre.
    pub firmware_over_usb: bool,
    /// Préférer un effacement cryptographique (rapide) à un block erase
    /// (vérifiable à la lecture) quand les deux sont *Purge*.
    pub prefer_cryptographic: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            allow_host_fallback: true,
            host_passes: 1,
            host_final_zero: true,
            firmware_over_usb: false,
            prefer_cryptographic: false,
        }
    }
}

/// Pourquoi la cible est refusée, définitivement ou jusqu'à intervention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RefusalReason {
    /// Héberge le système.
    HostsSystem,
    /// Partition montée.
    Mounted,
    /// Swap actif.
    ActiveSwap,
    /// Membre d'un volume LVM / md / dm.
    VolumeMember,
    /// Média NVMe en lecture seule (Critical Warning bit 3).
    ReadOnlyMedia,
    /// Inspection firmware impossible et repli hôte interdit.
    InspectionFailedNoFallback,
    /// Aucune méthode utilisable, repli hôte interdit.
    NoMethodAvailable,
}

/// Blocage levable par l'opérateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Blocker {
    /// ATA Security verrouillé : mot de passe requis.
    SecurityLocked,
    /// Compteur de tentatives de mot de passe expiré.
    PasswordAttemptsExpired,
    /// Un sanitize est déjà en cours.
    SanitizeInProgress,
    /// Le dernier sanitize a échoué (mode Failure).
    SanitizeFailed,
}

impl Blocker {
    /// Consigne pour lever le blocage.
    pub const fn guidance(self) -> Guidance {
        match self {
            Self::SecurityLocked => Guidance::UnlockWithPassword,
            Self::PasswordAttemptsExpired => Guidance::PowerCycle,
            Self::SanitizeInProgress => Guidance::WaitForSanitize,
            Self::SanitizeFailed => Guidance::ClearSanitizeFailure,
        }
    }
}

/// Consigne opérateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Guidance {
    /// Déverrouiller avec le mot de passe utilisateur ou maître, puis le retirer.
    UnlockWithPassword,
    /// Couper puis remettre l'alimentation du disque.
    PowerCycle,
    /// Attendre la fin du sanitize en cours.
    WaitForSanitize,
    /// Sortir du mode Failure (SANITIZE STATUS EXT avec CLEAR, ou Exit Failure Mode NVMe).
    ClearSanitizeFailure,
    /// Dé-frozen par suspend-to-RAM puis réinspection.
    SuspendToRam,
    /// Retirer le mot de passe ATA posé par un tiers.
    RemoveForeignPassword,
    /// Brancher le disque en SATA natif plutôt que sur un pont USB.
    AttachNatively,
    /// Aucune : la cible ne propose rien de mieux.
    None,
}

/// Pourquoi une méthode candidate a été écartée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExclusionReason {
    /// SECURITY FREEZE LOCK actif.
    SecurityFrozen,
    /// Un mot de passe est déjà posé par un tiers (jamais écrasé).
    ForeignPassword,
    /// Sanitize Freeze Lock actif.
    SanitizeFrozen,
    /// Pont USB et `firmware_over_usb` faux.
    UsbBridge,
    /// Repli hôte interdit par les préférences.
    HostFallbackDisabled,
}

impl ExclusionReason {
    /// Consigne qui aurait rendu la méthode disponible.
    pub const fn guidance(self) -> Guidance {
        match self {
            Self::SecurityFrozen => Guidance::SuspendToRam,
            Self::ForeignPassword => Guidance::RemoveForeignPassword,
            Self::SanitizeFrozen => Guidance::PowerCycle,
            Self::UsbBridge => Guidance::AttachNatively,
            Self::HostFallbackDisabled => Guidance::None,
        }
    }
}

/// Méthode candidate écartée, avec sa raison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Excluded {
    /// La méthode.
    pub method: EraseMethod,
    /// Niveau qu'elle aurait eu.
    pub level: NistLevel,
    /// Pourquoi.
    pub reason: ExclusionReason,
}

/// Dégradation explicite du niveau visé.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Degradation {
    /// Niveau qu'une méthode écartée aurait atteint.
    pub from: NistLevel,
    /// Niveau du plan retenu.
    pub to: NistLevel,
    /// Méthodes écartées qui auraient donné `from`.
    pub lost: Vec<Excluded>,
    /// Consignes qui permettraient de remonter à `from`.
    pub guidance: Vec<Guidance>,
}

/// Critère dont la violation rend l'étape échouée (et le rapport NON CONFORME).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FailureCriterion {
    /// Le firmware n'a pas rendu un succès (statut, sense, mode Failure).
    FirmwareReportedFailure,
    /// Le journal sanitize ne revient pas à un état terminé avec succès.
    SanitizeNotCompleted,
    /// Un canari écrit avant l'effacement est encore lisible.
    CanaryStillPresent,
    /// Un bloc échantillonné ne contient pas le motif attendu de la passe.
    PatternMismatch,
    /// Un bloc échantillonné n'est pas à zéro après la passe de zéros.
    NotZeroed,
    /// Une lecture d'échantillon a échoué (secteur illisible).
    SampleReadError,
    /// Une signature de partition ou de système de fichiers subsiste.
    SignatureStillPresent,
    /// Après SECURITY ERASE UNIT, le mot de passe est encore actif.
    SecurityStillEnabled,
    /// Pour un effacement cryptographique : la commande n'est pas attestée
    /// (ex. acquittement sans statut) — un no-op silencieux est possible.
    CommandNotAttested,
}

/// Une étape du plan.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlannedStep {
    /// Méthode.
    pub method: EraseMethod,
    /// Niveau certifié si l'étape réussit.
    pub level: NistLevel,
    /// Ce qui fait échouer l'étape, dans l'ordre de vérification.
    pub failure_criteria: Vec<FailureCriterion>,
    /// Vérification post-étape.
    pub verification: VerificationSpec,
    /// L'étape porte sur tous les namespaces du contrôleur NVMe.
    pub whole_controller: bool,
    /// Durée estimée par le disque, en secondes, si connue (Security Erase).
    pub estimated_seconds: Option<u64>,
}

/// Limite connue du plan, à porter sur le certificat si l'étape concernée
/// est celle qui s'exécute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PlanNote {
    /// Le contrôleur NVMe expose plusieurs namespaces et au moins une étape
    /// ne porte que sur celui ouvert (Format NVM sans FNA bit 1, écriture
    /// hôte) : les autres namespaces restent à traiter.
    NvmeOtherNamespacesUntouched,
}

/// Raison de recommander la destruction physique après l'effacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DestroyReason {
    /// SMART prédit une panne.
    SmartFailing,
    /// Secteurs réalloués au-delà du seuil de la politique.
    ReallocatedAboveThreshold,
    /// Secteurs en attente ou non corrigibles : des zones peuvent être illisibles.
    PendingOrUncorrectable,
    /// NVMe : fiabilité dégradée.
    ReliabilityDegraded,
    /// NVMe : erreurs d'intégrité média.
    MediaErrors,
    /// NVMe : réserve sous le seuil.
    SpareBelowThreshold,
    /// NVMe : endurance consommée à 100 %.
    EnduranceExhausted,
}

/// Plan d'action complet.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ErasePlan {
    /// Identifiant de la politique appliquée.
    pub policy_id: &'static str,
    /// Niveau NIST visé par la première étape.
    pub target_level: NistLevel,
    /// Étape à exécuter en premier.
    pub primary: PlannedStep,
    /// Replis, dans l'ordre, si l'étape précédente échoue.
    pub fallbacks: Vec<PlannedStep>,
    /// Méthodes écartées et pourquoi.
    pub excluded: Vec<Excluded>,
    /// Dégradation explicite, si le visé est sous le meilleur niveau théorique.
    pub degradation: Option<Degradation>,
    /// Destruction physique recommandée après l'effacement.
    pub recommend_destroy: Vec<DestroyReason>,
    /// Limites connues du plan.
    pub notes: Vec<PlanNote>,
}

impl ErasePlan {
    /// Toutes les étapes, primaire comprise.
    pub fn steps(&self) -> impl Iterator<Item = &PlannedStep> {
        std::iter::once(&self.primary).chain(self.fallbacks.iter())
    }

    /// Niveau garanti quel que soit le repli exécuté (le plus bas des étapes).
    pub fn worst_level(&self) -> NistLevel {
        self.steps()
            .map(|s| s.level)
            .min()
            .unwrap_or(self.target_level)
    }
}

/// Verdict du moteur.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// Jamais effacée dans cet état.
    Refused {
        /// Raisons, triées.
        reasons: Vec<RefusalReason>,
    },
    /// Effaçable après intervention.
    Blocked {
        /// Blocages, triés.
        reasons: Vec<Blocker>,
        /// Consignes, dédoublonnées.
        guidance: Vec<Guidance>,
    },
    /// Plan prêt.
    Ready(Box<ErasePlan>),
}

/// Décide pour `target` sous `policy` et `preferences`.
pub fn decide(
    target: &TargetCapabilities,
    policy: &Policy,
    preferences: &Preferences,
) -> Result<Verdict> {
    if target.block_count == 0 {
        return Err(CoreError::NoBlocks);
    }
    if target.block_size == 0 || target.block_size % 4 != 0 {
        return Err(CoreError::InvalidBlockSize(target.block_size));
    }
    if !(1..=16).contains(&preferences.host_passes) {
        return Err(CoreError::PassesOutOfRange(preferences.host_passes));
    }

    // 1. Protections.
    let mut refusals = Vec::new();
    let p = target.protection;
    if p.hosts_system {
        refusals.push(RefusalReason::HostsSystem);
    }
    if p.mounted {
        refusals.push(RefusalReason::Mounted);
    }
    if p.swap {
        refusals.push(RefusalReason::ActiveSwap);
    }
    if p.held {
        refusals.push(RefusalReason::VolumeMember);
    }
    if !refusals.is_empty() {
        return Ok(Verdict::Refused { reasons: refusals });
    }

    // 2. Refus définitifs.
    if target.nvme.is_some_and(|n| n.read_only) {
        return Ok(Verdict::Refused {
            reasons: vec![RefusalReason::ReadOnlyMedia],
        });
    }
    if target.inspection_failed && !preferences.allow_host_fallback {
        return Ok(Verdict::Refused {
            reasons: vec![RefusalReason::InspectionFailedNoFallback],
        });
    }

    // 3. Blocages levables.
    let mut blockers = Vec::new();
    if let Some(ata) = target.ata {
        if ata.security_locked {
            blockers.push(Blocker::SecurityLocked);
        }
        if ata.security_attempts_expired {
            blockers.push(Blocker::PasswordAttemptsExpired);
        }
        match ata.sanitize_state {
            SanitizeState::InProgress => blockers.push(Blocker::SanitizeInProgress),
            SanitizeState::Failed => blockers.push(Blocker::SanitizeFailed),
            SanitizeState::Idle => {}
        }
    }
    if let Some(nvme) = target.nvme {
        match nvme.sanitize_state {
            SanitizeState::InProgress => blockers.push(Blocker::SanitizeInProgress),
            SanitizeState::Failed => blockers.push(Blocker::SanitizeFailed),
            SanitizeState::Idle => {}
        }
    }
    if !blockers.is_empty() {
        blockers.sort();
        blockers.dedup();
        let mut guidance: Vec<Guidance> = blockers.iter().map(|b| b.guidance()).collect();
        guidance.sort();
        guidance.dedup();
        return Ok(Verdict::Blocked {
            reasons: blockers,
            guidance,
        });
    }

    // 4. Candidats et exclusions.
    let (available, excluded) = candidates(target, policy, preferences);
    let Some(primary) = available.first().cloned() else {
        return Ok(Verdict::Refused {
            reasons: vec![RefusalReason::NoMethodAvailable],
        });
    };
    let fallbacks: Vec<PlannedStep> = available.into_iter().skip(1).collect();

    // 5. Dégradation.
    let best_excluded = excluded.iter().map(|e| e.level).max();
    let degradation = best_excluded
        .filter(|&best| best > primary.level)
        .map(|best| {
            let lost: Vec<Excluded> = excluded
                .iter()
                .copied()
                .filter(|e| e.level == best)
                .collect();
            let mut guidance: Vec<Guidance> = lost.iter().map(|e| e.reason.guidance()).collect();
            guidance.sort();
            guidance.dedup();
            Degradation {
                from: best,
                to: primary.level,
                lost,
                guidance,
            }
        });

    // Plusieurs namespaces NVMe : un sanitize couvre le contrôleur, mais un
    // Format NVM sans FNA bit 1 ou une écriture hôte ne touche que le
    // namespace ouvert. Le plan reste valable pour ce namespace ; la limite
    // est portée pour que l'opérateur traite les autres.
    let mut notes = Vec::new();
    if target.nvme.is_some_and(|n| n.multiple_namespaces)
        && std::iter::once(&primary)
            .chain(fallbacks.iter())
            .any(|s| !s.whole_controller)
    {
        notes.push(PlanNote::NvmeOtherNamespacesUntouched);
    }

    Ok(Verdict::Ready(Box::new(ErasePlan {
        policy_id: policy.id,
        target_level: primary.level,
        primary,
        fallbacks,
        excluded,
        degradation,
        recommend_destroy: destroy_reasons(target, policy),
        notes,
    })))
}

/// Candidats dans l'ordre de préférence, séparés en disponibles et écartés.
fn candidates(
    target: &TargetCapabilities,
    policy: &Policy,
    preferences: &Preferences,
) -> (Vec<PlannedStep>, Vec<Excluded>) {
    let mut available = Vec::new();
    let mut excluded = Vec::new();
    let usb_blocked = target.transport == Transport::Usb && !preferences.firmware_over_usb;

    let mut offer = |supported: bool, method: EraseMethod, exclusion: Option<ExclusionReason>| {
        if !supported {
            return;
        }
        let level = policy.level_of(method, target.media);
        let exclusion = exclusion.or(if usb_blocked && method.is_firmware() {
            Some(ExclusionReason::UsbBridge)
        } else {
            None
        });
        match exclusion {
            Some(reason) => excluded.push(Excluded {
                method,
                level,
                reason,
            }),
            None => available.push(step(target, policy, method, level)),
        }
    };

    if !target.inspection_failed {
        if let Some(nvme) = target.nvme {
            let (first, second) = if preferences.prefer_cryptographic {
                (
                    (
                        nvme.sanitize_crypto_erase,
                        EraseMethod::NvmeSanitizeCryptoErase,
                    ),
                    (
                        nvme.sanitize_block_erase,
                        EraseMethod::NvmeSanitizeBlockErase,
                    ),
                )
            } else {
                (
                    (
                        nvme.sanitize_block_erase,
                        EraseMethod::NvmeSanitizeBlockErase,
                    ),
                    (
                        nvme.sanitize_crypto_erase,
                        EraseMethod::NvmeSanitizeCryptoErase,
                    ),
                )
            };
            offer(first.0, first.1, None);
            offer(second.0, second.1, None);
            offer(
                nvme.format_supported && nvme.format_crypto_erase,
                EraseMethod::NvmeFormatCryptoErase,
                None,
            );
            offer(
                nvme.format_supported,
                EraseMethod::NvmeFormatUserDataErase,
                None,
            );
            offer(
                nvme.sanitize_overwrite,
                EraseMethod::NvmeSanitizeOverwrite { passes: 1 },
                None,
            );
        }
        if let Some(ata) = target.ata {
            let sanitize_exclusion = ata
                .sanitize_frozen
                .then_some(ExclusionReason::SanitizeFrozen);
            let security_exclusion = if ata.security_frozen {
                Some(ExclusionReason::SecurityFrozen)
            } else if ata.security_enabled {
                Some(ExclusionReason::ForeignPassword)
            } else {
                None
            };
            let sanitize = ata.sanitize_supported;
            if target.media.solid_state_rules() {
                let (first, second) = if preferences.prefer_cryptographic {
                    (
                        (
                            ata.sanitize_crypto_scramble,
                            EraseMethod::AtaSanitizeCryptoScramble,
                        ),
                        (ata.sanitize_block_erase, EraseMethod::AtaSanitizeBlockErase),
                    )
                } else {
                    (
                        (ata.sanitize_block_erase, EraseMethod::AtaSanitizeBlockErase),
                        (
                            ata.sanitize_crypto_scramble,
                            EraseMethod::AtaSanitizeCryptoScramble,
                        ),
                    )
                };
                offer(sanitize && first.0, first.1, sanitize_exclusion);
                offer(sanitize && second.0, second.1, sanitize_exclusion);
                // Jamais d'overwrite firmware sur un SSD : usure inutile, et
                // sans garantie sur l'over-provisioning.
            } else {
                offer(
                    sanitize && ata.sanitize_overwrite,
                    EraseMethod::AtaSanitizeOverwrite,
                    sanitize_exclusion,
                );
            }
            offer(
                ata.security_supported && ata.enhanced_erase_supported,
                EraseMethod::AtaSecurityEraseEnhanced,
                security_exclusion,
            );
            offer(
                ata.security_supported,
                EraseMethod::AtaSecurityEraseNormal,
                security_exclusion,
            );
        }
    }

    let host = EraseMethod::HostOverwrite {
        passes: preferences.host_passes,
        final_zero: preferences.host_final_zero,
    };
    offer(
        true,
        host,
        (!preferences.allow_host_fallback).then_some(ExclusionReason::HostFallbackDisabled),
    );

    // Purge d'abord, à stabilité d'ordre égale (l'ordre d'offre fait le reste).
    available.sort_by_key(|step| std::cmp::Reverse(step.level));
    (available, excluded)
}

fn step(
    target: &TargetCapabilities,
    policy: &Policy,
    method: EraseMethod,
    level: NistLevel,
) -> PlannedStep {
    let sampling = SamplingRules {
        random_samples: policy.validation_samples.max(policy.min_validation_samples),
        min_samples: policy.min_validation_samples,
    };
    let mut criteria = Vec::new();
    let verification = match method {
        EraseMethod::HostOverwrite { passes, final_zero } => {
            criteria.push(FailureCriterion::SampleReadError);
            criteria.push(FailureCriterion::PatternMismatch);
            if final_zero {
                criteria.push(FailureCriterion::NotZeroed);
            }
            criteria.push(FailureCriterion::SignatureStillPresent);
            VerificationSpec {
                sampling,
                target: if final_zero {
                    VerificationTarget::ZerosAfterPasses { passes }
                } else {
                    VerificationTarget::PatternOfPass { pass: passes - 1 }
                },
                write_canaries: false,
                require_no_signatures: true,
            }
        }
        firmware => {
            criteria.push(FailureCriterion::FirmwareReportedFailure);
            if firmware.is_ata_sanitize()
                || matches!(
                    firmware,
                    EraseMethod::NvmeSanitizeBlockErase
                        | EraseMethod::NvmeSanitizeCryptoErase
                        | EraseMethod::NvmeSanitizeOverwrite { .. }
                )
            {
                criteria.push(FailureCriterion::SanitizeNotCompleted);
            }
            if firmware.is_ata_security_erase() {
                criteria.push(FailureCriterion::SecurityStillEnabled);
            }
            if firmware.is_cryptographic() {
                criteria.push(FailureCriterion::CommandNotAttested);
            }
            criteria.push(FailureCriterion::SampleReadError);
            criteria.push(FailureCriterion::CanaryStillPresent);
            criteria.push(FailureCriterion::SignatureStillPresent);
            VerificationSpec {
                sampling,
                target: VerificationTarget::NoCanary,
                write_canaries: true,
                require_no_signatures: true,
            }
        }
    };
    let whole_controller = match method {
        EraseMethod::NvmeSanitizeBlockErase
        | EraseMethod::NvmeSanitizeCryptoErase
        | EraseMethod::NvmeSanitizeOverwrite { .. } => true,
        EraseMethod::NvmeFormatCryptoErase | EraseMethod::NvmeFormatUserDataErase => {
            target.nvme.is_some_and(|n| n.format_erases_all_namespaces)
        }
        _ => false,
    };
    let estimated_seconds = target
        .ata
        .and_then(|ata| match method {
            EraseMethod::AtaSecurityEraseEnhanced => ata.enhanced_erase_minutes,
            EraseMethod::AtaSecurityEraseNormal => ata.security_erase_minutes,
            _ => None,
        })
        .map(|minutes| u64::from(minutes) * 60);
    PlannedStep {
        method,
        level,
        failure_criteria: criteria,
        verification,
        whole_controller,
        estimated_seconds,
    }
}

fn destroy_reasons(target: &TargetCapabilities, policy: &Policy) -> Vec<DestroyReason> {
    let h = target.health;
    let above = |v: Option<u64>| v.is_some_and(|n| n > 0);
    let mut reasons = Vec::new();
    if h.smart_failing {
        reasons.push(DestroyReason::SmartFailing);
    }
    if h.reallocated_sectors
        .is_some_and(|n| n >= policy.max_reallocated_sectors)
    {
        reasons.push(DestroyReason::ReallocatedAboveThreshold);
    }
    if above(h.pending_sectors) || above(h.offline_uncorrectable) {
        reasons.push(DestroyReason::PendingOrUncorrectable);
    }
    if h.nvme_reliability_degraded {
        reasons.push(DestroyReason::ReliabilityDegraded);
    }
    if h.nvme_media_errors > 0 {
        reasons.push(DestroyReason::MediaErrors);
    }
    if h.nvme_spare_below_threshold {
        reasons.push(DestroyReason::SpareBelowThreshold);
    }
    if h.nvme_percentage_used.is_some_and(|u| u >= 100) {
        reasons.push(DestroyReason::EnduranceExhausted);
    }
    reasons.sort();
    reasons
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::taxonomy::{AtaCaps, HealthFacts, MediaType, NvmeCaps, ProtectionFlags};

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

    fn nvme_full() -> TargetCapabilities {
        TargetCapabilities {
            nvme: Some(NvmeCaps {
                sanitize_block_erase: true,
                sanitize_crypto_erase: true,
                sanitize_overwrite: false,
                format_supported: true,
                format_crypto_erase: true,
                ..NvmeCaps::default()
            }),
            ..base(MediaType::Ssd, Transport::Nvme)
        }
    }

    fn sata_ssd() -> TargetCapabilities {
        TargetCapabilities {
            ata: Some(AtaCaps {
                sanitize_supported: true,
                sanitize_block_erase: true,
                sanitize_crypto_scramble: true,
                security_supported: true,
                enhanced_erase_supported: true,
                security_erase_minutes: Some(2),
                enhanced_erase_minutes: Some(4),
                ..AtaCaps::default()
            }),
            ..base(MediaType::Ssd, Transport::Sata)
        }
    }

    fn sata_hdd() -> TargetCapabilities {
        TargetCapabilities {
            ata: Some(AtaCaps {
                sanitize_supported: true,
                sanitize_overwrite: true,
                security_supported: true,
                enhanced_erase_supported: true,
                security_erase_minutes: Some(180),
                enhanced_erase_minutes: Some(180),
                ..AtaCaps::default()
            }),
            ..base(MediaType::Hdd, Transport::Sata)
        }
    }

    fn ready(verdict: Verdict) -> ErasePlan {
        match verdict {
            Verdict::Ready(plan) => *plan,
            other => panic!("attendu Ready, obtenu {other:?}"),
        }
    }

    fn plan(target: &TargetCapabilities) -> ErasePlan {
        ready(
            decide(target, &Policy::nist_800_88_rev1(), &Preferences::default()).expect("décision"),
        )
    }

    fn methods(plan: &ErasePlan) -> Vec<EraseMethod> {
        plan.steps().map(|s| s.method).collect()
    }

    const HOST: EraseMethod = EraseMethod::HostOverwrite {
        passes: 1,
        final_zero: true,
    };

    // --- Matrice NIST : cibles saines.

    #[test]
    fn nvme_multiple_namespaces_are_flagged_when_a_step_is_namespace_local() {
        // Un seul namespace : rien à signaler, même avec un Format local.
        let single = plan(&nvme_full());
        assert!(single.notes.is_empty());

        // Plusieurs namespaces, sanitize disponible : le primaire couvre le
        // contrôleur, mais les replis Format et hôte ne couvrent que le
        // namespace ouvert → limite portée.
        let mut multi = nvme_full();
        multi.nvme = Some(NvmeCaps {
            multiple_namespaces: true,
            ..multi.nvme.expect("nvme")
        });
        let plan_multi = plan(&multi);
        assert!(plan_multi.primary.whole_controller);
        assert_eq!(plan_multi.notes, [PlanNote::NvmeOtherNamespacesUntouched]);

        // Plusieurs namespaces, Format NVM seul avec FNA bit 1 : le secure
        // erase porte sur tous les namespaces, rien à signaler tant que le
        // repli hôte n'existe pas... mais il existe toujours : limite portée
        // par le repli. Sans repli hôte, le plan est propre.
        let mut format_all = base(MediaType::Ssd, Transport::Nvme);
        format_all.nvme = Some(NvmeCaps {
            format_supported: true,
            format_crypto_erase: true,
            format_erases_all_namespaces: true,
            multiple_namespaces: true,
            ..NvmeCaps::default()
        });
        let plan_all = plan(&format_all);
        assert!(plan_all.primary.whole_controller);
        assert!(plan_all
            .notes
            .contains(&PlanNote::NvmeOtherNamespacesUntouched));
        let no_host = Preferences {
            allow_host_fallback: false,
            ..Preferences::default()
        };
        let plan_no_host =
            ready(decide(&format_all, &Policy::nist_800_88_rev1(), &no_host).expect("décision"));
        assert!(plan_no_host.steps().all(|s| s.whole_controller));
        assert!(plan_no_host.notes.is_empty());
    }

    #[test]
    fn nvme_prefers_purge_firmware_then_falls_back_in_order() {
        let plan = plan(&nvme_full());
        assert_eq!(plan.target_level, NistLevel::Purge);
        assert_eq!(
            methods(&plan),
            [
                EraseMethod::NvmeSanitizeBlockErase,
                EraseMethod::NvmeSanitizeCryptoErase,
                EraseMethod::NvmeFormatCryptoErase,
                EraseMethod::NvmeFormatUserDataErase,
                HOST,
            ]
        );
        assert!(plan.degradation.is_none() && plan.excluded.is_empty());
        assert!(
            plan.primary.whole_controller,
            "un sanitize NVMe porte sur tout le contrôleur"
        );
        assert_eq!(
            plan.worst_level(),
            NistLevel::Clear,
            "le dernier repli est Clear"
        );
        assert!(plan
            .primary
            .failure_criteria
            .contains(&FailureCriterion::CanaryStillPresent));
        assert!(plan.primary.verification.write_canaries);
        assert_eq!(plan.policy_id, "nist-800-88r1");
    }

    #[test]
    fn prefer_cryptographic_swaps_the_first_two_purge_methods() {
        let prefs = Preferences {
            prefer_cryptographic: true,
            ..Preferences::default()
        };
        let plan = ready(decide(&nvme_full(), &Policy::nist_800_88_rev1(), &prefs).unwrap());
        assert_eq!(plan.primary.method, EraseMethod::NvmeSanitizeCryptoErase);
        assert!(plan
            .primary
            .failure_criteria
            .contains(&FailureCriterion::CommandNotAttested));
    }

    #[test]
    fn nvme_format_user_data_erase_is_purge_in_rev1_and_clear_in_rev2() {
        let target = TargetCapabilities {
            nvme: Some(NvmeCaps {
                format_supported: true,
                ..NvmeCaps::default()
            }),
            ..base(MediaType::Ssd, Transport::Nvme)
        };
        let r1 = ready(
            decide(
                &target,
                &Policy::nist_800_88_rev1(),
                &Preferences::default(),
            )
            .unwrap(),
        );
        assert_eq!(
            (r1.primary.method, r1.target_level),
            (EraseMethod::NvmeFormatUserDataErase, NistLevel::Purge)
        );
        let r2 = ready(
            decide(
                &target,
                &Policy::nist_800_88_rev2(),
                &Preferences::default(),
            )
            .unwrap(),
        );
        assert_eq!(
            (r2.primary.method, r2.target_level),
            (EraseMethod::NvmeFormatUserDataErase, NistLevel::Clear)
        );
        assert!(
            r2.degradation.is_none(),
            "rien n'a été écarté : pas de dégradation, juste un Clear assumé"
        );
    }

    #[test]
    fn sata_ssd_order_and_rev2_demotion_of_security_erase() {
        let plan = plan(&sata_ssd());
        assert_eq!(
            methods(&plan),
            [
                EraseMethod::AtaSanitizeBlockErase,
                EraseMethod::AtaSanitizeCryptoScramble,
                EraseMethod::AtaSecurityEraseEnhanced,
                EraseMethod::AtaSecurityEraseNormal,
                HOST,
            ]
        );
        let levels: Vec<NistLevel> = plan.steps().map(|s| s.level).collect();
        assert_eq!(
            levels,
            [
                NistLevel::Purge,
                NistLevel::Purge,
                NistLevel::Purge,
                NistLevel::Clear,
                NistLevel::Clear
            ]
        );
        let enhanced = plan
            .steps()
            .find(|s| s.method == EraseMethod::AtaSecurityEraseEnhanced)
            .unwrap();
        assert_eq!(enhanced.estimated_seconds, Some(240));
        assert!(enhanced
            .failure_criteria
            .contains(&FailureCriterion::SecurityStillEnabled));
        let r2 = ready(
            decide(
                &sata_ssd(),
                &Policy::nist_800_88_rev2(),
                &Preferences::default(),
            )
            .unwrap(),
        );
        assert_eq!(
            r2.steps().map(|s| s.level).collect::<Vec<_>>(),
            [
                NistLevel::Purge,
                NistLevel::Purge,
                NistLevel::Clear,
                NistLevel::Clear,
                NistLevel::Clear
            ]
        );
    }

    #[test]
    fn hdd_uses_sanitize_overwrite_then_security_erase_never_block_erase() {
        let plan = plan(&sata_hdd());
        assert_eq!(
            methods(&plan),
            [
                EraseMethod::AtaSanitizeOverwrite,
                EraseMethod::AtaSecurityEraseEnhanced,
                EraseMethod::AtaSecurityEraseNormal,
                HOST,
            ]
        );
        assert!(plan.steps().take(3).all(|s| s.level == NistLevel::Purge));
    }

    #[test]
    fn unknown_media_follows_ssd_rules() {
        let mut target = sata_hdd();
        target.media = MediaType::Unknown;
        let plan = plan(&target);
        assert!(!methods(&plan).contains(&EraseMethod::AtaSanitizeOverwrite));
        assert_eq!(plan.primary.method, EraseMethod::AtaSecurityEraseEnhanced);
    }

    #[test]
    fn no_firmware_method_degrades_explicitly_to_host_overwrite() {
        let target = base(MediaType::Ssd, Transport::Virtio);
        let plan = plan(&target);
        assert_eq!(plan.primary.method, HOST);
        assert_eq!(plan.target_level, NistLevel::Clear);
        assert!(plan.fallbacks.is_empty());
        assert!(
            plan.degradation.is_none(),
            "aucun Purge n'existait : pas une dégradation"
        );
        assert_eq!(
            plan.primary.verification.target,
            VerificationTarget::ZerosAfterPasses { passes: 1 }
        );
        assert!(plan
            .primary
            .failure_criteria
            .contains(&FailureCriterion::NotZeroed));
    }

    #[test]
    fn host_passes_and_final_zero_follow_preferences_and_stay_clear() {
        let prefs = Preferences {
            host_passes: 3,
            host_final_zero: false,
            ..Preferences::default()
        };
        let plan = ready(
            decide(
                &base(MediaType::Hdd, Transport::Other),
                &Policy::nist_800_88_rev1(),
                &prefs,
            )
            .unwrap(),
        );
        assert_eq!(
            plan.primary.method,
            EraseMethod::HostOverwrite {
                passes: 3,
                final_zero: false
            }
        );
        assert_eq!(plan.primary.level, NistLevel::Clear);
        assert_eq!(
            plan.primary.verification.target,
            VerificationTarget::PatternOfPass { pass: 2 }
        );
        assert!(!plan
            .primary
            .failure_criteria
            .contains(&FailureCriterion::NotZeroed));
        let bad = Preferences {
            host_passes: 0,
            ..Preferences::default()
        };
        assert_eq!(
            decide(&sata_ssd(), &Policy::nist_800_88_rev1(), &bad),
            Err(CoreError::PassesOutOfRange(0))
        );
    }

    // --- Dégradations explicites.

    #[test]
    fn frozen_ssd_with_sanitize_stays_purge_and_lists_security_erase_as_excluded() {
        let mut target = sata_ssd();
        target.ata.as_mut().unwrap().security_frozen = true;
        let plan = plan(&target);
        assert_eq!(plan.primary.method, EraseMethod::AtaSanitizeBlockErase);
        assert!(plan.degradation.is_none(), "un Purge reste disponible");
        assert!(plan
            .excluded
            .iter()
            .all(|e| e.reason == ExclusionReason::SecurityFrozen));
        assert_eq!(plan.excluded.len(), 2);
        assert!(!methods(&plan).iter().any(|m| m.is_ata_security_erase()));
    }

    #[test]
    fn frozen_ssd_without_sanitize_degrades_to_clear_with_suspend_guidance() {
        let mut target = sata_ssd();
        let ata = target.ata.as_mut().unwrap();
        ata.sanitize_supported = false;
        ata.security_frozen = true;
        let plan = plan(&target);
        assert_eq!(plan.primary.method, HOST);
        let degradation = plan.degradation.expect("dégradation explicite");
        assert_eq!(
            (degradation.from, degradation.to),
            (NistLevel::Purge, NistLevel::Clear)
        );
        assert_eq!(degradation.guidance, [Guidance::SuspendToRam]);
        assert_eq!(degradation.lost.len(), 1, "seul l'enhanced était Purge");
        assert_eq!(
            degradation.lost[0].method,
            EraseMethod::AtaSecurityEraseEnhanced
        );
    }

    #[test]
    fn foreign_password_excludes_security_erase_with_its_guidance() {
        let mut target = sata_ssd();
        let ata = target.ata.as_mut().unwrap();
        ata.sanitize_supported = false;
        ata.security_enabled = true;
        let plan = plan(&target);
        assert_eq!(plan.primary.method, HOST);
        assert_eq!(
            plan.degradation.unwrap().guidance,
            [Guidance::RemoveForeignPassword]
        );
    }

    #[test]
    fn sanitize_freeze_lock_excludes_sanitize_only() {
        let mut target = sata_ssd();
        target.ata.as_mut().unwrap().sanitize_frozen = true;
        let plan = plan(&target);
        assert_eq!(plan.primary.method, EraseMethod::AtaSecurityEraseEnhanced);
        assert!(plan
            .excluded
            .iter()
            .all(|e| e.reason == ExclusionReason::SanitizeFrozen));
        assert!(plan.degradation.is_none());
    }

    #[test]
    fn usb_bridge_degrades_unless_firmware_over_usb_is_allowed() {
        let mut target = sata_ssd();
        target.transport = Transport::Usb;
        let plan = plan(&target);
        assert_eq!(plan.primary.method, HOST);
        let degradation = plan.degradation.expect("dégradation USB");
        assert_eq!(degradation.guidance, [Guidance::AttachNatively]);
        assert!(plan
            .excluded
            .iter()
            .all(|e| e.reason == ExclusionReason::UsbBridge));
        let prefs = Preferences {
            firmware_over_usb: true,
            ..Preferences::default()
        };
        let allowed = ready(decide(&target, &Policy::nist_800_88_rev1(), &prefs).unwrap());
        assert_eq!(allowed.primary.method, EraseMethod::AtaSanitizeBlockErase);
    }

    #[test]
    fn host_fallback_disabled_refuses_when_nothing_else_remains() {
        let prefs = Preferences {
            allow_host_fallback: false,
            ..Preferences::default()
        };
        let target = base(MediaType::Ssd, Transport::Virtio);
        assert_eq!(
            decide(&target, &Policy::nist_800_88_rev1(), &prefs).unwrap(),
            Verdict::Refused {
                reasons: vec![RefusalReason::NoMethodAvailable]
            }
        );
        // Avec un firmware, le plan existe mais sans repli hôte.
        let plan = ready(decide(&nvme_full(), &Policy::nist_800_88_rev1(), &prefs).unwrap());
        assert!(!methods(&plan).contains(&HOST));
        assert!(plan
            .excluded
            .iter()
            .any(|e| e.reason == ExclusionReason::HostFallbackDisabled));
    }

    #[test]
    fn inspection_failed_offers_host_only_or_refuses() {
        let mut target = nvme_full();
        target.inspection_failed = true;
        let plan = plan(&target);
        assert_eq!(methods(&plan), [HOST]);
        let prefs = Preferences {
            allow_host_fallback: false,
            ..Preferences::default()
        };
        assert_eq!(
            decide(&target, &Policy::nist_800_88_rev1(), &prefs).unwrap(),
            Verdict::Refused {
                reasons: vec![RefusalReason::InspectionFailedNoFallback]
            }
        );
    }

    // --- Refus et blocages.

    #[test]
    fn protections_refuse_before_anything_else() {
        let mut target = nvme_full();
        target.protection = ProtectionFlags {
            hosts_system: true,
            mounted: true,
            swap: true,
            held: true,
        };
        target.nvme.as_mut().unwrap().read_only = true;
        assert_eq!(
            plan_or_verdict(&target),
            Verdict::Refused {
                reasons: vec![
                    RefusalReason::HostsSystem,
                    RefusalReason::Mounted,
                    RefusalReason::ActiveSwap,
                    RefusalReason::VolumeMember
                ]
            }
        );
        let mut mounted = sata_ssd();
        mounted.protection.mounted = true;
        assert_eq!(
            plan_or_verdict(&mounted),
            Verdict::Refused {
                reasons: vec![RefusalReason::Mounted]
            }
        );
    }

    fn plan_or_verdict(target: &TargetCapabilities) -> Verdict {
        decide(target, &Policy::nist_800_88_rev1(), &Preferences::default()).expect("décision")
    }

    #[test]
    fn read_only_nvme_is_refused() {
        let mut target = nvme_full();
        target.nvme.as_mut().unwrap().read_only = true;
        assert_eq!(
            plan_or_verdict(&target),
            Verdict::Refused {
                reasons: vec![RefusalReason::ReadOnlyMedia]
            }
        );
    }

    #[test]
    fn locked_expired_and_sanitize_states_block_with_guidance() {
        let mut target = sata_ssd();
        {
            let ata = target.ata.as_mut().unwrap();
            ata.security_locked = true;
            ata.security_attempts_expired = true;
        }
        assert_eq!(
            plan_or_verdict(&target),
            Verdict::Blocked {
                reasons: vec![Blocker::SecurityLocked, Blocker::PasswordAttemptsExpired],
                guidance: vec![Guidance::UnlockWithPassword, Guidance::PowerCycle],
            }
        );
        let mut nvme = nvme_full();
        nvme.nvme.as_mut().unwrap().sanitize_state = SanitizeState::InProgress;
        assert_eq!(
            plan_or_verdict(&nvme),
            Verdict::Blocked {
                reasons: vec![Blocker::SanitizeInProgress],
                guidance: vec![Guidance::WaitForSanitize]
            }
        );
        let mut failed = sata_hdd();
        failed.ata.as_mut().unwrap().sanitize_state = SanitizeState::Failed;
        assert_eq!(
            plan_or_verdict(&failed),
            Verdict::Blocked {
                reasons: vec![Blocker::SanitizeFailed],
                guidance: vec![Guidance::ClearSanitizeFailure]
            }
        );
    }

    // --- Destruction recommandée (jamais exécutée).

    #[test]
    fn destroy_is_recommended_from_health_never_executed() {
        let mut target = sata_hdd();
        target.health = HealthFacts {
            smart_failing: true,
            reallocated_sectors: Some(50),
            pending_sectors: Some(1),
            ..HealthFacts::default()
        };
        let plan = plan(&target);
        assert_eq!(
            plan.recommend_destroy,
            [
                DestroyReason::SmartFailing,
                DestroyReason::ReallocatedAboveThreshold,
                DestroyReason::PendingOrUncorrectable
            ]
        );
        assert_ne!(plan.target_level, NistLevel::Destroy);
        assert!(plan.steps().all(|s| s.level != NistLevel::Destroy));
        let below = TargetCapabilities {
            health: HealthFacts {
                reallocated_sectors: Some(49),
                ..HealthFacts::default()
            },
            ..sata_hdd()
        };
        assert!(self::plan(&below).recommend_destroy.is_empty());
        let mut nvme = nvme_full();
        nvme.health = HealthFacts {
            nvme_reliability_degraded: true,
            nvme_media_errors: 3,
            nvme_spare_below_threshold: true,
            nvme_percentage_used: Some(100),
            ..HealthFacts::default()
        };
        assert_eq!(
            self::plan(&nvme).recommend_destroy,
            [
                DestroyReason::ReliabilityDegraded,
                DestroyReason::MediaErrors,
                DestroyReason::SpareBelowThreshold,
                DestroyReason::EnduranceExhausted
            ]
        );
    }

    // --- Invariants.

    #[test]
    fn decision_is_deterministic_and_validates_geometry() {
        let target = sata_ssd();
        let a = plan_or_verdict(&target);
        let b = plan_or_verdict(&target);
        assert_eq!(a, b);
        let mut empty = sata_ssd();
        empty.block_count = 0;
        assert_eq!(
            decide(&empty, &Policy::nist_800_88_rev1(), &Preferences::default()),
            Err(CoreError::NoBlocks)
        );
        let mut odd = sata_ssd();
        odd.block_size = 510;
        assert_eq!(
            decide(&odd, &Policy::nist_800_88_rev1(), &Preferences::default()),
            Err(CoreError::InvalidBlockSize(510))
        );
    }

    #[test]
    fn purge_steps_always_precede_clear_steps() {
        for target in [nvme_full(), sata_ssd(), sata_hdd()] {
            let plan = plan(&target);
            let levels: Vec<NistLevel> = plan.steps().map(|s| s.level).collect();
            let mut sorted = levels.clone();
            sorted.sort_by(|a, b| b.cmp(a));
            assert_eq!(levels, sorted, "{:?}", target.name);
        }
    }

    #[test]
    fn nvme_format_whole_controller_follows_fna() {
        let mut target = TargetCapabilities {
            nvme: Some(NvmeCaps {
                format_supported: true,
                format_crypto_erase: true,
                ..NvmeCaps::default()
            }),
            ..base(MediaType::Ssd, Transport::Nvme)
        };
        assert!(!plan(&target).primary.whole_controller);
        target.nvme.as_mut().unwrap().format_erases_all_namespaces = true;
        assert!(plan(&target).primary.whole_controller);
    }
}
