//! Types partagés de l'UI SecMet.
use crate::disk::Disk;
use gtk4::{Box as GtkBox, Button, CheckButton, Label, ProgressBar};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone)]
pub(super) struct DiskWidget {
    pub(super) disk: Disk,
    pub(super) checkbox: CheckButton,
    pub(super) subtitle_label: Label,
    pub(super) progress_bar: ProgressBar,
    pub(super) progress_text: Label,
    pub(super) progress_row: GtkBox,
    pub(super) status_badge: Button,
    pub(super) verify_label: Label,
    pub(super) verify_details_button: Button,
    pub(super) info_button: Button,
    pub(super) stop_button: Button,
    pub(super) container: GtkBox,
    /// Miroir de `disk.is_locked` pour le clic « i » (mis à jour à l'enrichissement).
    pub(super) is_ata_locked: Rc<Cell<bool>>,
    /// Gestionnaire du bouton Stop du lancement courant : remplacé à chaque
    /// lancement, jamais cumulé.
    pub(super) stop_handler: Rc<RefCell<Option<gtk4::glib::SignalHandlerId>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum JobState {
    Pending,
    Running,
    Success,
    FailedIo,
    FailedOther,
    /// SSD SATA frozen : non relançable tant que Dé-frozen n'a pas été fait.
    BlockedFrozen,
    /// ATA Security locked : non relançable sans mot de passe.
    BlockedLocked,
    Cancelled,
}

#[derive(Debug, Clone)]
pub(super) enum VerifyState {
    Ok,
    Ko,
    Error,
}

#[derive(Debug, Clone)]
pub(super) enum TestDiskState {
    Pending,
    Ok,
    Warn,
    Error,
    Cancelled,
}

#[derive(Debug, Clone)]
pub(super) struct JobRecord {
    pub(super) status: JobState,
    pub(super) attempts: Vec<String>,
    pub(super) verify_state: Option<VerifyState>,
    pub(super) verify_output: Option<String>,
    pub(super) testdisk_state: Option<TestDiskState>,
    pub(super) testdisk_output: Option<String>,
    pub(super) testdisk_summary: Option<String>,
}
