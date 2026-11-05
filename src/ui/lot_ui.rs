//! Lots persistants côté interface (Annexe M) : dialogue au démarrage
//! (continuer, clôturer, ouvrir), bandeau dans la fenêtre, contrôle de
//! doublon avant un effacement, enregistrement de la machine au lancement,
//! certificats automatiques en fin d'effacement, clôture (PDF groupé,
//! manifeste signé, copie sur la partition d'export, archivage).
//!
//! Actif seulement avec un dossier de données dédié (clé USB,
//! `SECMET_DATA_DIR`) : sur un banc installé, la session reste le lot.
//! Le lot actif vit dans un `thread_local` du fil GTK ; son dossier est
//! publié dans [`crate::lot`] pour que journaux et certificats y aillent.

use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Box as GtkBox, Button, Dialog, Entry, Label, Orientation};
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use secmet_attest::sign::SignedAttestation;
use secmet_attest::{export, manifest, sha256_hex};

use crate::certificate::{self, CertificatePdfMode};
use crate::disk::Disk;
use crate::i18n::{self, tr, trf};
use crate::lot::{self, CertificateRef, DiskIdentity, EntryStatus, Lot, LotScope, MachineIdentity};
use crate::{helper_client, paths, pdf_logo, settings};

use super::app_theme::apply_color_scheme_class;
use super::dialogs::show_error_dialog;

thread_local! {
    static ACTIVE: RefCell<Option<Lot>> = const { RefCell::new(None) };
    static CURRENT_ENTRY: Cell<Option<u32>> = const { Cell::new(None) };
    static PENDING_REPLACE: Cell<Option<u32>> = const { Cell::new(None) };
    static BANNER: RefCell<Option<glib::WeakRef<GtkBox>>> = const { RefCell::new(None) };
}

/// Lots : toujours sur la clé (dossier de données dédié), sinon sur
/// demande (Réglages, « Mode lot »).
pub(super) fn enabled() -> bool {
    paths::dedicated_data_dir() || settings::load().lots_enabled
}

/// Portée des lots de ce poste : machines sur la clé live, disques sur un
/// poste installé (la machine est toujours le banc).
pub(super) fn scope_here() -> LotScope {
    if paths::dedicated_data_dir() {
        LotScope::Machines
    } else {
        LotScope::Disks
    }
}

/// « 7 machine(s) » ou « 16 disque(s) » selon la portée du lot.
fn units_text(lot: &Lot) -> String {
    let count = lot.units_count().to_string();
    match lot.state().scope {
        LotScope::Machines => trf("{count} machine(s)", &[("count", &count)]),
        LotScope::Disks => trf("{count} disque(s)", &[("count", &count)]),
    }
}

/// Dossier à ouvrir pour « voir les certificats » : le lot en cours, sinon
/// sur la clé les lots clôturés (`archive/`), sinon les certificats du
/// poste.
pub(super) fn certificates_folder() -> PathBuf {
    if let Some(dir) = with_active(|lot| lot.certificates_dir()) {
        return dir;
    }
    if enabled() {
        let archive = paths::archive_dir();
        if archive.is_dir() {
            return archive;
        }
    }
    certificate::certificates_dir()
}

/// Nombre de certificats déjà enregistrés au lot pour cette session
/// (machine conclue) ; `None` si la session n'y est pas enregistrée.
pub(super) fn session_registered(session_id: &str) -> Option<usize> {
    with_active(|lot| {
        lot.state()
            .entries
            .iter()
            .filter(|e| e.session_id == session_id && e.status != EntryStatus::Replaced)
            .filter(|e| matches!(e.status, EntryStatus::Finished | EntryStatus::Failed))
            .map(|e| e.certificates.len())
            .find(|n| *n > 0)
    })
    .flatten()
}

/// « Générer le PDF » alors que les certificats de la machine sont déjà
/// au lot : les régénérer écraserait les fichiers enregistrés (empreinte
/// différente à la clôture). On propose d'ouvrir le dossier.
pub(super) fn show_already_registered_dialog(window: &ApplicationWindow, count: usize) {
    let (dialog, content, buttons) = dialog_frame(window, &tr("Certificats déjà produits"));
    content.append(&note(&trf(
        "Les certificats de cette machine ont déjà été produits et enregistrés au lot \
         ({count} fichier(s)). Les régénérer les rendrait différents de ceux du journal du lot.",
        &[("count", &count.to_string())],
    )));
    let open = Button::with_label(&tr("Ouvrir le dossier"));
    let close = Button::with_label(&tr("Fermer"));
    close.add_css_class("suggested-action");
    buttons.append(&open);
    buttons.append(&close);
    content.append(&buttons);
    {
        let window = window.clone();
        open.connect_clicked(move |_| {
            if let Err(e) = certificate::open_folder(&certificates_folder()) {
                show_error_dialog(&window, &tr("Lot"), &e);
            }
        });
    }
    {
        let dialog = dialog.clone();
        close.connect_clicked(move |_| dialog.close());
    }
    dialog.present();
}

/// Certificat dont le fichier ne correspond plus à l'empreinte du journal.
struct StaleCertificate {
    entry: u32,
    certificate_id: String,
    file: String,
    sha256: String,
}

/// Contrôle d'intégrité avant clôture. Un fichier régénéré (bouton
/// « Générer le PDF » après l'enregistrement) est accepté s'il est signé
/// par la clé de ce poste et désigne la même session ; sinon erreur.
fn stale_certificates(lot: &Lot) -> Result<Vec<StaleCertificate>, String> {
    let dir = lot.certificates_dir();
    let identity = certificate::signing_identity()?;
    let trusted = vec![identity.public_key_hex()];
    let mut stale = Vec::new();
    for entry in lot
        .state()
        .entries
        .iter()
        .filter(|e| e.status != EntryStatus::Replaced)
    {
        for r in &entry.certificates {
            let path = dir.join(&r.file);
            let text =
                fs::read_to_string(&path).map_err(|e| format!("{} : {e}", path.display()))?;
            let envelope: SignedAttestation =
                serde_json::from_str(&text).map_err(|e| format!("{} : {e}", path.display()))?;
            let digest = manifest::envelope_sha256(&envelope).map_err(|e| e.to_string())?;
            if digest == r.sha256 {
                continue;
            }
            let same_origin = envelope.payload.certificate_id == r.certificate_id
                && envelope.payload.session_id == entry.session_id
                && secmet_attest::sign::verify(&envelope, &trusted)
                    .map(|v| v.trusted)
                    .unwrap_or(false);
            if !same_origin {
                return Err(trf(
                    "{file} : signé par une autre clé ou pour une autre machine, clôture impossible",
                    &[("file", &r.file)],
                ));
            }
            stale.push(StaleCertificate {
                entry: entry.id,
                certificate_id: r.certificate_id.clone(),
                file: r.file.clone(),
                sha256: digest,
            });
        }
    }
    Ok(stale)
}

/// Un lot est ouvert dans cette session.
pub(super) fn is_active() -> bool {
    ACTIVE.with(|a| a.borrow().is_some())
}

fn set_active(lot: Option<Lot>) {
    lot::set_active_dir(lot.as_ref().map(|l| l.dir().to_path_buf()));
    ACTIVE.with(|a| *a.borrow_mut() = lot);
    refresh_banner();
}

fn with_active<R>(f: impl FnOnce(&mut Lot) -> R) -> Option<R> {
    ACTIVE.with(|a| a.borrow_mut().as_mut().map(f))
}

fn machine_identity() -> MachineIdentity {
    helper_client::current()
        .and_then(|h| helper_client::host_info(&h).ok())
        .map(|h| MachineIdentity {
            manufacturer: h.manufacturer,
            model: h.model,
            serial: h.serial,
            hostname: h.hostname,
        })
        .unwrap_or_default()
}

fn disk_identities(disks: &[Disk]) -> Vec<DiskIdentity> {
    disks
        .iter()
        .map(|d| DiskIdentity {
            name: d.name.clone(),
            model: d.model.clone(),
            serial: d.serial.clone(),
            size_bytes: (d.size_bytes > 0).then_some(d.size_bytes),
        })
        .collect()
}

fn status_label(status: EntryStatus) -> String {
    tr(match status {
        EntryStatus::InProgress => "statut : en cours",
        EntryStatus::Finished => "statut : terminée",
        EntryStatus::Failed => "statut : en échec",
        EntryStatus::Interrupted => "statut : interrompue",
        EntryStatus::Replaced => "statut : remplacée",
    })
}

// ── Bandeau ─────────────────────────────────────────────────────────────────

/// Bandeau « Lot … » avec le bouton Clôturer ; invisible sans lot actif.
pub(super) fn banner(window: &ApplicationWindow) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.add_css_class("lot-banner");
    row.set_visible(false);
    let label = Label::new(None);
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);
    let close_btn = Button::with_label(&tr("Clôturer"));
    close_btn.set_valign(gtk4::Align::Center);
    row.append(&close_btn);
    {
        let window = window.clone();
        close_btn.connect_clicked(move |_| close_dialog(&window, None));
    }
    BANNER.with(|slot| *slot.borrow_mut() = Some(row.downgrade()));
    refresh_banner();
    row
}

fn banner_text() -> Option<String> {
    with_active(|lot| {
        let s = lot.state();
        let mut text = trf(
            "Lot {id} · {operator} · {units}",
            &[
                ("id", &s.id),
                ("operator", &s.operator),
                ("units", &units_text(lot)),
            ],
        );
        if !s.reference.is_empty() {
            text.push_str(" · ");
            text.push_str(&trf(
                "Référence : {reference}",
                &[("reference", &s.reference)],
            ));
        }
        text
    })
}

pub(super) fn refresh_banner() {
    let text = banner_text();
    BANNER.with(|slot| {
        let Some(row) = slot.borrow().as_ref().and_then(|w| w.upgrade()) else {
            return;
        };
        if let Some(label) = row.first_child().and_downcast::<Label>() {
            label.set_text(text.as_deref().unwrap_or_default());
        }
        row.set_visible(text.is_some());
    });
}

// ── Dialogues ───────────────────────────────────────────────────────────────

fn dialog_frame(window: &ApplicationWindow, title: &str) -> (Dialog, GtkBox, GtkBox) {
    let dialog = Dialog::builder()
        .title(title)
        .modal(true)
        .transient_for(window)
        .build();
    apply_color_scheme_class(&dialog);
    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    content.set_margin_start(18);
    content.set_margin_end(18);
    let buttons = GtkBox::new(Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    buttons.set_margin_top(6);
    dialog.set_child(Some(&content));
    dialog.set_default_size(560, -1);
    (dialog, content, buttons)
}

/// Dialogue d'attente (roue + texte), sans bouton : fermé par l'appelant.
fn busy_dialog(window: &ApplicationWindow, title: &str, text: &str) -> Dialog {
    let (dialog, content, _buttons) = dialog_frame(window, title);
    dialog.set_deletable(false);
    let row = GtkBox::new(Orientation::Horizontal, 12);
    let spinner = gtk4::Spinner::new();
    spinner.set_size_request(28, 28);
    spinner.start();
    row.append(&spinner);
    let label = note(text);
    label.set_valign(gtk4::Align::Center);
    row.append(&label);
    content.append(&row);
    dialog.set_default_size(480, -1);
    dialog.present();
    dialog
}

fn note(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.set_wrap(true);
    l.set_xalign(0.0);
    l
}

type Done = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

fn once(done: &Done) {
    if let Some(f) = done.borrow_mut().take() {
        f();
    }
}

/// Au démarrage : reprend le lot ouvert ou propose d'en ouvrir un, puis
/// `on_done` (chargement de l'inventaire). Sans dossier dédié : `on_done`
/// tout de suite.
pub(super) fn startup(window: &ApplicationWindow, on_done: Box<dyn FnOnce()>) {
    if !enabled() {
        on_done();
        return;
    }
    // Horloge d'abord (§M.7) : les horodatages du lot en dépendent.
    let window_lot = window.clone();
    super::clock_ui::startup(window, Box::new(move || startup_lot(&window_lot, on_done)));
}

fn startup_lot(window: &ApplicationWindow, on_done: Box<dyn FnOnce()>) {
    match Lot::find_open(&paths::lots_dir()) {
        Err(e) => {
            show_error_dialog(window, &tr("Lot"), &e);
            on_done();
        }
        Ok(Some(mut lot)) => {
            let interrupted = lot.mark_interrupted().unwrap_or_default();
            show_resume_dialog(window, lot, interrupted.len(), on_done);
        }
        Ok(None) => show_open_dialog(window, on_done),
    }
}

fn show_open_dialog(window: &ApplicationWindow, on_done: Box<dyn FnOnce()>) {
    let (dialog, content, buttons) = dialog_frame(window, &tr("Ouvrir un lot"));
    content.append(&note(&match scope_here() {
        LotScope::Machines => tr(
            "Un lot regroupe les machines traitées successivement avec cette clé. Les \
             certificats de chaque machine y sont enregistrés ; à la clôture, un certificat \
             groupé et un manifeste signé sont produits.",
        ),
        LotScope::Disks => tr(
            "Un lot regroupe les disques effacés sur ce poste, par sessions successives \
             (brancher, effacer, débrancher, recommencer). Les certificats de chaque disque y \
             sont enregistrés ; à la clôture, un certificat groupé et un manifeste signé sont \
             produits.",
        ),
    }));
    let operator_title = Label::new(Some(&tr("Opérateur du lot")));
    operator_title.set_xalign(0.0);
    content.append(&operator_title);
    let operator = Entry::new();
    operator.set_text(&settings::load().operator_name);
    content.append(&operator);
    let reference_title = Label::new(Some(&tr("Référence client (facultatif)")));
    reference_title.set_xalign(0.0);
    content.append(&reference_title);
    let reference = Entry::new();
    content.append(&reference);
    let status = Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);
    content.append(&status);

    let without = Button::with_label(&tr("Sans lot"));
    let open = Button::with_label(&tr("Ouvrir"));
    open.add_css_class("suggested-action");
    buttons.append(&without);
    buttons.append(&open);
    content.append(&buttons);

    let done: Done = Rc::new(RefCell::new(Some(on_done)));
    {
        let dialog = dialog.clone();
        let done = done.clone();
        without.connect_clicked(move |_| {
            dialog.close();
            once(&done);
        });
    }
    {
        let dialog = dialog.clone();
        let done = done.clone();
        open.connect_clicked(move |_| {
            let op = operator.text().trim().to_string();
            if op.is_empty() {
                status.set_text(&tr("Indiquez l'opérateur du lot."));
                return;
            }
            match Lot::open(
                &paths::lots_dir(),
                &op,
                reference.text().trim(),
                &certificate::software_line(),
                scope_here(),
            ) {
                Ok(lot) => {
                    // Premier lot sur la clé : l'opérateur est retenu pour
                    // les suivants (pré-rempli, modifiable dans les Réglages).
                    let mut saved = settings::load();
                    if saved.operator_name.is_empty() {
                        saved.operator_name = op.clone();
                        settings::save(&saved);
                    }
                    set_active(Some(lot));
                    dialog.close();
                    once(&done);
                }
                Err(e) => {
                    status.set_text(&trf("Lot ouvert impossible : {error}", &[("error", &e)]))
                }
            }
        });
    }
    {
        let done = done.clone();
        dialog.connect_close_request(move |_| {
            once(&done);
            glib::Propagation::Proceed
        });
    }
    dialog.present();
}

fn show_resume_dialog(
    window: &ApplicationWindow,
    lot: Lot,
    interrupted: usize,
    on_done: Box<dyn FnOnce()>,
) {
    let (dialog, content, buttons) = dialog_frame(window, &tr("Lot en cours"));
    let s = lot.state();
    content.append(&note(&trf(
        "Lot {id} — {operator} — {units}, ouvert le {date}",
        &[
            ("id", &s.id),
            ("operator", &s.operator),
            ("units", &units_text(&lot)),
            ("date", &s.opened_at),
        ],
    )));
    if !s.reference.is_empty() {
        content.append(&note(&trf(
            "Référence : {reference}",
            &[("reference", &s.reference)],
        )));
    }
    if interrupted > 0 {
        let count = interrupted.to_string();
        let text = match lot.state().scope {
            LotScope::Machines => trf(
                "{count} machine(s) interrompue(s) (coupure ou arrêt pendant l'effacement) : à \
                 refaire ou à consigner en échec.",
                &[("count", &count)],
            ),
            LotScope::Disks => trf(
                "{count} session(s) interrompue(s) (coupure ou arrêt pendant l'effacement) : \
                 disques à refaire ou à consigner en échec.",
                &[("count", &count)],
            ),
        };
        let warn = note(&text);
        warn.add_css_class("host-notice");
        content.append(&warn);
    }
    let new_lot = Button::with_label(&tr("Nouveau lot"));
    new_lot.set_sensitive(false);
    new_lot.set_tooltip_text(Some(&tr("Clôturez d'abord le lot en cours")));
    let close = Button::with_label(&tr("Clôturer"));
    let resume = Button::with_label(&tr("Continuer"));
    resume.add_css_class("suggested-action");
    buttons.append(&new_lot);
    buttons.append(&close);
    buttons.append(&resume);
    content.append(&buttons);

    set_active(Some(lot));
    let done: Done = Rc::new(RefCell::new(Some(on_done)));
    {
        let dialog = dialog.clone();
        let done = done.clone();
        resume.connect_clicked(move |_| {
            dialog.close();
            once(&done);
        });
    }
    {
        let dialog = dialog.clone();
        let window = window.clone();
        let done = done.clone();
        close.connect_clicked(move |_| {
            dialog.close();
            let done = done.clone();
            let window_after = window.clone();
            close_dialog(
                &window,
                Some(Box::new(move || {
                    // Lot clôturé (ou clôture annulée) : proposer d'en ouvrir un
                    // s'il n'y en a plus, sinon continuer.
                    if is_active() {
                        once(&done);
                    } else if let Some(f) = done.borrow_mut().take() {
                        show_open_dialog(&window_after, f);
                    }
                })),
            );
        });
    }
    {
        let done = done.clone();
        dialog.connect_close_request(move |_| {
            once(&done);
            glib::Propagation::Proceed
        });
    }
    dialog.present();
}

/// Avant un effacement : si la machine est déjà dans le lot, demander
/// quoi faire ; puis `proceed`. Sans lot actif : `proceed` directement.
pub(super) fn check_before_erase(
    window: &ApplicationWindow,
    disks: &[Disk],
    proceed: Box<dyn FnOnce()>,
) {
    if !is_active() {
        proceed();
        return;
    }
    let machine = machine_identity();
    let identities = disk_identities(disks);
    let previous = with_active(|lot| {
        lot.find_previous(&machine, &identities)
            .map(|e| (e.id, e.status, e.started_at.clone()))
    })
    .flatten();
    let Some((id, status, at)) = previous else {
        PENDING_REPLACE.set(None);
        proceed();
        return;
    };
    let (dialog, content, buttons) = dialog_frame(window, &tr("Machine déjà dans le lot"));
    content.append(&note(&trf(
        "Cette machine figure déjà dans le lot : entrée n°{n} ({status}), le {date}.",
        &[
            ("n", &id.to_string()),
            ("status", &status_label(status)),
            ("date", &at),
        ],
    )));
    let cancel = Button::with_label(&tr("Annuler"));
    let add = Button::with_label(&tr("Ajouter quand même"));
    let replace = Button::with_label(&tr("Remplacer"));
    replace.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&add);
    buttons.append(&replace);
    content.append(&buttons);
    let proceed: Done = Rc::new(RefCell::new(Some(proceed)));
    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let dialog = dialog.clone();
        let proceed = proceed.clone();
        add.connect_clicked(move |_| {
            PENDING_REPLACE.set(None);
            dialog.close();
            once(&proceed);
        });
    }
    {
        let dialog = dialog.clone();
        replace.connect_clicked(move |_| {
            PENDING_REPLACE.set(Some(id));
            dialog.close();
            once(&proceed);
        });
    }
    dialog.present();
}

/// Au lancement effectif : écrit l'entrée machine dans le journal du lot
/// (avant le premier bloc effacé).
pub(super) fn register_start(disks: &[Disk]) {
    if !is_active() {
        return;
    }
    let session_id = certificate::current_session()
        .map(|s| s.certificate_id)
        .unwrap_or_default();
    let machine = machine_identity();
    let identities = disk_identities(disks);
    let result = with_active(|lot| {
        let id = lot.start_machine(machine, identities, &session_id)?;
        if let Some(old) = PENDING_REPLACE.take() {
            lot.replace(old, id)?;
        }
        Ok::<u32, String>(id)
    });
    match result {
        Some(Ok(id)) => CURRENT_ENTRY.set(Some(id)),
        Some(Err(e)) => eprintln!("Lot : enregistrement impossible : {e}"),
        None => {}
    }
    refresh_banner();
}

/// En fin de session : certificats de la machine (une page par disque,
/// opérateur du lot) dans le dossier du lot, puis entrée conclue.
pub(super) fn register_end(window: &ApplicationWindow, status: &Label) {
    let Some(entry) = CURRENT_ENTRY.take() else {
        return;
    };
    let Some(session) = certificate::current_session() else {
        return;
    };
    let Some((operator, dir)) =
        with_active(|lot| (lot.state().operator.clone(), lot.certificates_dir()))
    else {
        return;
    };
    let finished: Vec<String> = session
        .disks
        .iter()
        .filter(|d| d.is_finished())
        .map(|d| d.disk_name.clone())
        .collect();
    let all_ok = !finished.is_empty() && session.disks.iter().all(|d| d.is_success());
    let outcome = if all_ok {
        EntryStatus::Finished
    } else {
        EntryStatus::Failed
    };
    if finished.is_empty() {
        if let Some(Err(e)) = with_active(|lot| lot.finish_machine(entry, outcome, Vec::new())) {
            eprintln!("Lot : {e}");
        }
        refresh_banner();
        return;
    }
    status.set_text(&tr("Certificats du lot en cours de génération…"));
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(certificate::render_pdf_bundle_in(
            &dir,
            &session,
            &finished,
            &operator,
            CertificatePdfMode::PerDisk,
        ));
    });
    let window = window.clone();
    let status = status.clone();
    glib::timeout_add_local(Duration::from_millis(150), move || {
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => Err("génération interrompue".into()),
        };
        let refs: Vec<CertificateRef> = match &result {
            Ok(exported) => exported
                .signed
                .iter()
                .filter_map(|s| {
                    Some(CertificateRef {
                        certificate_id: s.payload.certificate_id.clone(),
                        file: format!("{}.json", export::safe_filename(&s.payload.certificate_id)),
                        sha256: manifest::envelope_sha256(s).ok()?,
                    })
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        let recorded = with_active(|lot| {
            let st = if result.is_ok() {
                outcome
            } else {
                EntryStatus::Failed
            };
            lot.finish_machine(entry, st, refs.clone())
                .map(|()| lot.id().to_string())
        });
        match (result, recorded) {
            (Ok(_), Some(Ok(id))) => status.set_text(&trf(
                "Machine enregistrée au lot {id} ({count} certificat(s))",
                &[("id", &id), ("count", &refs.len().to_string())],
            )),
            (Err(e), _) | (_, Some(Err(e))) => {
                status.set_text("");
                show_error_dialog(
                    &window,
                    &tr("Lot"),
                    &trf(
                        "Enregistrement au lot impossible : {error}",
                        &[("error", &e)],
                    ),
                );
            }
            (Ok(_), None) => status.set_text(""),
        }
        refresh_banner();
        glib::ControlFlow::Break
    });
}

// ── Clôture ─────────────────────────────────────────────────────────────────

struct ClosureReport {
    pdf: PathBuf,
    manifest: PathBuf,
    archived: PathBuf,
    exported_to: Option<PathBuf>,
    export_error: Option<String>,
}

/// Travail de clôture, hors du fil GTK. Rend le lot en cas d'échec pour
/// qu'il redevienne actif.
fn close_lot_work(mut lot: Lot) -> Result<ClosureReport, Box<(Option<Lot>, String)>> {
    let result = (|| -> Result<(PathBuf, PathBuf, Option<PathBuf>, Option<String>), String> {
        let dir = lot.certificates_dir();
        let mut signed: Vec<SignedAttestation> = Vec::new();
        for r in lot.certificates() {
            let path = dir.join(&r.file);
            let text =
                fs::read_to_string(&path).map_err(|e| format!("{} : {e}", path.display()))?;
            let envelope: SignedAttestation =
                serde_json::from_str(&text).map_err(|e| format!("{} : {e}", path.display()))?;
            let digest = manifest::envelope_sha256(&envelope).map_err(|e| e.to_string())?;
            if digest != r.sha256 {
                return Err(trf(
                    "{file} : empreinte différente du journal du lot",
                    &[("file", &r.file)],
                ));
            }
            signed.push(envelope);
        }
        if signed.is_empty() {
            return Err(tr("Aucun certificat dans ce lot : clôture impossible."));
        }
        let identity = certificate::signing_identity()?;
        let lot_id = lot.id().to_string();
        let exported = export::export_signed_lot(
            &dir,
            &format!("{lot_id}-lot.pdf"),
            &signed,
            &lot_id,
            pdf_logo::current_jpeg().as_deref(),
            certificate::pdf_lang(i18n::current()),
        )
        .map_err(|e| e.to_string())?;
        let s = lot.state().clone();
        let payload = manifest::LotManifest {
            version: manifest::SCHEMA_VERSION,
            lot_id: lot_id.clone(),
            operator: s.operator,
            reference: s.reference,
            opened_at: s.opened_at,
            closed_at: crate::clock::stamp(),
            software: certificate::software_line(),
            machines: lot.machines_count(),
            certificates: signed
                .iter()
                .map(manifest::entry_for)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?,
            journal_sha256: lot.journal_sha256()?,
        };
        let signed_manifest =
            manifest::sign_manifest(&identity, &payload).map_err(|e| e.to_string())?;
        let manifest_json =
            serde_json::to_string_pretty(&signed_manifest).map_err(|e| e.to_string())?;
        let manifest_path = lot.dir().join(format!("{lot_id}-manifest.json"));
        lot::write_atomic_sync(&manifest_path, manifest_json.as_bytes())?;
        lot.close(&sha256_hex(manifest_json.as_bytes()))?;

        // Livrables : PDF groupé, JSON du lot, et pour chaque certificat du
        // manifeste son attestation et le PDF de sa session. Les entrées
        // remplacées restent dans l'archive seulement.
        let mut deliverables: Vec<PathBuf> = vec![exported.pdf.clone()];
        if let Some(lot_json) = &exported.lot_json {
            deliverables.push(lot_json.clone());
        }
        for envelope in &signed {
            let p = &envelope.payload;
            deliverables
                .push(dir.join(format!("{}.json", export::safe_filename(&p.certificate_id))));
            deliverables.push(dir.join(format!("{}.pdf", export::safe_filename(&p.session_id))));
        }
        // Livrables sur la partition d'export de la clé, seulement quand
        // l'application tourne depuis la clé (non bloquant). Sur un poste
        // installé, une clé SecMet branchée n'est qu'un disque parmi
        // d'autres : les livrables restent dans l'archive du poste.
        let export_mount = if paths::dedicated_data_dir() {
            lot::find_export_mount()
        } else {
            None
        };
        let (exported_to, export_error) = match export_mount {
            None => (None, None),
            Some(mount) => {
                let target = mount.join(&lot_id);
                match copy_deliverables(&dir, &deliverables, &manifest_path, &target) {
                    Ok(()) => (Some(target), None),
                    Err(e) => (None, Some(e)),
                }
            }
        };
        Ok((exported.pdf, manifest_path, exported_to, export_error))
    })();
    match result {
        Ok((pdf, manifest_path, exported_to, export_error)) => {
            let old_dir = lot.dir().to_path_buf();
            match lot.archive(&paths::archive_dir()) {
                Ok(archived) => {
                    let relocate = |p: &PathBuf| match p.strip_prefix(&old_dir) {
                        Ok(rel) => archived.join(rel),
                        Err(_) => p.clone(),
                    };
                    Ok(ClosureReport {
                        pdf: relocate(&pdf),
                        manifest: relocate(&manifest_path),
                        archived,
                        exported_to,
                        export_error,
                    })
                }
                // Clôturé sur disque mais pas archivé : le lot est rechargé tel quel
                // s'il est lisible ; l'erreur est affichée dans tous les cas.
                Err(e) => Err(Box::new((Lot::load(&old_dir).ok(), e))),
            }
        }
        Err(e) => Err(Box::new((Some(lot), e))),
    }
}

fn copy_deliverables(
    certificates: &std::path::Path,
    deliverables: &[PathBuf],
    manifest_path: &std::path::Path,
    target: &std::path::Path,
) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| format!("{} : {e}", target.display()))?;
    let mut sources: Vec<PathBuf> = deliverables
        .iter()
        .filter(|p| p.starts_with(certificates) && p.is_file())
        .cloned()
        .collect();
    sources.push(manifest_path.to_path_buf());
    for src in sources {
        let Some(name) = src.file_name() else {
            continue;
        };
        fs::copy(&src, target.join(name)).map_err(|e| format!("{} : {e}", src.display()))?;
    }
    Ok(())
}

/// Dialogue de clôture ; `after` est appelé une fois le lot clôturé ou la
/// clôture annulée.
pub(super) fn close_dialog(window: &ApplicationWindow, after: Option<Box<dyn FnOnce()>>) {
    let after: Done = Rc::new(RefCell::new(after));
    let Some(summary) = with_active(|lot| {
        let s = lot.state();
        let count = |st: EntryStatus| s.entries.iter().filter(|e| e.status == st).count();
        (
            s.id.clone(),
            units_text(lot),
            lot.certificates().len(),
            count(EntryStatus::Failed),
            count(EntryStatus::Interrupted),
        )
    }) else {
        once(&after);
        return;
    };
    let (id, units, certificates, failed, interrupted) = summary;
    let (dialog, content, buttons) = dialog_frame(window, &tr("Clôturer le lot"));
    content.append(&note(&trf(
        "Lot {id} : {units}, {certificates} certificat(s), {failed} en échec, \
         {interrupted} interrompue(s).\n\nLa clôture produit le certificat groupé et le \
         manifeste signé, copie les livrables sur la partition d'export si elle est présente, \
         puis archive le lot. Plus rien ne pourra y être ajouté.",
        &[
            ("id", &id),
            ("units", &units),
            ("certificates", &certificates.to_string()),
            ("failed", &failed.to_string()),
            ("interrupted", &interrupted.to_string()),
        ],
    )));
    if certificates == 0 {
        let warn = note(&tr("Aucun certificat dans ce lot : clôture impossible."));
        warn.add_css_class("host-notice");
        content.append(&warn);
    }
    let cancel = Button::with_label(&tr("Annuler"));
    let confirm = Button::with_label(&tr("Clôturer"));
    confirm.add_css_class("destructive-action");
    confirm.set_sensitive(certificates > 0);
    buttons.append(&cancel);
    buttons.append(&confirm);
    content.append(&buttons);
    {
        let dialog = dialog.clone();
        let after = after.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
            once(&after);
        });
    }
    {
        let dialog = dialog.clone();
        let window = window.clone();
        let after = after.clone();
        confirm.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            dialog.close();
            // Intégrité des certificats avant de retirer le lot de la session.
            match with_active(|lot| stale_certificates(lot)) {
                None => once(&after),
                Some(Err(e)) => {
                    show_error_dialog(
                        &window,
                        &tr("Lot"),
                        &trf("Clôture impossible : {error}", &[("error", &e)]),
                    );
                    once(&after);
                }
                Some(Ok(stale)) if stale.is_empty() => take_and_close(&window, after.clone()),
                Some(Ok(stale)) => stale_dialog(&window, stale, after.clone()),
            }
        });
    }
    dialog.present();
}

/// Retire le lot de la session et lance la clôture.
fn take_and_close(window: &ApplicationWindow, after: Done) {
    match ACTIVE.with(|a| a.borrow_mut().take()) {
        Some(lot) => run_closure(window, lot, after),
        None => once(&after),
    }
}

/// Certificats régénérés après enregistrement : l'opérateur choisit de
/// les enregistrer à nouveau (journalisé) puis de clôturer, ou renonce.
fn stale_dialog(window: &ApplicationWindow, stale: Vec<StaleCertificate>, after: Done) {
    let (dialog, content, buttons) = dialog_frame(window, &tr("Certificats régénérés"));
    let list: Vec<String> = stale.iter().map(|s| format!("• {}", s.file)).collect();
    content.append(&note(&trf(
        "{count} certificat(s) ont été régénérés après leur enregistrement au lot \
         (bouton « Générer le PDF ») :\n{list}\n\nIls sont signés par la clé de ce poste et \
         désignent les mêmes machines. Les enregistrer à nouveau au journal du lot, puis \
         clôturer ?",
        &[
            ("count", &stale.len().to_string()),
            ("list", &list.join("\n")),
        ],
    )));
    let cancel = Button::with_label(&tr("Annuler"));
    let confirm = Button::with_label(&tr("Enregistrer et clôturer"));
    confirm.add_css_class("destructive-action");
    buttons.append(&cancel);
    buttons.append(&confirm);
    content.append(&buttons);
    {
        let dialog = dialog.clone();
        let after = after.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
            once(&after);
        });
    }
    {
        let dialog = dialog.clone();
        let window = window.clone();
        confirm.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            dialog.close();
            let recorded = with_active(|lot| {
                stale
                    .iter()
                    .try_for_each(|s| lot.update_certificate(s.entry, &s.certificate_id, &s.sha256))
            });
            match recorded {
                Some(Ok(())) => take_and_close(&window, after.clone()),
                Some(Err(e)) => {
                    show_error_dialog(
                        &window,
                        &tr("Lot"),
                        &trf("Clôture impossible : {error}", &[("error", &e)]),
                    );
                    once(&after);
                }
                None => once(&after),
            }
        });
    }
    dialog.present();
}

fn run_closure(window: &ApplicationWindow, lot: Lot, after: Done) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(close_lot_work(lot));
    });
    // Plusieurs secondes (PDF groupé, manifeste, copie sur la clé) : une
    // roue d'attente, sinon l'interface semble figée.
    let busy = busy_dialog(
        window,
        &tr("Clôture du lot"),
        &tr("Clôture en cours : certificat groupé, manifeste signé, copie des livrables…"),
    );
    let window = window.clone();
    glib::timeout_add_local(Duration::from_millis(150), move || {
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                busy.close();
                once(&after);
                return glib::ControlFlow::Break;
            }
        };
        busy.close();
        match result {
            Ok(report) => {
                set_active(None);
                let mut text = trf(
                    "Certificat groupé : {pdf}\nManifeste signé : {manifest}\nArchive : {dir}",
                    &[
                        ("pdf", &report.pdf.display().to_string()),
                        ("manifest", &report.manifest.display().to_string()),
                        ("dir", &report.archived.display().to_string()),
                    ],
                );
                text.push_str("\n\n");
                text.push_str(&match (&report.exported_to, &report.export_error) {
                    (Some(dir), _) => trf(
                        "Copié sur la partition d'export : {dir}",
                        &[("dir", &dir.display().to_string())],
                    ),
                    (None, Some(e)) => trf(
                        "Copie sur la partition d'export impossible : {error}",
                        &[("error", e)],
                    ),
                    (None, None) if paths::dedicated_data_dir() => {
                        tr("Partition d'export absente (SECMET-EXP) : rien n'a été copié.")
                    }
                    (None, None) => tr("Livrables dans l'archive du lot."),
                });
                let folder = report
                    .exported_to
                    .clone()
                    .unwrap_or_else(|| report.archived.clone());
                show_closed_dialog(&window, &text, folder);
            }
            Err(boxed) => {
                let (lot, e) = *boxed;
                set_active(lot);
                show_error_dialog(
                    &window,
                    &tr("Lot"),
                    &trf("Clôture impossible : {error}", &[("error", &e)]),
                );
            }
        }
        once(&after);
        glib::ControlFlow::Break
    });
}

/// Résultat de la clôture, avec un accès direct aux livrables (export si
/// copié, sinon archive).
fn show_closed_dialog(window: &ApplicationWindow, text: &str, folder: PathBuf) {
    let (dialog, content, buttons) = dialog_frame(window, &tr("Lot clôturé"));
    content.append(&note(text));
    let status = Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);
    content.append(&status);
    let open = Button::with_label(&tr("Ouvrir le dossier"));
    let ok = Button::with_label(&tr("Valider"));
    ok.add_css_class("suggested-action");
    buttons.append(&open);
    buttons.append(&ok);
    content.append(&buttons);
    open.connect_clicked(move |_| {
        if let Err(e) = certificate::open_folder(&folder) {
            status.set_text(&e);
        }
    });
    {
        let dialog = dialog.clone();
        ok.connect_clicked(move |_| dialog.close());
    }
    dialog.present();
}
