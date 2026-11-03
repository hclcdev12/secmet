//! Validation post-effacement via TestDisk (mode scripté non interactif),
//! exécuté par le helper sur le disque d'une session ré-identifiée. Ce module
//! ne garde que l'analyse de la sortie.

use regex::Regex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::helper_client::{self, TestdiskRun};

/// Message renvoyé lorsque l'opérateur interrompt l'analyse.
pub const CANCELLED_MSG: &str = "Analyse interrompue par l'opérateur";

/// Résultat structuré d'une analyse TestDisk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestDiskResult {
    pub success: bool,
    pub partitions_found: bool,
    pub filesystem_found: bool,
    pub partition_table_found: bool,
    pub summary: String,
    pub formatted_report: String,
    /// Texte affiché dans le dialogue (log propre, ou repli stdout/stderr).
    pub raw_output: String,
    /// Corps du journal TestDisk uniquement (vide si indisponible).
    pub log_body: String,
    pub version: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedSignals {
    partitions_found: bool,
    filesystem_found: bool,
    partition_table_found: bool,
}

static PARTITION_LINE_RE: OnceLock<Regex> = OnceLock::new();
static ANSI_ESCAPE_RE: OnceLock<Regex> = OnceLock::new();

fn partition_line_re() -> &'static Regex {
    PARTITION_LINE_RE
        .get_or_init(|| Regex::new(r"(?m)^\s*\d+\s+[PDEL*!]\s+\S").expect("partition line regex"))
}

fn ansi_escape_re() -> &'static Regex {
    ANSI_ESCAPE_RE
        .get_or_init(|| Regex::new(r"\x1b\[[0-9;]*[ -/]*[@-~]").expect("ansi escape regex"))
}

/// TestDisk est utilisable dès que la session helper est ouverte (le helper
/// signale un binaire absent au lancement).
pub fn is_available() -> bool {
    helper_client::is_active()
}

/// Handle d'une analyse en cours : jeton de session (pour `Cancel`) et drapeau.
#[derive(Debug, Clone, Default)]
pub struct TestDiskHandle {
    cancelled: Arc<AtomicBool>,
    token: Arc<Mutex<Option<String>>>,
}

impl TestDiskHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn set_token(&self, token: Option<String>) {
        if let Ok(mut t) = self.token.lock() {
            *t = token;
        }
    }
}

/// Interrompt l'analyse en cours (Cancel sur le jeton de session).
pub fn stop(handle: &TestDiskHandle) -> Result<(), String> {
    handle.cancelled.store(true, Ordering::SeqCst);
    let token = handle.token.lock().ok().and_then(|t| t.clone());
    let Some(token) = token else {
        return Ok(());
    };
    let helper =
        helper_client::current().ok_or_else(|| "Session administrateur absente".to_string())?;
    helper_client::cancel(&helper, &token)
        .map(|_| ())
        .map_err(String::from)
}

/// Comme [`run`], avec un handle pour permettre l'arrêt.
pub fn run_with_handle(device: &str, handle: &TestDiskHandle) -> Result<TestDiskResult, String> {
    handle.cancelled.store(false, Ordering::SeqCst);
    let name = device.strip_prefix("/dev/").unwrap_or(device).to_string();
    if !(name.starts_with("sd") || name.starts_with("nvme")) {
        return Err(format!("Périphérique invalide pour TestDisk: {device}"));
    }
    let helper = helper_client::ensure()?;
    let scan = helper_client::scan_device(&helper, &name, helper_client::default_preferences())
        .map_err(String::from)?;
    handle.set_token(Some(scan.token.clone()));
    if handle.is_cancelled() {
        handle.set_token(None);
        return Err(CANCELLED_MSG.to_string());
    }
    let run = helper_client::verify_testdisk(&helper, &scan.token, |_line| {});
    handle.set_token(None);
    match run {
        Ok(run) => result_from_run(&run, handle.is_cancelled()),
        Err(e) if handle.is_cancelled() => {
            let _ = e;
            Err(CANCELLED_MSG.to_string())
        }
        Err(e) => Err(format!("TestDisk : {e}")),
    }
}

/// Construit le résultat depuis l'exécution rapportée par le helper.
pub fn result_from_run(run: &TestdiskRun, cancelled: bool) -> Result<TestDiskResult, String> {
    if cancelled || run.cancelled {
        return Err(CANCELLED_MSG.to_string());
    }
    let version = if run.version.is_empty() {
        "inconnue".to_string()
    } else {
        run.version.clone()
    };
    let log_body = if run.log.trim().is_empty() {
        None
    } else {
        Some(run.log.clone())
    };
    let signals = parse_testdisk_output(log_body.as_deref().unwrap_or(&run.stdout));
    let display = build_display_output(log_body.as_deref(), &run.stdout, &run.stderr);
    build_result(&version, &signals, display, log_body.unwrap_or_default())
}

/// Nettoie une sortie terminal (ANSI, caractères de contrôle) pour affichage GTK.
pub fn sanitize_for_display(text: &str) -> String {
    let without_ansi = ansi_escape_re().replace_all(text, "");
    without_ansi
        .replace('\r', "\n")
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect::<String>()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_display_output(log_body: Option<&str>, stdout: &str, stderr: &str) -> String {
    if let Some(log) = log_body {
        sanitize_for_display(log)
    } else {
        sanitize_for_display(&format!(
            "⚠ testdisk.log indisponible — repli sur stdout/stderr\n\n\
             ── stdout ──\n{stdout}\n\
             ── stderr ──\n{stderr}"
        ))
    }
}

pub(crate) fn parse_testdisk_output(raw: &str) -> ParsedSignals {
    let lower = raw.to_lowercase();
    let mut signals = ParsedSignals::default();

    let no_partition = lower.contains("no partition")
        || lower.contains("no partition found")
        || lower.contains("partition table type: none")
        || lower.contains("partition table: none");

    if partition_line_re().is_match(raw) {
        signals.partitions_found = true;
    }

    if !no_partition {
        signals.partition_table_found = lower.contains("gpt")
            || lower.contains("efi gpt")
            || lower.contains("ms-dos")
            || lower.contains("mbr")
            || lower.contains("intel partition")
            || lower.contains("mac partition map")
            || lower.contains("partition table type");
    }

    // Si TestDisk conclut « No partition found », ne pas lever de FS
    // (évite les faux positifs de la bannière des libs compilées).
    if no_partition && !signals.partitions_found {
        signals.filesystem_found = false;
    } else {
        signals.filesystem_found = detect_filesystem_mentions(&lower);
    }

    if lower.contains("recoverable") && !no_partition {
        signals.partitions_found = true;
    }

    signals
}

/// Retire les lignes de bruit TestDisk (bannière libs, OS, compilateur)
/// qui citent des noms de FS sans qu'ils soient présents sur le disque.
fn strip_testdisk_banner_noise(lower: &str) -> String {
    lower
        .lines()
        .filter(|line| {
            let t = line.trim();
            if t.is_empty() {
                return false;
            }
            // Ex. "ext2fs lib: 1.47.0, ntfs lib: libntfs-3g, reiserfs lib: none"
            if t.contains("ext2fs lib")
                || t.contains("ntfs lib")
                || t.contains("reiserfs lib")
                || t.contains("ewf lib")
                || t.contains("curses lib")
            {
                return false;
            }
            if t.starts_with("compiler:") || t.starts_with("os:") {
                return false;
            }
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn detect_filesystem_mentions(lower: &str) -> bool {
    let text = strip_testdisk_banner_noise(lower);
    // Marqueurs avec bornes de mot pour éviter « ext2 » ⊂ « ext2fs ».
    static FS_RE: OnceLock<Regex> = OnceLock::new();
    let re = FS_RE.get_or_init(|| {
        Regex::new(
            r"(?x)
            \b(
                fat12 | fat16 | fat32 | exfat | ntfs |
                ext2 | ext3 | ext4 |
                hfs\+ | apfs | btrfs | xfs | zfs |
                reiserfs | hpfs | luks
            )\b
            | \bhfs\b
            | linux\s+swap
            ",
        )
        .expect("filesystem marker regex")
    });
    re.is_match(&text)
}

fn build_result(
    version: &str,
    signals: &ParsedSignals,
    raw_output: String,
    log_body: String,
) -> Result<TestDiskResult, String> {
    let structures_found =
        signals.partitions_found || signals.filesystem_found || signals.partition_table_found;
    let success = !structures_found;

    let summary = if success {
        "Aucune partition ou structure de disque récupérable détectée.".to_string()
    } else {
        "TestDisk a détecté une ou plusieurs structures récupérables.".to_string()
    };

    let formatted_report = format_report(version, signals, success);

    Ok(TestDiskResult {
        success,
        partitions_found: signals.partitions_found,
        filesystem_found: signals.filesystem_found,
        partition_table_found: signals.partition_table_found,
        summary,
        formatted_report,
        raw_output,
        log_body,
        version: version.to_string(),
    })
}

/// Texte complet pour le dialogue « Afficher les détails ».
pub fn details_output(result: &TestDiskResult) -> String {
    format!(
        "{}\n\n── testdisk.log ──\n{}",
        result.formatted_report, result.raw_output
    )
}

fn format_report(version: &str, signals: &ParsedSignals, success: bool) -> String {
    let (mark, part_line, fs_line, struct_line) = if success {
        (
            "✓",
            "✓ Aucune partition détectée",
            "✓ Aucun système de fichiers détecté",
            "✓ Aucune structure récupérable détectée",
        )
    } else {
        (
            "⚠",
            if signals.partitions_found {
                "⚠ Partition(s) détectée(s)"
            } else {
                "✓ Aucune partition détectée"
            },
            if signals.filesystem_found {
                "⚠ Système(s) de fichiers détecté(s)"
            } else {
                "✓ Aucun système de fichiers détecté"
            },
            if signals.partition_table_found || signals.partitions_found || signals.filesystem_found
            {
                "⚠ Des structures récupérables ont été détectées"
            } else {
                "✓ Aucune structure récupérable détectée"
            },
        )
    };

    format!(
        "Validation TestDisk\n\
         -------------------\n\
         Outil : TestDisk\n\
         Version : {version}\n\
         \n\
         Résultat :\n\
         {mark} État : {}\n\
         {part_line}\n\
         {fs_line}\n\
         {struct_line}\n",
        if success { "Succès" } else { "Attention" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_ansi_and_control_chars() {
        let raw = "Disk /dev/sda\x1b[31m - 500 GB\x1b[0m\n\x07No partition found";
        let clean = sanitize_for_display(raw);
        assert!(!clean.contains('\x1b'));
        assert!(!clean.contains('\x07'));
        assert!(clean.contains("Disk /dev/sda - 500 GB"));
        assert!(clean.contains("No partition found"));
    }

    #[test]
    fn parse_ignores_stderr_noise_when_log_present() {
        let log = r#"
TestDisk 7.2, Data Recovery Utility
Disk /dev/sda - 500 GB
No partition found
"#;
        let stderr = "\x1b[2J\x1b[HPartition table type: EFI GPT";
        let display = build_display_output(Some(log), "", stderr);
        assert!(display.contains("No partition found"));
        assert!(!display.contains("EFI GPT"));

        let signals = parse_testdisk_output(log);
        assert!(!signals.partitions_found);
        assert!(!signals.partition_table_found);
    }

    #[test]
    fn parse_empty_disk_output() {
        let raw = r#"
TestDisk 7.2, Data Recovery Utility
Disk /dev/sda - 500 GB
Analyse, partition None
No partition found
"#;
        let signals = parse_testdisk_output(raw);
        assert!(!signals.partitions_found);
        assert!(!signals.filesystem_found);
        let result = build_result("7.2", &signals, raw.to_string(), String::new()).unwrap();
        assert!(result.success);
        assert!(result.formatted_report.contains("Succès"));
    }

    #[test]
    fn parse_ignores_library_banner_false_positives() {
        // Log réel TestDisk 7.1 : disque effacé, mais bannière libs cite ext2/ntfs/reiserfs.
        let raw = r#"
Using locale 'fr_FR.UTF-8'.

Thu Jul 16 16:30:35 2026
Command line: TestDisk /log /logname /var/tmp/secmet-testdisk-x/testdisk.log /cmd /dev/sdc analyze,search

TestDisk 7.1, Data Recovery Utility, July 2019
Christophe GRENIER <grenier@cgsecurity.org>
https://www.cgsecurity.org
OS: Linux, kernel 6.8.0-106-generic x86_64
Compiler: GCC 13.2
ext2fs lib: 1.47.0, ntfs lib: libntfs-3g, reiserfs lib: none, ewf lib: none, curses lib: ncurses 6.4
Hard disk list
Disk /dev/sdc - 128 GB / 119 GiB - CHS 15566 255 63, sector size=512 - ATA HFS128G32TND-N21, FW:0P10

Partition table type defaults to Intel
Disk /dev/sdc - 128 GB / 119 GiB - ATA HFS128G32TND-N21
Partition table type: Intel

Analyse Disk /dev/sdc - 128 GB / 119 GiB - CHS 15566 255 63
Current partition structure:

Partition sector doesn't have the endmark 0xAA55

search_part()
Disk /dev/sdc - 128 GB / 119 GiB - CHS 15566 255 63

interface_write()

No partition found or selected for recovery

search_part()
Disk /dev/sdc - 128 GB / 119 GiB - CHS 15566 255 63

interface_write()

No partition found or selected for recovery

TestDisk exited normally.
"#;
        let signals = parse_testdisk_output(raw);
        assert!(!signals.partitions_found);
        assert!(!signals.filesystem_found);
        assert!(!signals.partition_table_found);
        let result = build_result("7.1", &signals, raw.to_string(), String::new()).unwrap();
        assert!(
            result.success,
            "doit être Succès malgré la bannière des libs"
        );
        assert!(result.formatted_report.contains("Succès"));
        assert!(result
            .formatted_report
            .contains("Aucun système de fichiers détecté"));
    }

    #[test]
    fn detect_filesystem_still_sees_real_ntfs_partition_line() {
        let raw = r#"
Disk /dev/sdb - 1000 GB
     1 P HPFS - NTFS              2048  1953523711  1953521664
Partition table type: EFI GPT
"#;
        let signals = parse_testdisk_output(raw);
        assert!(signals.partitions_found);
        assert!(signals.filesystem_found);
        assert!(signals.partition_table_found);
    }

    #[test]
    fn parse_partition_detected_output() {
        let raw = r#"
Disk /dev/sdb - 1000 GB
     1 P HPFS - NTFS              2048  1953523711  1953521664
Partition table type: EFI GPT
"#;
        let signals = parse_testdisk_output(raw);
        assert!(signals.partitions_found);
        assert!(signals.partition_table_found);
        let result = build_result("7.2", &signals, raw.to_string(), String::new()).unwrap();
        assert!(!result.success);
        assert!(result.summary.contains("structures récupérables"));
    }

    #[test]
    fn format_report_success_lines() {
        let signals = ParsedSignals::default();
        let report = format_report("7.2", &signals, true);
        assert!(report.contains("Validation TestDisk"));
        assert!(report.contains("✓ Aucune partition détectée"));
    }
}
