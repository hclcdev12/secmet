//! Sérialise les appels pkexec pour l'élévation de privilèges via PolicyKit.

use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};

static PKEXEC_LOCK: Mutex<()> = Mutex::new(());
static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Sérialise les appels `run_on_gtk_main_thread` hors thread propriétaire.
static GTK_INVOKE_GATE: Mutex<()> = Mutex::new(());
/// Durée max d'un appel pkexec capturant une sortie/statut (authentification
/// comprise). Au-delà : `timeout` tue pkexec et l'appel échoue — un disque ou
/// un polkit bloqué ne peut plus geler indéfiniment le fil appelant.
const PKEXEC_TIMEOUT_SECS: &str = "300";

pub fn is_session_active() -> bool {
    SESSION_ACTIVE.load(Ordering::Relaxed)
}

pub fn mark_session_active() {
    SESSION_ACTIVE.store(true, Ordering::Relaxed);
}

pub fn clear_session_active() {
    SESSION_ACTIVE.store(false, Ordering::Relaxed);
}

/// Vérifie si pkexec est disponible.
pub fn check_pkexec_available() -> bool {
    pkexec_path().is_some()
}

fn pkexec_path() -> Option<std::path::PathBuf> {
    Command::new("which")
        .arg("pkexec")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| std::path::PathBuf::from(s.trim().to_string()))
        .filter(|p| !p.as_os_str().is_empty())
}

/// Vérifie que pkexec est installé avec le bit setuid (requis par PolicyKit).
pub fn check_pkexec_setuid() -> Result<(), crate::error::AuthError> {
    let path = pkexec_path().ok_or(crate::error::AuthError::PkexecMissing)?;
    if pkexec_has_setuid() {
        return Ok(());
    }
    Err(crate::error::AuthError::PkexecNoSetuid {
        path: path.display().to_string(),
    })
}

pub(crate) fn pkexec_has_setuid() -> bool {
    let output = match Command::new("which").arg("pkexec").output() {
        Ok(output) if output.status.success() => output,
        _ => return false,
    };
    let path = match String::from_utf8(output.stdout) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let path = path.trim();
    if path.is_empty() {
        return false;
    }
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(_) => return false,
    };
    meta.permissions().mode() & 0o4000 != 0
}

pub fn with_pkexec_lock<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = PKEXEC_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f()
}

/// Exécute l'élévation sur le thread GTK principal (requis pour PolicyKit / session D-Bus).
fn run_on_gtk_main_thread<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let ctx = glib::MainContext::default();
    if ctx.is_owner() {
        return f();
    }
    // Sans cette porte : quand un thread exécute f() en direct (fallback
    // « contexte sans propriétaire »), les appelants concurrents basculent
    // dans la file du contexte — jamais traitée faute de boucle principale
    // (tests unitaires, démarrage) → interblocage. La porte force un seul
    // exécutant direct à la fois ; le contexte est libéré avant le suivant.
    let _gate = GTK_INVOKE_GATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Ok(_owner) = ctx.acquire() {
        return f();
    }
    // Contexte possédé par la boucle GTK : mise en file et attente de son traitement.
    let (tx, rx) = mpsc::sync_channel(1);
    ctx.invoke(move || {
        let _ = tx.send(f());
    });
    rx.recv()
        .expect("elevation: thread GTK principal indisponible")
}

pub fn pkexec_output(args: &[&str]) -> std::io::Result<Output> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    run_on_gtk_main_thread(move || elevation_output_impl(&args))
}

/// Lance un processus privilégié avec stdout/stderr pipés (ex. shred longue durée).
pub fn pkexec_spawn(args: &[&str]) -> std::io::Result<Child> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    run_on_gtk_main_thread(move || elevation_spawn_impl(&args))
}

fn elevation_output_impl(args: &[String]) -> std::io::Result<Output> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    with_pkexec_lock(|| {
        let output = Command::new("timeout")
            .arg(PKEXEC_TIMEOUT_SECS)
            .arg("pkexec")
            .args(&refs)
            .output()?;
        update_session_from_status(&output.status);
        Ok(output)
    })
}

fn elevation_spawn_impl(args: &[String]) -> std::io::Result<Child> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    with_pkexec_lock(|| {
        let child = Command::new("pkexec")
            .args(&refs)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        mark_session_active();
        Ok(child)
    })
}

fn update_session_from_status(status: &std::process::ExitStatus) {
    if status.success() {
        mark_session_active();
    } else if was_elevation_denied(status) {
        clear_session_active();
    }
}

fn was_elevation_denied(status: &std::process::ExitStatus) -> bool {
    matches!(status.code(), Some(126) | Some(127))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_active_can_be_cleared() {
        mark_session_active();
        assert!(is_session_active());
        clear_session_active();
        assert!(!is_session_active());
    }

    #[test]
    fn pkexec_has_setuid_matches_which_metadata() {
        if let Ok(output) = Command::new("which").arg("pkexec").output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    let meta = std::fs::metadata(&path).expect("pkexec path metadata");
                    let setuid = meta.permissions().mode() & 0o4000 != 0;
                    assert_eq!(pkexec_has_setuid(), setuid);
                }
            }
        }
    }
}
