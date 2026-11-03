//! Enrichissement des disques au démarrage : `ScanDevice` par disque via le
//! helper (IDENTIFY, SMART, protections, verdict du moteur), chaque carte se
//! mettant à jour dès que son scan est prêt.
use gtk4::prelude::*;
use gtk4::{CheckButton, Label};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use crate::app_identity::PKG_NAME;
use crate::disk::Disk;
use crate::helper_client::{self, CallError, ScanResult};
use crate::i18n::tr;

use super::app_theme::set_info_button_style;
use super::disk_list::{disk_subtitle_text, set_status_badge, update_select_all_state};
use super::types::DiskWidget;

enum EnrichEvent {
    Scanned(usize, Box<ScanResult>),
    ScanFailed(usize, CallError),
    Done,
    Failed(String),
}

/// Applique un scan à un disque ; retourne le style du bouton « i ».
pub(super) fn enrich_disk_from_scan(
    mut disk: Disk,
    scan: Option<&ScanResult>,
) -> (Disk, &'static str) {
    let mut info_style = "info-unknown";
    if let Some(scan) = scan {
        disk.apply_scan(scan);
        if let Some(recovery) = &scan.recovery {
            eprintln!(
                "[{}] levée du mot de passe ATA : {} — {}",
                disk.name, recovery.status, recovery.detail
            );
        }
        if !scan.disk.inspection_failed {
            info_style = if scan.disk.health.smart_failing {
                "info-bad"
            } else {
                "info-good"
            };
        }
        if disk.is_locked == Some(true) {
            info_style = "info-bad";
        }
    }
    (disk, info_style)
}

fn apply_enrichment_to_widget(
    disk_widgets: &Rc<RefCell<Vec<DiskWidget>>>,
    index: usize,
    disk: Disk,
    info_style: &'static str,
) {
    let mut widgets = disk_widgets.borrow_mut();
    if let Some(widget) = widgets.get_mut(index) {
        widget.disk = disk;
        widget
            .is_ata_locked
            .set(widget.disk.is_locked == Some(true));
        let (can_shred, reason) = widget.disk.can_be_shredded();
        widget
            .subtitle_label
            .set_text(&disk_subtitle_text(&widget.disk, can_shred, &reason));
        widget.checkbox.set_sensitive(can_shred);
        if widget.disk.is_locked == Some(true) {
            set_status_badge(&widget.status_badge, "LOCKED", "status-failed");
        } else if can_shred {
            set_status_badge(&widget.status_badge, &tr("INACTIF"), "status-idle");
        } else {
            set_status_badge(&widget.status_badge, &tr("INDISPONIBLE"), "status-failed");
        }
        set_info_button_style(&widget.info_button, info_style);
    }
}

fn mark_cards_analyzing(disk_widgets: &Rc<RefCell<Vec<DiskWidget>>>) {
    let mut widgets = disk_widgets.borrow_mut();
    for widget in widgets.iter_mut() {
        set_status_badge(&widget.status_badge, &tr("ANALYSE…"), "status-pending");
        let (can_shred, reason) = widget.disk.can_be_shredded();
        let mut sub = disk_subtitle_text(&widget.disk, can_shred, &reason);
        sub.push_str(&tr(" — Inspection…"));
        widget.subtitle_label.set_text(&sub);
        widget.checkbox.set_sensitive(false);
    }
}

fn finalize_missing(disk_widgets: &Rc<RefCell<Vec<DiskWidget>>>, received: &HashSet<usize>) {
    let count = disk_widgets.borrow().len();
    for idx in 0..count {
        if received.contains(&idx) {
            continue;
        }
        let disk = disk_widgets.borrow().get(idx).map(|w| w.disk.clone());
        if let Some(disk) = disk {
            eprintln!("[{PKG_NAME}] {} : pas de scan, repli inventaire", disk.name);
            let (disk, style) = enrich_disk_from_scan(disk, None);
            apply_enrichment_to_widget(disk_widgets, idx, disk, style);
        }
    }
}

thread_local! {
    static ENRICH_GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// Rend caduc tout enrichissement en cours : ses résultats ne seront plus
/// appliqués (les cartes qu'il visait vont être retirées).
pub(super) fn cancel_enrichment() {
    ENRICH_GENERATION.with(|g| g.set(g.get().wrapping_add(1)));
}

/// Scan de chaque disque via le helper, mises à jour carte par carte.
pub(super) fn spawn_disk_enrichment(
    disk_widgets: Rc<RefCell<Vec<DiskWidget>>>,
    smart_auth_ok: Rc<Cell<bool>>,
    select_all_button: CheckButton,
    select_all_updating: Rc<Cell<bool>>,
    selected_total_label: Label,
    operations_active: Rc<Cell<bool>>,
) -> Result<(), String> {
    mark_cards_analyzing(&disk_widgets);

    let names: Vec<(usize, String)> = disk_widgets
        .borrow()
        .iter()
        .enumerate()
        .map(|(i, w)| (i, w.disk.name.clone()))
        .collect();
    if names.is_empty() {
        return Ok(());
    }

    let helper = helper_client::current().ok_or_else(|| {
        "Session administrateur absente.\n\
         Relancez SecMet et validez le mot de passe au démarrage."
            .to_string()
    })?;

    // Génération : un « Rafraîchir » pendant un enrichissement en cours
    // remplace la liste des cartes ; les résultats de l'ancien passage ne
    // doivent plus être appliqués par index sur les nouvelles cartes.
    let generation = ENRICH_GENERATION.with(|g| {
        g.set(g.get().wrapping_add(1));
        g.get()
    });

    let (tx, rx) = mpsc::channel::<EnrichEvent>();
    std::thread::spawn(move || {
        let preferences = helper_client::default_preferences();
        for (idx, name) in names {
            match helper_client::scan_device(&helper, &name, preferences) {
                Ok(scan) => {
                    let _ = tx.send(EnrichEvent::Scanned(idx, Box::new(scan)));
                }
                // Liaison rompue : le helper est parti, inutile d'insister.
                Err(e) if e.kind == "transport" => {
                    let _ = tx.send(EnrichEvent::Failed(e.message));
                    return;
                }
                // Délai dépassé sur ce disque (firmware lent) : on passe au
                // suivant, la carte garde le repli inventaire.
                Err(e) => {
                    let _ = tx.send(EnrichEvent::ScanFailed(idx, e));
                }
            }
        }
        let _ = tx.send(EnrichEvent::Done);
    });

    let received = Rc::new(RefCell::new(HashSet::<usize>::new()));

    glib::timeout_add_local(Duration::from_millis(50), move || {
        if ENRICH_GENERATION.with(|g| g.get()) != generation {
            return glib::ControlFlow::Break;
        }
        loop {
            match rx.try_recv() {
                Ok(EnrichEvent::Scanned(idx, scan)) => {
                    smart_auth_ok.set(true);
                    received.borrow_mut().insert(idx);
                    let disk = disk_widgets
                        .borrow()
                        .get(idx)
                        .filter(|w| w.disk.name == scan.disk.name)
                        .map(|w| w.disk.clone());
                    if let Some(disk) = disk {
                        let (disk, style) = enrich_disk_from_scan(disk, Some(&scan));
                        apply_enrichment_to_widget(&disk_widgets, idx, disk, style);
                    }
                }
                Ok(EnrichEvent::ScanFailed(idx, error)) => {
                    received.borrow_mut().insert(idx);
                    let disk = disk_widgets.borrow().get(idx).map(|w| w.disk.clone());
                    if let Some(mut disk) = disk {
                        eprintln!("[{PKG_NAME}] scan {} : {}", disk.name, error.message);
                        disk.inspection_failed = true;
                        disk.inspection_error = Some(error.message.clone());
                        if error.kind == "busy" {
                            disk.is_mounted = true;
                            disk.protection_summary = error.message.clone();
                        }
                        apply_enrichment_to_widget(&disk_widgets, idx, disk, "info-unknown");
                    }
                }
                Ok(EnrichEvent::Done) => {
                    smart_auth_ok.set(true);
                    finalize_missing(&disk_widgets, &received.borrow());
                    if !operations_active.get() {
                        update_select_all_state(
                            &disk_widgets.borrow(),
                            &select_all_button,
                            select_all_updating.as_ref(),
                            &selected_total_label,
                        );
                    }
                    return glib::ControlFlow::Break;
                }
                Ok(EnrichEvent::Failed(e)) => {
                    eprintln!("❌ Enrichissement disques: {e}");
                    smart_auth_ok.set(false);
                    finalize_missing(&disk_widgets, &received.borrow());
                    return glib::ControlFlow::Break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            }
        }
        glib::ControlFlow::Continue
    });

    Ok(())
}
