//! Journal de session lisible par un humain (`sessions/<ID>/` sous la racine
//! des journaux, voir [`crate::paths::logs_dir`]).

use crate::app_identity::APP_NAME;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const JOURNAL_TAIL_LINES: usize = 20;
const SEP_MAJOR: &str = "══════════════════════════════════════════════════════════════";
const SEP_MINOR: &str = "───────────────────────────────────────────────────────────────";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Complete,
    EraseFailed,
    VerifyFailed,
    Interrupted,
    NotVerified,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Complete => "PROCÉDURE COMPLÈTE",
            Verdict::EraseFailed => "ÉCHEC EFFACEMENT",
            Verdict::VerifyFailed => "ÉCHEC VÉRIFICATION",
            Verdict::Interrupted => "INTERROMPU",
            Verdict::NotVerified => "VÉRIFICATION NON EFFECTUÉE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub started_at: String,
    pub disks: Vec<String>,
    pub tools_line: String,
}

#[derive(Clone)]
struct DiskTrace {
    header_line: String,
    verdict: Option<Verdict>,
}

struct ActiveSession {
    session_dir: PathBuf,
    journal_path: PathBuf,
    disks: HashMap<String, DiskTrace>,
}

static SESSION: OnceLock<Mutex<Option<ActiveSession>>> = OnceLock::new();

fn session_lock() -> &'static Mutex<Option<ActiveSession>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

pub fn sessions_root() -> PathBuf {
    // Lot actif (annexe M) : journaux de session dans son dossier.
    crate::lot::active_dir()
        .map(|d| d.join(crate::lot::SESSIONS_DIR))
        .unwrap_or_else(|| crate::paths::logs_dir().join("sessions"))
}

pub fn session_dir(certificate_id: &str) -> PathBuf {
    sessions_root().join(certificate_id)
}

/// Ouvre (ou rouvre) le journal de la session `certificate_id`. Une
/// relance (« Relancer les échoués », disque ajouté sans rafraîchir) ne
/// tronque jamais un journal existant : un bloc « Reprise » est ajouté et
/// les disques nouveaux rejoignent la carte des disques suivis.
pub fn open_session(certificate_id: &str, meta: SessionMeta) -> Result<PathBuf, String> {
    let dir = session_dir(certificate_id);
    crate::certificate::create_private_dir(&dir)
        .map_err(|e| format!("Création dossier session: {e}"))?;
    let journal_path = dir.join("journal.log");
    let resumed = journal_path.exists();

    let mut header = String::new();
    header.push_str(SEP_MAJOR);
    header.push('\n');
    if resumed {
        header.push_str(&format!(
            "{APP_NAME} — Session {certificate_id} — reprise {}\n",
            crate::clock::stamp()
        ));
    } else {
        header.push_str(&format!("{APP_NAME} — Session {certificate_id}\n"));
        header.push_str(&format!("Démarrée : {}\n", meta.started_at));
    }
    header.push_str(&format!("Disques  : {}\n", meta.disks.join(", ")));
    header.push_str(&format!("Outils   : {}\n", meta.tools_line));
    header.push_str(SEP_MAJOR);
    header.push('\n');

    if resumed {
        append_to(&journal_path, &header)?;
    } else {
        write_file(&journal_path, &header)?;
    }

    let mut disks_map: HashMap<String, DiskTrace> = HashMap::new();
    if let Ok(guard) = session_lock().lock() {
        if let Some(active) = guard.as_ref().filter(|a| a.session_dir == dir) {
            disks_map = active.disks.clone();
        }
    }
    for d in &meta.disks {
        disks_map.entry(d.clone()).or_insert(DiskTrace {
            header_line: String::new(),
            verdict: None,
        });
    }

    if let Ok(mut guard) = session_lock().lock() {
        *guard = Some(ActiveSession {
            session_dir: dir.clone(),
            journal_path: journal_path.clone(),
            disks: disks_map,
        });
    }

    Ok(journal_path)
}

pub fn begin_disk_section(disk_name: &str, device_path: &str, subtitle: &str) {
    let line =
        format!("\n{SEP_MINOR}\n── DISQUE {disk_name} ({device_path}) — {subtitle} {SEP_MINOR}\n");
    let _ = append_journal(&line);
    if let Ok(mut guard) = session_lock().lock() {
        if let Some(session) = guard.as_mut() {
            if let Some(disk) = session.disks.get_mut(disk_name) {
                disk.header_line = format!("{disk_name} ({device_path}) — {subtitle}");
            }
        }
    }
}

pub fn log_timestamped(message: &str) {
    let stamp = crate::clock::now().format("%H:%M:%S");
    let _ = append_journal(&format!("[{stamp}] {message}\n"));
}

pub fn log_erase_start(disk_name: &str, method: &str) {
    log_timestamped(&format!("[{disk_name}] Effacement : {method}"));
    let _ = append_disk_file(
        disk_name,
        &format!("\n[{disk_name}] Démarrage effacement\nMéthode : {method}\n"),
    );
}

/// Fin d'un effacement : `outcome` court dans le journal, `detail` complet
/// dans le fichier du disque (rapport du helper).
pub fn log_erase_finished(disk_name: &str, outcome: &str, detail: &str) {
    log_timestamped(&format!(
        "[{disk_name}] Effacement : {outcome} — détails → {disk_name}.log"
    ));

    let tail = tail_lines(detail, JOURNAL_TAIL_LINES);
    if !tail.is_empty() {
        let mut block = String::from("           Dernières lignes :\n");
        for line in &tail {
            block.push_str("             ");
            block.push_str(line);
            block.push('\n');
        }
        let _ = append_journal(&block);
    }

    let mut disk_body = format!("\n── Rapport d'effacement ({outcome}) ──\n");
    push_indented_block(&mut disk_body, detail);
    let _ = append_disk_file(disk_name, &disk_body);
}

pub fn log_verify(disk_name: &str, ok: bool, detail: &str, output: Option<&str>) {
    let label = if ok { "OK" } else { "KO" };
    log_timestamped(&format!(
        "[{disk_name}] Vérification par relecture : {label} — {detail}"
    ));

    let mut disk_body =
        format!("\n── Vérification par relecture ──\nRésultat : {label} — {detail}\n");
    if let Some(out) = output.filter(|s| !s.trim().is_empty()) {
        disk_body.push_str("Sortie :\n");
        for line in out.lines() {
            disk_body.push_str("  ");
            disk_body.push_str(line);
            disk_body.push('\n');
        }
    }
    let _ = append_disk_file(disk_name, &disk_body);
}

pub fn log_testdisk_validation(
    disk_name: &str,
    result: &crate::verification::testdisk::TestDiskResult,
) {
    let label = if result.success {
        "Succès"
    } else {
        "Attention"
    };
    log_timestamped(&format!(
        "[{disk_name}] Validation TestDisk : {label} — {}",
        result.summary
    ));

    let mut disk_body = format!("\n{}\n", result.formatted_report);
    let log_for_disk = if !result.log_body.trim().is_empty() {
        result.log_body.as_str()
    } else if !result.raw_output.trim().is_empty() {
        result.raw_output.as_str()
    } else {
        ""
    };
    if !log_for_disk.is_empty() {
        disk_body.push_str("── testdisk.log ──\n");
        for line in log_for_disk.lines() {
            disk_body.push_str("  ");
            disk_body.push_str(line);
            disk_body.push('\n');
        }
    }
    let _ = append_disk_file(disk_name, &disk_body);

    if !result.log_body.trim().is_empty() {
        let _ = write_testdisk_session_file(disk_name, &result.log_body);
    }
}

fn write_testdisk_session_file(disk_name: &str, log_body: &str) -> Result<PathBuf, String> {
    let dir = session_lock()
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.session_dir.clone()))
        .ok_or_else(|| "Session trace inactive".to_string())?;
    let safe_name = disk_name.replace('/', "_");
    let path = dir.join(format!("testdisk_{safe_name}.log"));
    write_file(&path, log_body)?;
    Ok(path)
}

pub fn set_verdict(disk_name: &str, verdict: Verdict) {
    log_timestamped(&format!("[{disk_name}] VERDICT : {}", verdict.label()));
    if let Ok(mut guard) = session_lock().lock() {
        if let Some(session) = guard.as_mut() {
            if let Some(disk) = session.disks.get_mut(disk_name) {
                disk.verdict = Some(verdict);
            }
        }
    }
}

pub fn disk_log_path(disk_name: &str) -> Option<PathBuf> {
    session_lock().lock().ok().and_then(|g| {
        g.as_ref()
            .map(|s| s.session_dir.join(format!("{disk_name}.log")))
    })
}

pub fn journal_path() -> Option<PathBuf> {
    session_lock()
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.journal_path.clone()))
}

pub fn finalize_session() -> Option<PathBuf> {
    let (journal_path, bilan) = {
        let mut guard = session_lock().lock().ok()?;
        let session = guard.take()?;
        let complete = session
            .disks
            .values()
            .filter(|d| d.verdict == Some(Verdict::Complete))
            .count();
        let total = session.disks.len();
        let bilan = format!(
            "\n{SEP_MINOR}\n── BILAN ── {SEP_MINOR}\n{complete}/{total} disques : procédure complète\nJournal : {}\n",
            session.session_dir.display()
        );
        (session.journal_path, bilan)
    };
    let _ = append_journal(&bilan);
    Some(journal_path)
}

pub fn tail_lines(text: &str, max_lines: usize) -> Vec<String> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    if lines.len() <= max_lines {
        lines.into_iter().map(str::to_string).collect()
    } else {
        lines[lines.len() - max_lines..]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

fn push_indented_block(out: &mut String, text: &str) {
    if text.trim().is_empty() {
        out.push_str("  (vide)\n");
        return;
    }
    for line in text.lines() {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
}

fn append_to(path: &Path, text: &str) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("Journal {}: {e}", path.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|e| format!("Écriture {}: {e}", path.display()))
}

fn append_journal(text: &str) -> Result<(), String> {
    let path = journal_path().ok_or_else(|| "Session trace inactive".to_string())?;
    append_to(&path, text)
}

fn append_disk_file(disk_name: &str, text: &str) -> Result<(), String> {
    let path = disk_log_path(disk_name).ok_or_else(|| "Session trace inactive".to_string())?;
    append_to(&path, text)
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        crate::certificate::create_private_dir(parent).map_err(|e| format!("Dossier log: {e}"))?;
    }
    crate::certificate::write_private(path, content.as_bytes())
        .map_err(|e| format!("Écriture {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use std::fs;

    /// Sérialise les tests manipulant l'état global SESSION (course sinon).
    static SESSION_TESTS_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn tail_lines_keeps_last_n() {
        let text = (1..=25)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let tail = tail_lines(&text, 20);
        assert_eq!(tail.len(), 20);
        assert_eq!(tail[0], "line6");
        assert_eq!(tail[19], "line25");
    }

    #[test]
    fn smoke_session_journal_contains_verdict() {
        let _guard = SESSION_TESTS_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let id = format!("SEC-TEST-{}", Local::now().format("%Y%m%d%H%M%S"));
        let meta = SessionMeta {
            started_at: "2026-07-03 14:00:00".to_string(),
            disks: vec!["sda".to_string()],
            tools_line: "secmet test".to_string(),
        };
        let Ok(_) = open_session(&id, meta) else {
            eprintln!("skip smoke_session_journal_contains_verdict: dossier session inaccessible");
            return;
        };
        begin_disk_section("sda", "/dev/sda", "HDD 1 To");
        log_erase_start("sda", "HOST_OVERWRITE_1_ZERO");
        log_erase_finished(
            "sda",
            "terminé",
            "Issue : terminé\nValidation : 1003 blocs relus",
        );
        log_verify("sda", true, "aucune signature détectée", None);
        set_verdict("sda", Verdict::Complete);
        let Some(journal) = finalize_session() else {
            panic!("finalize_session should succeed after open_session");
        };
        let content = fs::read_to_string(&journal).unwrap();
        assert!(content.contains("VERDICT"));
        assert!(content.contains("PROCÉDURE COMPLÈTE"));
        assert!(content.contains("HOST_OVERWRITE_1_ZERO"));
        let _ = fs::remove_dir_all(session_dir(&id));
    }

    #[test]
    fn log_testdisk_writes_session_file() {
        let _guard = SESSION_TESTS_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let id = format!("SEC-TESTDISK-{}", Local::now().format("%Y%m%d%H%M%S%3f"));
        let meta = SessionMeta {
            started_at: "2026-07-16 15:00:00".to_string(),
            disks: vec!["sdc".to_string()],
            tools_line: "secmet test".to_string(),
        };
        let Ok(_) = open_session(&id, meta) else {
            eprintln!("skip log_testdisk_writes_session_file: dossier session inaccessible");
            return;
        };
        begin_disk_section("sdc", "/dev/sdc", "SSD 128 Go");
        let result = crate::verification::testdisk::TestDiskResult {
            success: true,
            partitions_found: false,
            filesystem_found: false,
            partition_table_found: false,
            summary: "Aucune partition".to_string(),
            formatted_report: "Validation TestDisk\n".to_string(),
            raw_output: "affichage".to_string(),
            log_body: "No partition found or selected for recovery\n".to_string(),
            version: "7.1".to_string(),
        };
        log_testdisk_validation("sdc", &result);
        let session = session_dir(&id);
        let td_path = session.join("testdisk_sdc.log");
        assert!(td_path.is_file(), "attendu {}", td_path.display());
        let content = fs::read_to_string(&td_path).unwrap();
        assert!(content.contains("No partition found"));
        let _ = finalize_session();
        let _ = fs::remove_dir_all(session);
    }

    #[test]
    fn derive_verdict_erase_ok_verify_ko() {
        assert_eq!(Verdict::Complete.label(), "PROCÉDURE COMPLÈTE");
        assert_eq!(Verdict::VerifyFailed.label(), "ÉCHEC VÉRIFICATION");
    }
}
