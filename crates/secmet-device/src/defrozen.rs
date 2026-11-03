//! Dé-frozen ATA par cycle d'économie d'énergie ACPI (suspend-to-RAM).
//!
//! Le BIOS/UEFI envoie souvent SECURITY FREEZE LOCK au démarrage : le disque
//! refuse ensuite toute commande Security (set password, erase unit). Un
//! cycle suspend/resume réinitialise la liaison SATA et lève le gel sur la
//! plupart des contrôleurs. C'est l'op `suspend-mem` du helper SecMet.
//!
//! Sur un poste avec systemd, la veille passe par logind (`systemctl
//! suspend`) : la session graphique est prévenue (PrepareForSleep), ce qui
//! permet au bureau de rallumer l'écran et de restaurer le rétroéclairage
//! au réveil ; un écran noir après reprise est le symptôme classique d'une
//! veille déclenchée dans le dos de la session. La reprise est détectée par
//! le compteur `/sys/power/suspend_stats/success`. Sans systemd, écriture
//! directe de `mem` dans `/sys/power/state` (bloque jusqu'à la reprise).
//!
//! Toutes les causes d'échec sont typées : noyau sans état `mem`, droits
//! insuffisants (`EACCES`/`EPERM`), refus ACPI à l'écriture (`EINVAL`,
//! `EBUSY`), disque disparu après la reprise, gel persistant.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use crate::ata::AtaDevice;
use crate::{DeviceError, Result};

const POWER_STATE: &str = "/sys/power/state";
const SUSPEND_SUCCESS: &str = "/sys/power/suspend_stats/success";
const SYSTEMD_RUNNING: &str = "/run/systemd/system";
/// Temps (machine éveillée) accordé à logind pour suspendre puis reprendre.
const LOGIND_TIMEOUT: Duration = Duration::from_secs(90);

/// Pourquoi la mise en veille n'a pas eu lieu. Enveloppée dans
/// [`DeviceError::Power`] avec un message lisible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspendRefusal {
    /// `/sys/power/state` ne propose pas `mem` (noyau, VM, conteneur).
    Unsupported,
    /// Pas les droits d'écrire `/sys/power/state` (root requis).
    Privileges,
    /// ACPI ou le noyau a refusé la transition (`EINVAL`, `EBUSY`, `EIO`…).
    Rejected,
}

impl SuspendRefusal {
    fn message(self, detail: &io::Error) -> String {
        match self {
            Self::Unsupported => {
                "le noyau ne propose pas la veille « mem » dans /sys/power/state".to_string()
            }
            Self::Privileges => {
                format!("droits insuffisants pour écrire /sys/power/state ({detail})")
            }
            Self::Rejected => format!("transition ACPI refusée par le noyau ({detail})"),
        }
    }
}

/// Classe une erreur d'écriture de `/sys/power/state`.
pub fn classify_write_error(error: &io::Error) -> SuspendRefusal {
    match error.raw_os_error() {
        Some(libc::EACCES) | Some(libc::EPERM) => SuspendRefusal::Privileges,
        _ => SuspendRefusal::Rejected,
    }
}

/// `mem` figure-t-il dans le contenu de `/sys/power/state` ?
pub fn mem_in(power_state: &str) -> bool {
    power_state.split_whitespace().any(|s| s == "mem")
}

/// Écrit `state` dans `path`, erreurs classées.
fn write_state(path: &Path, state: &str) -> Result<()> {
    let text =
        fs::read_to_string(path).map_err(|e| DeviceError::io(path.display().to_string(), e))?;
    if !mem_in(&text) {
        return Err(DeviceError::Power(
            SuspendRefusal::Unsupported.message(&io::Error::other("état absent")),
        ));
    }
    let mut file = match fs::OpenOptions::new().write(true).open(path) {
        Ok(file) => file,
        Err(e) => return Err(DeviceError::Power(classify_write_error(&e).message(&e))),
    };
    // L'écriture ne revient qu'après la reprise (ou immédiatement en cas de refus).
    file.write_all(state.as_bytes())
        .map_err(|e| DeviceError::Power(classify_write_error(&e).message(&e)))
}

/// Met la machine en veille (bloque jusqu'à la reprise). Root requis.
/// Par logind si systemd tourne, sinon par `/sys/power/state`.
pub fn suspend_to_ram() -> Result<()> {
    if Path::new(SYSTEMD_RUNNING).is_dir() {
        match suspend_via_logind() {
            Ok(()) => return Ok(()),
            Err(DeviceError::Power(reason)) => {
                eprintln!(
                    "secmet-helper: veille par logind impossible ({reason}), écriture directe"
                );
            }
            Err(e) => return Err(e),
        }
    }
    write_state(Path::new(POWER_STATE), "mem")
}

/// Compteur de reprises réussies du noyau.
fn suspend_successes() -> Option<u64> {
    fs::read_to_string(SUSPEND_SUCCESS)
        .ok()
        .and_then(|t| t.trim().parse().ok())
}

/// `systemctl suspend`, puis attente de la reprise (compteur du noyau).
fn suspend_via_logind() -> Result<()> {
    let text =
        fs::read_to_string(POWER_STATE).map_err(|e| DeviceError::io(POWER_STATE.to_string(), e))?;
    if !mem_in(&text) {
        return Err(DeviceError::Power(
            SuspendRefusal::Unsupported.message(&io::Error::other("état absent")),
        ));
    }
    let Some(before) = suspend_successes() else {
        return Err(DeviceError::Power(format!("{SUSPEND_SUCCESS} illisible")));
    };
    let status = std::process::Command::new("systemctl")
        .arg("suspend")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| DeviceError::Power(format!("systemctl introuvable ({e})")))?;
    if !status.success() {
        return Err(DeviceError::Power(format!("systemctl suspend : {status}")));
    }
    // Instant est monotone et ne court pas pendant la veille : le délai
    // compte le temps éveillé (préparation par logind, puis reprise).
    let started = std::time::Instant::now();
    while started.elapsed() < LOGIND_TIMEOUT {
        sleep(Duration::from_millis(500));
        if suspend_successes().is_some_and(|after| after > before) {
            return Ok(());
        }
    }
    Err(DeviceError::Power(
        "la mise en veille demandée à logind n'a pas eu lieu".to_string(),
    ))
}

/// Réglages du dégel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefrozenOptions {
    /// Délai laissé au contrôleur pour ré-énumérer le disque après la reprise.
    pub settle: Duration,
    /// Nombre de relectures d'IDENTIFY tentées après la reprise.
    pub identify_attempts: u32,
}

impl Default for DefrozenOptions {
    fn default() -> Self {
        Self {
            settle: Duration::from_secs(2),
            identify_attempts: 5,
        }
    }
}

/// Résultat d'une tentative de dégel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefrozenOutcome {
    /// Le disque était gelé avant.
    pub was_frozen: bool,
    /// Une mise en veille a été déclenchée.
    pub suspended: bool,
    /// Le disque est encore gelé après la reprise.
    pub still_frozen: bool,
}

impl DefrozenOutcome {
    /// Le disque accepte maintenant les commandes Security.
    pub fn unfrozen(&self) -> bool {
        !self.still_frozen
    }
}

/// Dégèle un disque ATA : si IDENTIFY dit « frozen », suspend-to-RAM puis
/// relecture d'IDENTIFY (avec quelques essais, le temps que le disque
/// revienne). Ne suspend pas un disque qui n'est pas gelé.
pub fn defrozen_by_suspend(
    ata: &AtaDevice<'_>,
    options: DefrozenOptions,
) -> Result<DefrozenOutcome> {
    let before = ata.identify()?;
    if !before.security.frozen {
        return Ok(DefrozenOutcome {
            was_frozen: false,
            suspended: false,
            still_frozen: false,
        });
    }
    suspend_to_ram()?;
    sleep(options.settle);
    let mut last_error = None;
    for attempt in 0..options.identify_attempts.max(1) {
        match ata.identify() {
            Ok(after) => {
                if after.serial != before.serial {
                    return Err(DeviceError::Inventory(format!(
                        "après la reprise, le nœud porte un autre disque (série « {} » au lieu de « {} »)",
                        after.serial, before.serial
                    )));
                }
                return Ok(DefrozenOutcome {
                    was_frozen: true,
                    suspended: true,
                    still_frozen: after.security.frozen,
                });
            }
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < options.identify_attempts {
                    sleep(Duration::from_secs(1));
                }
            }
        }
    }
    Err(last_error
        .unwrap_or_else(|| DeviceError::Power("IDENTIFY impossible après la reprise".to_string())))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn detects_mem_in_power_state() {
        assert!(mem_in("freeze mem disk\n"));
        assert!(mem_in("mem"));
        assert!(!mem_in("freeze disk\n"));
        assert!(!mem_in("memory"));
    }

    #[test]
    fn write_errors_are_classified() {
        assert_eq!(
            classify_write_error(&io::Error::from_raw_os_error(libc::EACCES)),
            SuspendRefusal::Privileges
        );
        assert_eq!(
            classify_write_error(&io::Error::from_raw_os_error(libc::EPERM)),
            SuspendRefusal::Privileges
        );
        assert_eq!(
            classify_write_error(&io::Error::from_raw_os_error(libc::EINVAL)),
            SuspendRefusal::Rejected
        );
        assert!(SuspendRefusal::Privileges
            .message(&io::Error::from_raw_os_error(libc::EACCES))
            .contains("droits"));
    }

    #[test]
    fn unsupported_state_and_missing_file_are_typed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("state");
        assert!(matches!(
            write_state(&missing, "mem"),
            Err(DeviceError::Io { .. })
        ));
        let no_mem = dir.path().join("no_mem");
        fs::write(&no_mem, "freeze disk\n").expect("fichier");
        match write_state(&no_mem, "mem") {
            Err(DeviceError::Power(message)) => assert!(message.contains("mem")),
            other => panic!("attendu Power, obtenu {other:?}"),
        }
        let ok = dir.path().join("ok");
        fs::write(&ok, "freeze mem disk\n").expect("fichier");
        write_state(&ok, "mem").expect("écriture");
        // Sans troncature, comme pour un fichier sysfs : « mem » en tête.
        assert!(fs::read_to_string(&ok).expect("lecture").starts_with("mem"));
    }

    #[test]
    fn privilege_refusal_on_read_only_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state");
        fs::write(&path, "mem\n").expect("fichier");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).expect("chmod");
        let outcome = write_state(&path, "mem");
        // root ignore le mode : le test n'exige l'erreur que hors root.
        let is_root = fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("Uid:"))
                    .map(|l| l.contains("\t0\t"))
            })
            .unwrap_or(false);
        if !is_root {
            match outcome {
                Err(DeviceError::Power(message)) => assert!(message.contains("droits")),
                other => panic!("attendu Power(droits), obtenu {other:?}"),
            }
        }
    }

    #[test]
    fn outcome_and_defaults() {
        let o = DefrozenOutcome {
            was_frozen: true,
            suspended: true,
            still_frozen: false,
        };
        assert!(o.unfrozen());
        assert_eq!(DefrozenOptions::default().identify_attempts, 5);
    }
}
