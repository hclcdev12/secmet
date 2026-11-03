//! La cible réelle du runner : un [`BlockDevice`] ouvert `O_EXCL | O_DIRECT`.
//!
//! Toutes les E/S passent par un tampon aligné à 4 Kio (exigence
//! `O_DIRECT`), par morceaux de [`CHUNK_BYTES`](crate::runner::CHUNK_BYTES).
//! Les commandes firmware sont celles de `secmet-device` : sanitize suivi par
//! sondage (annulable entre deux sondages, le disque continue), Format NVM et
//! SECURITY ERASE UNIT bloquants (non annulables).

use std::os::unix::fs::FileExt;
use std::thread::sleep;
use std::time::{Duration, Instant};

use secmet_core::engine::FailureCriterion;
use secmet_core::taxonomy::EraseMethod;
use secmet_device::aligned::AlignedBuffer;
use secmet_device::ata::{self, security, AtaDevice, SanitizeOperation};
use secmet_device::block::BlockDevice;
use secmet_device::nvme::{self, NvmeDevice, SanitizeAction, SecureErase};
use secmet_device::sysfs::Transport;
use secmet_device::DeviceError;

use crate::identity::Scan;
use crate::runner::{Observer, StepFailure, Target, CHUNK_BYTES};
use crate::vault::PasswordVault;

/// Intervalle de sondage d'un sanitize.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Sondages consécutifs « jamais sanitisé » tolérés après le démarrage.
const START_GRACE_POLLS: u32 = 5;
/// Cadence des événements pendant une commande bloquante.
const TICK_INTERVAL: Duration = Duration::from_secs(1);

/// Disque réel.
pub struct DiskTarget {
    device: BlockDevice,
    buffer: AlignedBuffer,
    serial: String,
    transport: Transport,
    security_erase_minutes: Option<u32>,
    enhanced_erase_minutes: Option<u32>,
    vault: PasswordVault,
}

impl std::fmt::Debug for DiskTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiskTarget")
            .field("device", &self.device.path())
            .field("serial", &self.serial)
            .finish()
    }
}

impl DiskTarget {
    /// Enveloppe `device` avec ce que `scan` sait du disque.
    pub fn new(device: BlockDevice, scan: &Scan, vault: PasswordVault) -> Self {
        let (security_erase_minutes, enhanced_erase_minutes) = scan
            .caps
            .ata
            .map(|a| (a.security_erase_minutes, a.enhanced_erase_minutes))
            .unwrap_or((None, None));
        let block_size = device.block_size() as usize;
        let chunk = (CHUNK_BYTES / block_size).max(1) * block_size;
        Self {
            device,
            buffer: AlignedBuffer::new(chunk),
            serial: scan.identity.serial.clone(),
            transport: scan.entry.transport,
            security_erase_minutes,
            enhanced_erase_minutes,
            vault,
        }
    }

    fn check_len(&self, len: usize) -> std::io::Result<()> {
        if len % self.device.block_size() as usize != 0 {
            return Err(std::io::Error::other("longueur non multiple du bloc"));
        }
        Ok(())
    }

    fn firmware_error(error: DeviceError) -> StepFailure {
        StepFailure::Criterion {
            criterion: FailureCriterion::FirmwareReportedFailure,
            detail: error.to_string(),
        }
    }

    /// Exécute `run` (commande bloquante, non sondable) sur un fil dédié et
    /// publie pendant ce temps une progression déduite de la durée annoncée
    /// par le disque : pourcentage plafonné à 99 et reste estimé. Sans
    /// estimation, les événements continuent à 0 % pour signaler l'activité.
    fn blocking<T, F>(observer: &mut dyn Observer, estimate_minutes: Option<u32>, run: F) -> T
    where
        T: Send,
        F: FnOnce() -> T + Send,
    {
        let estimate = estimate_minutes
            .filter(|m| *m > 0)
            .map(|m| Duration::from_secs(u64::from(m) * 60));
        let started = Instant::now();
        observer.progress(0);
        observer.eta(estimate.map(|e| e.as_secs()));
        std::thread::scope(|scope| {
            let worker = scope.spawn(run);
            while !worker.is_finished() {
                sleep(TICK_INTERVAL);
                let elapsed = started.elapsed();
                match estimate {
                    Some(total) => {
                        let ratio = elapsed.as_secs_f64() / total.as_secs_f64();
                        let percent = (ratio * 100.0).floor().clamp(0.0, 99.0) as u8;
                        observer.eta(Some(total.saturating_sub(elapsed).as_secs()));
                        observer.progress(percent);
                    }
                    None => observer.progress(0),
                }
            }
            worker
                .join()
                .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
        })
    }

    fn poll<F>(observer: &mut dyn Observer, mut read: F) -> Result<(), StepFailure>
    where
        F: FnMut() -> Result<Poll, StepFailure>,
    {
        let mut not_started = 0u32;
        loop {
            match read()? {
                Poll::Done => return Ok(()),
                Poll::Failed(detail) => {
                    return Err(StepFailure::Criterion {
                        criterion: FailureCriterion::FirmwareReportedFailure,
                        detail,
                    })
                }
                Poll::NotStarted => {
                    not_started += 1;
                    if not_started > START_GRACE_POLLS {
                        return Err(StepFailure::Criterion {
                            criterion: FailureCriterion::SanitizeNotCompleted,
                            detail: "le disque ne rapporte aucun sanitize en cours".into(),
                        });
                    }
                }
                Poll::Running(percent) => observer.progress(percent),
            }
            if observer.cancelled() {
                return Err(StepFailure::Cancelled);
            }
            sleep(POLL_INTERVAL);
        }
    }

    fn nvme_sanitize(
        &self,
        action: SanitizeAction,
        observer: &mut dyn Observer,
    ) -> Result<(), StepFailure> {
        let device = NvmeDevice::new(self.device.file()).map_err(Self::firmware_error)?;
        device
            .start_sanitize(action)
            .map_err(Self::firmware_error)?;
        Self::poll(observer, || {
            let log = device.sanitize_log().map_err(Self::firmware_error)?;
            Ok(match log.state {
                nvme::SanitizeState::InProgress => Poll::Running(log.percent()),
                nvme::SanitizeState::Completed => Poll::Done,
                nvme::SanitizeState::Failed => {
                    Poll::Failed("journal sanitize : mode Failure".into())
                }
                nvme::SanitizeState::NeverSanitized => Poll::NotStarted,
            })
        })
    }

    fn nvme_format(
        &self,
        ses: SecureErase,
        observer: &mut dyn Observer,
    ) -> Result<(), StepFailure> {
        let device = NvmeDevice::new(self.device.file()).map_err(Self::firmware_error)?;
        Self::blocking(observer, None, || device.format(ses)).map_err(Self::firmware_error)?;
        observer.eta(Some(0));
        observer.progress(100);
        Ok(())
    }

    fn ata_sanitize(
        &self,
        operation: SanitizeOperation,
        observer: &mut dyn Observer,
    ) -> Result<(), StepFailure> {
        let device = AtaDevice::for_transport(self.device.file(), self.transport);
        device
            .start_sanitize(operation)
            .map_err(Self::firmware_error)?;
        let mut seen_running = false;
        Self::poll(observer, || {
            let status = device.sanitize_status().map_err(Self::firmware_error)?;
            Ok(match status.state {
                ata::SanitizeState::InProgress => {
                    seen_running = true;
                    Poll::Running(status.percent())
                }
                ata::SanitizeState::Failed => Poll::Failed("SANITIZE STATUS EXT : échec".into()),
                // Achevé sans erreur (bit 15) : terminé, même sans avoir été
                // vu en cours (opération très rapide).
                ata::SanitizeState::Completed => Poll::Done,
                // Idle après avoir été vu en cours : terminé. Idle d'emblée :
                // le disque n'a rien lancé (après la période de grâce, échec).
                ata::SanitizeState::Idle if seen_running => Poll::Done,
                ata::SanitizeState::Idle => Poll::NotStarted,
            })
        })
    }

    fn ata_security_erase(
        &self,
        enhanced: bool,
        observer: &mut dyn Observer,
    ) -> Result<(), StepFailure> {
        let device = AtaDevice::for_transport(self.device.file(), self.transport);
        let password = security::Password::random().map_err(Self::firmware_error)?;
        self.vault
            .store(&self.serial, &password)
            .map_err(|e| StepFailure::Io(format!("enregistrement du mot de passe: {e}")))?;
        if let Err(e) = device.security_set_user_password(&password) {
            // Le mot de passe n'a jamais atteint le disque : pas d'entrée orpheline.
            let _ = self.vault.remove(&self.serial);
            return Err(Self::firmware_error(e));
        }
        let estimate = if enhanced {
            self.enhanced_erase_minutes
        } else {
            self.security_erase_minutes
        };
        let timeout_ms = security::erase_timeout_ms(estimate);
        let (erase, after, device) = Self::blocking(observer, estimate, || {
            let erase = device.security_erase_unit(&password, enhanced, timeout_ms);
            let after = device.identify();
            (erase, after, device)
        });
        match (erase, after) {
            (Ok(()), Ok(identify)) if !identify.security.enabled => {
                let _ = self.vault.remove(&self.serial);
                observer.eta(Some(0));
                observer.progress(100);
                Ok(())
            }
            (Ok(()), Ok(_)) => {
                let _ = device.security_disable_password(security::Identifier::User, &password);
                Err(StepFailure::Criterion {
                    criterion: FailureCriterion::SecurityStillEnabled,
                    detail: "le mot de passe est encore actif après SECURITY ERASE UNIT".into(),
                })
            }
            (Err(e), _) => {
                let _ = device.security_disable_password(security::Identifier::User, &password);
                Err(Self::firmware_error(e))
            }
            (Ok(()), Err(e)) => Err(Self::firmware_error(e)),
        }
    }
}

enum Poll {
    Running(u8),
    Done,
    Failed(String),
    NotStarted,
}

impl Target for DiskTarget {
    fn block_size(&self) -> u64 {
        self.device.block_size()
    }

    fn block_count(&self) -> u64 {
        self.device.block_count()
    }

    fn read_blocks(&mut self, lba: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.check_len(buf.len())?;
        let block_size = self.device.block_size();
        let chunk = self.buffer.len();
        let mut done = 0usize;
        while done < buf.len() {
            let n = chunk.min(buf.len() - done);
            let offset = lba * block_size + done as u64;
            self.device
                .file()
                .read_exact_at(&mut self.buffer[..n], offset)?;
            buf[done..done + n].copy_from_slice(&self.buffer[..n]);
            done += n;
        }
        Ok(())
    }

    fn write_blocks(&mut self, lba: u64, data: &[u8]) -> std::io::Result<()> {
        self.check_len(data.len())?;
        let block_size = self.device.block_size();
        let chunk = self.buffer.len();
        let mut done = 0usize;
        while done < data.len() {
            let n = chunk.min(data.len() - done);
            self.buffer[..n].copy_from_slice(&data[done..done + n]);
            let offset = lba * block_size + done as u64;
            self.device.file().write_all_at(&self.buffer[..n], offset)?;
            done += n;
        }
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.device
            .flush()
            .map_err(|e| std::io::Error::other(e.to_string()))
    }

    fn run_firmware(
        &mut self,
        method: EraseMethod,
        observer: &mut dyn Observer,
    ) -> Result<(), StepFailure> {
        let is_nvme = self.device.is_nvme();
        match method {
            EraseMethod::HostOverwrite { .. } => Err(StepFailure::Io(
                "l'écriture hôte n'est pas une commande firmware".into(),
            )),
            m if m.is_nvme() != is_nvme => Err(StepFailure::Io(format!(
                "{} ne s'applique pas à {}",
                m.label(),
                self.device.path().display()
            ))),
            EraseMethod::NvmeSanitizeBlockErase => {
                self.nvme_sanitize(SanitizeAction::BlockErase, observer)
            }
            EraseMethod::NvmeSanitizeCryptoErase => {
                self.nvme_sanitize(SanitizeAction::CryptoErase, observer)
            }
            EraseMethod::NvmeSanitizeOverwrite { passes } => {
                self.nvme_sanitize(SanitizeAction::Overwrite { passes, pattern: 0 }, observer)
            }
            EraseMethod::NvmeFormatCryptoErase => {
                self.nvme_format(SecureErase::Cryptographic, observer)
            }
            EraseMethod::NvmeFormatUserDataErase => {
                self.nvme_format(SecureErase::UserData, observer)
            }
            EraseMethod::AtaSanitizeBlockErase => {
                self.ata_sanitize(SanitizeOperation::BlockErase, observer)
            }
            EraseMethod::AtaSanitizeCryptoScramble => {
                self.ata_sanitize(SanitizeOperation::CryptoScramble, observer)
            }
            EraseMethod::AtaSanitizeOverwrite => {
                self.ata_sanitize(SanitizeOperation::Overwrite, observer)
            }
            EraseMethod::AtaSecurityEraseEnhanced => self.ata_security_erase(true, observer),
            EraseMethod::AtaSecurityEraseNormal => self.ata_security_erase(false, observer),
        }
    }

    fn firmware_cancellable(&self, method: EraseMethod) -> bool {
        // Le sondage peut s'interrompre (le disque continue seul) ; une
        // commande bloquante dans l'ioctl, non.
        matches!(
            method,
            EraseMethod::NvmeSanitizeBlockErase
                | EraseMethod::NvmeSanitizeCryptoErase
                | EraseMethod::NvmeSanitizeOverwrite { .. }
                | EraseMethod::AtaSanitizeBlockErase
                | EraseMethod::AtaSanitizeCryptoScramble
                | EraseMethod::AtaSanitizeOverwrite
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[derive(Default)]
    struct Recorder {
        percents: Vec<u8>,
        etas: Vec<Option<u64>>,
    }

    impl Observer for Recorder {
        fn progress(&mut self, percent: u8) {
            self.percents.push(percent);
        }

        fn eta(&mut self, seconds: Option<u64>) {
            self.etas.push(seconds);
        }

        fn cancelled(&self) -> bool {
            false
        }
    }

    #[test]
    fn blocking_command_reports_time_based_progress_and_eta() {
        let mut observer = Recorder::default();
        let result = DiskTarget::blocking(&mut observer, Some(1), || {
            sleep(Duration::from_millis(2300));
            42
        });
        assert_eq!(result, 42);
        assert_eq!(observer.percents.first(), Some(&0));
        assert!(observer.percents.len() >= 3, "{:?}", observer.percents);
        assert!(observer.percents.iter().all(|p| *p < 100));
        assert!(observer.percents.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(observer.etas.first(), Some(&Some(60)));
        let last = observer.etas.last().unwrap().unwrap();
        assert!((55..60).contains(&last), "{last}");
    }

    #[test]
    fn blocking_command_without_estimate_ticks_at_zero() {
        let mut observer = Recorder::default();
        DiskTarget::blocking(&mut observer, None, || sleep(Duration::from_millis(1300)));
        assert!(observer.percents.len() >= 2);
        assert!(observer.percents.iter().all(|p| *p == 0));
        assert_eq!(observer.etas, vec![None]);
    }
}
