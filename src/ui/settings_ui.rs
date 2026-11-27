//! Dialogue Réglages : langue de l'interface, logo des certificats PDF,
//! nom de l'opérateur pré-rempli et mises à jour.
//!
//! Le logo est converti à la validation (voir [`crate::pdf_logo`]) ; les
//! générations de PDF suivantes l'utilisent. Un changement de langue
//! reconstruit la fenêtre principale.

use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Button, CheckButton, ComboBoxText, Dialog, Entry,
    FileChooserAction, FileChooserDialog, FileFilter, Label, Orientation, Picture, ResponseType,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use crate::busy::{self, Blocker, BusyKind};
use crate::i18n::{self, count_text, plural_key, tr, trf, Lang};
use crate::update::{self, UpdateState};
use crate::{key_cleanup, pdf_logo, settings};

use super::app::build_ui;
use super::app_theme::apply_color_scheme_class;

/// Lieux du nettoyage de la clé, d'après les chemins courants.
fn cleanup_places() -> key_cleanup::Places {
    key_cleanup::Places {
        archive: crate::paths::archive_dir(),
        logs: crate::paths::logs_dir(),
        export: crate::lot::export_mount_on_key(),
    }
}

/// Taille lisible : kilo-octets sous 1 Mo, sinon méga-octets à une décimale.
fn human_size(bytes: u64) -> String {
    if bytes < 1_048_576 {
        trf("{n} Ko", &[("n", &bytes.div_ceil(1024).to_string())])
    } else {
        trf(
            "{n} Mo",
            &[("n", &format!("{:.1}", bytes as f64 / 1_048_576.0))],
        )
    }
}

/// Libellés des trois groupes avec leurs comptes courants.
fn relabel_groups(
    archived: &CheckButton,
    exports: &CheckButton,
    standalone: &CheckButton,
    inv: &key_cleanup::Inventory,
) {
    archived.set_label(Some(&trf(
        "Lots archivés ({count})",
        &[("count", &inv.archived.len().to_string())],
    )));
    exports.set_label(Some(&trf(
        "Livrables sur la partition d'export ({folders})",
        &[(
            "folders",
            &count_text(inv.exports.len(), "{count} dossier", "{count} dossiers"),
        )],
    )));
    standalone.set_label(Some(&trf(
        "Certificats hors lot, rapports et journaux de session ({count})",
        &[("count", &inv.standalone_files.to_string())],
    )));
}

/// Réglages de la clé : section « Nettoyage de la clé ». Trois groupes à
/// cocher, inventaire affiché avant d'agir, confirmation avec les comptes
/// réels. Refusé tant qu'un lot est ouvert, et refusé pour le groupe des lots
/// archivés tant qu'un lot clôturé n'a pas sa copie sur la partition d'export
/// (blocage strict : l'archive est alors sa seule copie signée).
fn cleanup_section(parent: &ApplicationWindow, content: &GtkBox) {
    let title = Label::new(None);
    title.set_markup(&format!("<b>{}</b>", tr("Nettoyage de la clé")));
    title.set_xalign(0.0);
    title.set_margin_top(6);
    content.append(&title);
    let note = Label::new(Some(&tr(
        "Retire de la clé ce que les lots et les effacements y ont laissé. Les réglages, \
         les mises à jour, le coffre des mots de passe ATA et les journaux de démarrage ne \
         sont jamais touchés ; sur la partition d'export, seuls les dossiers de lot (L-…) et \
         de session (SEC-…) le sont.",
    )));
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.add_css_class("dim-label");
    content.append(&note);

    let inv = key_cleanup::inventory(&cleanup_places());
    let archived = CheckButton::with_label(&trf(
        "Lots archivés ({count})",
        &[("count", &inv.archived.len().to_string())],
    ));
    let exports = CheckButton::with_label(&trf(
        "Livrables sur la partition d'export ({folders})",
        &[(
            "folders",
            &count_text(inv.exports.len(), "{count} dossier", "{count} dossiers"),
        )],
    ));
    let standalone = CheckButton::with_label(&trf(
        "Certificats hors lot, rapports et journaux de session ({count})",
        &[("count", &inv.standalone_files.to_string())],
    ));
    archived.set_active(true);
    exports.set_active(true);
    standalone.set_active(true);
    if crate::lot::export_mount_on_key().is_none() {
        exports.set_active(false);
        exports.set_sensitive(false);
        exports.set_tooltip_text(Some(&tr(
            "Partition d'export absente (SECMET-EXP) : rien n'a été copié.",
        )));
    }
    for c in [&archived, &exports, &standalone] {
        content.append(c);
    }
    let row = GtkBox::new(Orientation::Horizontal, 8);
    let button = Button::with_label(&tr("Nettoyer la clé…"));
    button.add_css_class("destructive-action");
    row.append(&button);
    let status = Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("dim-label");
    row.append(&status);
    content.append(&row);

    let parent = parent.clone();
    button.connect_clicked(move |button| {
        if super::lot_ui::is_active() {
            status.set_text(&tr(
                "Un lot est ouvert : clôturez-le ou abandonnez-le avant de nettoyer la clé.",
            ));
            return;
        }
        let choice = key_cleanup::Choice {
            archived: archived.is_active(),
            exports: exports.is_active(),
            standalone: standalone.is_active(),
        };
        if choice.is_empty() {
            status.set_text(&tr("Cochez au moins un groupe."));
            return;
        }
        let places = cleanup_places();
        let inv = key_cleanup::inventory(&places);
        // Les comptes ont pu changer depuis l'ouverture des Réglages (recopie
        // sur l'export, par exemple) : on les rafraîchit avant de décider.
        relabel_groups(&archived, &exports, &standalone, &inv);
        let archived_n = if choice.archived {
            inv.archived.len()
        } else {
            0
        };
        let exports_n = if choice.exports { inv.exports.len() } else { 0 };
        let standalone_n = if choice.standalone {
            inv.standalone_files
        } else {
            0
        };
        let message = trf(
            "Supprimer définitivement de la clé : {archived}, {exports}, {standalone}, soit \
             {size} ?",
            &[
                (
                    "archived",
                    &count_text(archived_n, "{count} lot archivé", "{count} lots archivés"),
                ),
                (
                    "exports",
                    &count_text(
                        exports_n,
                        "{count} dossier sur la partition d'export",
                        "{count} dossiers sur la partition d'export",
                    ),
                ),
                (
                    "standalone",
                    &count_text(
                        standalone_n,
                        "{count} fichier hors lot",
                        "{count} fichiers hors lot",
                    ),
                ),
                ("size", &human_size(inv.bytes)),
            ],
        );
        // Blocage strict (décision du 2026-11-06) : l'archive d'un lot clôturé
        // est sa seule copie signée tant que son manifeste manque sur l'export.
        if choice.archived && !inv.not_exported.is_empty() {
            status.set_text(&trf(
                plural_key(
                    inv.not_exported.len(),
                    "Refusé : {count} lot clôturé n'a pas sa copie sur la partition \
                     d'export ({list}). Son archive sur la clé est la seule copie signée : \
                     utilisez d'abord « Recopier les lots clôturés sur la partition d'export », \
                     ou décochez les lots archivés.",
                    "Refusé : {count} lots clôturés n'ont pas leur copie sur la partition \
                     d'export ({list}). Leur archive sur la clé est la seule copie signée : \
                     utilisez d'abord « Recopier les lots clôturés sur la partition d'export », \
                     ou décochez les lots archivés.",
                ),
                &[
                    ("count", &inv.not_exported.len().to_string()),
                    ("list", &inv.not_exported.join(", ")),
                ],
            ));
            return;
        }
        let button = button.clone();
        let button_declined = button.clone();
        let status = status.clone();
        let archived = archived.clone();
        let exports = exports.clone();
        let standalone = standalone.clone();
        button.set_sensitive(false);
        super::dialogs::confirm_action_dual(
            &parent,
            &tr("Nettoyage de la clé"),
            &message,
            move || {
                let report = key_cleanup::clean(&places, choice);
                let mut text = trf(
                    "{removed}, {size} libérés.",
                    &[
                        (
                            "removed",
                            &count_text(
                                report.removed,
                                "{count} élément supprimé",
                                "{count} éléments supprimés",
                            ),
                        ),
                        ("size", &human_size(report.bytes)),
                    ],
                );
                for (path, e) in &report.errors {
                    text.push_str(&format!("\n{path} : {e}"));
                }
                status.set_text(&text);
                relabel_groups(
                    &archived,
                    &exports,
                    &standalone,
                    &key_cleanup::inventory(&cleanup_places()),
                );
                button.set_sensitive(true);
            },
            move || button_declined.set_sensitive(true),
        );
    });
}

/// Message sous le choix du dossier : discret, ou en rouge pour un refus.
fn set_folder_status(label: &Label, text: &str, error: bool) {
    label.set_text(text);
    if error {
        label.remove_css_class("dim-label");
        label.add_css_class("status-error");
    } else {
        label.remove_css_class("status-error");
        label.add_css_class("dim-label");
    }
}

/// Dossier de sortie choisi, enregistré au clic sur Valider.
#[derive(Default)]
enum PendingFolder {
    /// Rien ne change.
    #[default]
    Keep,
    /// Dossier choisi et contrôlé.
    Custom(crate::output::Choice),
    /// Retour à `~/Documents/SecMet-Certificats`.
    Default,
}

/// Choix de logo en attente de validation.
#[derive(Default)]
enum PendingLogo {
    /// Rien ne change.
    #[default]
    Keep,
    /// Image source choisie, déjà convertie pour l'aperçu.
    Custom(PathBuf),
    /// Retour au logo SecMet.
    Default,
}

fn picture_from_jpeg(bytes: &[u8]) -> Option<Picture> {
    let loader = gtk4::gdk_pixbuf::PixbufLoader::new();
    loader.write(bytes).ok()?;
    loader.close().ok()?;
    let pixbuf = loader.pixbuf()?;
    let texture = gtk4::gdk::Texture::for_pixbuf(&pixbuf);
    let picture = Picture::for_paintable(&texture);
    picture.set_can_shrink(true);
    picture.set_keep_aspect_ratio(true);
    picture.set_size_request(320, 90);
    Some(picture)
}

fn set_preview(holder: &GtkBox, bytes: &[u8]) {
    while let Some(child) = holder.first_child() {
        holder.remove(&child);
    }
    match picture_from_jpeg(bytes) {
        Some(picture) => holder.append(&picture),
        None => holder.append(&Label::new(Some(&tr("Aperçu indisponible")))),
    }
}

/// Texte (markup) de la section Mises à jour, page de téléchargement à
/// proposer et notes de version, d'après l'état enregistré.
fn describe_update(state: &UpdateState) -> (String, Option<String>, Option<String>) {
    let esc = glib::markup_escape_text;
    let mut lines = vec![esc(&trf(
        "Version installée : {version}",
        &[("version", update::INSTALLED_VERSION)],
    ))
    .to_string()];
    let mut page = None;
    let mut notes = None;
    if let Some(latest) = state.available() {
        lines.push(format!(
            "<b>{}</b>",
            esc(&trf(
                "Nouvelle version disponible : {version} ({date})",
                &[("version", &latest.version), ("date", &latest.published_at)],
            ))
        ));
        page = Some(update::release_page_url(latest));
        notes = update::notes_for(latest, i18n::current().code()).map(str::to_string);
    } else if state.latest.is_some() && state.error.is_none() {
        lines.push(esc(&tr("Vous utilisez la dernière version.")).to_string());
    }
    if let Some(error) = &state.error {
        lines.push(
            esc(&trf(
                "Vérification impossible : {error}",
                &[("error", &tr(error))],
            ))
            .to_string(),
        );
    }
    lines.push(
        esc(&match state.checked_at {
            Some(at) => trf(
                "Dernière vérification : {date}",
                &[(
                    "date",
                    &at.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M")
                        .to_string(),
                )],
            ),
            None => tr("Jamais vérifié"),
        })
        .to_string(),
    );
    (lines.join("\n"), page, notes)
}

/// Opérations en cours, effacement compris même si sa garde manquait
/// (`operations_active` de la fenêtre principale fait foi aussi).
fn busy_now(operations_active: &Cell<bool>) -> Vec<BusyKind> {
    let mut kinds = busy::current();
    if operations_active.get() && !kinds.contains(&BusyKind::Erase) {
        kinds.insert(0, BusyKind::Erase);
    }
    kinds
}

/// Blocage courant de l'installation d'une mise à jour (D14) : opérations
/// en cours, lot ouvert, installation en cours, relance attendue.
fn install_blocker_now(operations_active: &Cell<bool>) -> Option<Blocker> {
    busy::install_blocker(
        &busy_now(operations_active),
        super::lot_ui::active_id().as_deref(),
        busy::installing(),
        busy::restart_pending(),
    )
}

/// Raison affichée sous le bouton d'installation masqué ; `None` quand
/// l'état parle de lui-même (installation en cours, relance attendue).
fn blocker_text(blocker: &Blocker) -> Option<String> {
    let busy = |what: &str| {
        Some(trf(
            "Installation impossible pour l'instant : {what}.",
            &[("what", &tr(what))],
        ))
    };
    match blocker {
        Blocker::Installing | Blocker::RestartPending => None,
        Blocker::Busy(BusyKind::Erase) => busy("effacement en cours"),
        Blocker::Busy(BusyKind::Smart) => busy("analyse des disques en cours"),
        Blocker::Busy(BusyKind::TestDisk) => busy("TestDisk en cours"),
        Blocker::Busy(BusyKind::Inventory) => busy("chargement de la liste des disques en cours"),
        Blocker::Busy(BusyKind::Pdf) => busy("génération d'un certificat PDF en cours"),
        Blocker::Busy(BusyKind::LotClose) => busy("clôture du lot en cours"),
        Blocker::Busy(BusyKind::Unlock) => busy("déverrouillage d'un disque en cours"),
        Blocker::Busy(BusyKind::Defrozen) => busy("sortie de l'état gelé d'un disque en cours"),
        Blocker::OpenLot(id) => Some(trf(
            "Lot {id} ouvert : clôturez-le avant d'installer la mise à jour (un lot n'est \
             traité que par une seule version de SecMet).",
            &[("id", id)],
        )),
    }
}

/// Section « Clé de signature des certificats » : `key_id` et clé publique
/// du poste (ou de la clé USB), à inscrire dans la liste de confiance du
/// client. Lecture seule : rien n'est créé si la clé n'existe pas encore.
fn signing_key_section(content: &GtkBox) {
    let title = Label::new(None);
    title.set_markup(&format!(
        "<b>{}</b>",
        tr("Clé de signature des certificats")
    ));
    title.set_xalign(0.0);
    title.set_margin_top(6);
    content.append(&title);
    let note = Label::new(Some(&tr(
        "Chaque certificat est signé par cette clé, propre à ce poste (ou à cette clé USB). \
         Le client inscrit la clé publique ci-dessous dans sa liste de confiance \
         (trusted-keys) pour vérifier les certificats, et l'en retire si le support est \
         perdu. Elle est aussi copiée avec les livrables de chaque lot clôturé.",
    )));
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.add_css_class("dim-label");
    content.append(&note);
    let pub_path = secmet_attest::keystore::public_key_path(&crate::paths::config_file(
        secmet_attest::keystore::KEY_FILE,
    ));
    match crate::lot::read_public_key(&pub_path) {
        Ok(Some(key)) => {
            let id = Label::new(Some(&trf(
                "Identifiant (key_id) : {key_id}",
                &[("key_id", &key.key_id)],
            )));
            id.set_xalign(0.0);
            id.set_selectable(true);
            content.append(&id);
            let row = GtkBox::new(Orientation::Horizontal, 8);
            let entry = Entry::new();
            entry.set_text(&key.public_key);
            entry.set_editable(false);
            entry.set_hexpand(true);
            entry.set_tooltip_text(Some(&tr("Clé publique Ed25519 (hexadécimal)")));
            row.append(&entry);
            let copy = Button::with_label(&tr("Copier"));
            row.append(&copy);
            content.append(&row);
            let status = Label::new(None);
            status.set_xalign(0.0);
            status.add_css_class("dim-label");
            content.append(&status);
            let text = key.public_key.clone();
            copy.connect_clicked(move |btn| {
                btn.clipboard().set_text(&text);
                status.set_text(&tr("Clé publique copiée dans le presse-papiers."));
            });
        }
        Ok(None) => {
            let missing = Label::new(Some(&trf(
                "Aucune clé de signature pour l'instant ({file} absent) : elle est créée à la \
                 première signature d'un certificat.",
                &[("file", &pub_path.display().to_string())],
            )));
            missing.set_wrap(true);
            missing.set_xalign(0.0);
            content.append(&missing);
        }
        Err(e) => {
            let error = Label::new(Some(&e));
            error.set_wrap(true);
            error.set_xalign(0.0);
            content.append(&error);
        }
    }
}

pub(super) fn show_settings_dialog(parent: &ApplicationWindow, operations_active: Rc<Cell<bool>>) {
    let current = settings::load();
    // Changer de langue reconstruit la fenêtre principale : impossible tant
    // qu'une opération y est suivie (relu en continu, et au clic).
    let lang_locked = {
        let operations_active = operations_active.clone();
        move || !busy_now(&operations_active).is_empty() || busy::installing()
    };
    let dialog = Dialog::builder()
        .title(tr("Réglages"))
        .modal(true)
        .transient_for(parent)
        .build();
    apply_color_scheme_class(&dialog);

    let content = GtkBox::new(Orientation::Vertical, 14);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    content.set_margin_start(18);
    content.set_margin_end(18);

    // ── Langue ──
    let lang_title = Label::new(None);
    lang_title.set_markup(&format!("<b>{}</b>", tr("Langue de l'interface")));
    lang_title.set_xalign(0.0);
    content.append(&lang_title);
    let lang_combo = ComboBoxText::new();
    for lang in Lang::ALL {
        lang_combo.append(Some(lang.code()), lang.label());
    }
    lang_combo.set_active_id(Some(Lang::from_code(&current.language).code()));
    // Changer de langue reconstruit la fenêtre : impossible pendant une
    // opération, dont le suivi vit dans la fenêtre courante.
    let apply_lang_lock = {
        let lang_combo = lang_combo.clone();
        let lang_locked = lang_locked.clone();
        move || {
            let locked = lang_locked();
            lang_combo.set_sensitive(!locked);
            lang_combo.set_tooltip_text(
                locked
                    .then(|| tr("Langue verrouillée pendant une opération en cours"))
                    .as_deref(),
            );
        }
    };
    apply_lang_lock();
    content.append(&lang_combo);
    let lang_note = Label::new(Some(&tr(
        "Le changement de langue s'applique immédiatement à la fenêtre principale \
         et aux certificats PDF générés ensuite.",
    )));
    lang_note.set_wrap(true);
    lang_note.set_xalign(0.0);
    lang_note.add_css_class("dim-label");
    content.append(&lang_note);

    // ── Logo PDF ──
    let logo_title = Label::new(None);
    logo_title.set_markup(&format!("<b>{}</b>", tr("Logo des certificats PDF")));
    logo_title.set_xalign(0.0);
    logo_title.set_margin_top(6);
    content.append(&logo_title);
    let logo_note = Label::new(Some(&tr(
        "Image PNG, JPEG, GIF, BMP, WebP ou TIFF. Elle est convertie en JPEG sur fond blanc \
         et placée en haut à gauche de chaque page à la place du logo SecMet.",
    )));
    logo_note.set_wrap(true);
    logo_note.set_xalign(0.0);
    logo_note.add_css_class("dim-label");
    content.append(&logo_note);

    let preview = GtkBox::new(Orientation::Horizontal, 0);
    preview.set_halign(gtk4::Align::Start);
    set_preview(
        &preview,
        &pdf_logo::current_jpeg().unwrap_or_else(|| secmet_attest::pdf::LOGO_JPEG.to_vec()),
    );
    content.append(&preview);

    let logo_status = Label::new(None);
    logo_status.set_xalign(0.0);
    logo_status.set_wrap(true);
    logo_status.set_text(&if current.pdf_logo.is_some() {
        tr("Logo actuel : personnalisé")
    } else {
        tr("Logo actuel : SecMet")
    });
    content.append(&logo_status);

    let logo_buttons = GtkBox::new(Orientation::Horizontal, 8);
    let choose_btn = Button::with_label(&tr("Choisir une image…"));
    let default_btn = Button::with_label(&tr("Logo SecMet"));
    logo_buttons.append(&choose_btn);
    logo_buttons.append(&default_btn);
    content.append(&logo_buttons);

    // ── Opérateur ──
    let operator_title = Label::new(None);
    operator_title.set_markup(&format!("<b>{}</b>", tr("Opérateur")));
    operator_title.set_xalign(0.0);
    operator_title.set_margin_top(6);
    content.append(&operator_title);
    let operator_note = Label::new(Some(&tr(
        "Nom pré-rempli dans le dialogue de génération des certificats PDF \
         (modifiable à chaque génération). Laisser vide pour le saisir à chaque fois.",
    )));
    operator_note.set_wrap(true);
    operator_note.set_xalign(0.0);
    operator_note.add_css_class("dim-label");
    content.append(&operator_note);
    let operator_entry = Entry::new();
    operator_entry.set_placeholder_text(Some(&tr("Votre nom")));
    operator_entry.set_text(&current.operator_name);
    operator_entry.set_hexpand(true);
    content.append(&operator_entry);

    // ── Dossier des certificats ──
    let folder_title = Label::new(None);
    folder_title.set_markup(&format!("<b>{}</b>", tr("Dossier des certificats")));
    folder_title.set_xalign(0.0);
    folder_title.set_margin_top(6);
    content.append(&folder_title);
    let folder_note = Label::new(Some(&if crate::output::applies() {
        tr(
            "Les certificats PDF et leur version signée (.json) sont enregistrés dans ce \
            dossier. Un lot clôturé y est copié dans un sous-dossier à son nom ; pendant le \
            lot, ses certificats restent dans le dossier du lot.",
        )
    } else {
        tr(
            "Certificats PDF et attestations signées déjà produits : lot en cours, ou lots \
            clôturés et certificats du poste.",
        )
    }));
    folder_note.set_wrap(true);
    folder_note.set_xalign(0.0);
    folder_note.add_css_class("dim-label");
    content.append(&folder_note);
    // Poste : dossier de sortie au choix (pas sur la clé : SECMET-EXP).
    let pending_folder: Rc<RefCell<PendingFolder>> = Rc::new(RefCell::new(PendingFolder::Keep));
    if crate::output::applies() {
        let output_path = Label::new(Some(&crate::output::display_dir().display().to_string()));
        output_path.set_xalign(0.0);
        output_path.set_wrap(true);
        output_path.set_selectable(true);
        content.append(&output_path);
        let output_row = GtkBox::new(Orientation::Horizontal, 8);
        let choose_folder_btn = Button::with_label(&tr("Choisir un dossier…"));
        let default_folder_btn = Button::with_label(&tr("Dossier par défaut"));
        output_row.append(&choose_folder_btn);
        output_row.append(&default_folder_btn);
        content.append(&output_row);
        let output_status = Label::new(None);
        output_status.set_xalign(0.0);
        output_status.set_wrap(true);
        output_status.add_css_class("dim-label");
        content.append(&output_status);
        {
            let dialog = dialog.clone();
            let output_path = output_path.clone();
            let output_status = output_status.clone();
            let pending = pending_folder.clone();
            choose_folder_btn.connect_clicked(move |_| {
                let chooser = FileChooserDialog::new(
                    Some(&tr("Dossier des certificats")),
                    Some(&dialog),
                    FileChooserAction::SelectFolder,
                    &[
                        (&tr("Annuler"), ResponseType::Cancel),
                        (&tr("Choisir"), ResponseType::Accept),
                    ],
                );
                let output_path = output_path.clone();
                let output_status = output_status.clone();
                let pending = pending.clone();
                chooser.connect_response(move |chooser, response| {
                    if response == ResponseType::Accept {
                        if let Some(path) = chooser.file().and_then(|f| f.path()) {
                            match crate::output::validate_choice(&path) {
                                Ok(choice) => {
                                    output_path.set_text(&choice.dir.display().to_string());
                                    let mut text = tr("Cliquez Valider pour l'enregistrer.");
                                    if let Some(w) = &choice.warning {
                                        text = format!("{w}\n{text}");
                                    }
                                    set_folder_status(&output_status, &text, false);
                                    *pending.borrow_mut() = PendingFolder::Custom(choice);
                                }
                                Err(e) => set_folder_status(
                                    &output_status,
                                    &trf("Dossier refusé : {error}", &[("error", &e)]),
                                    true,
                                ),
                            }
                        }
                    }
                    chooser.close();
                });
                chooser.show();
            });
        }
        {
            let output_path = output_path.clone();
            let output_status = output_status.clone();
            let pending = pending_folder.clone();
            default_folder_btn.connect_clicked(move |_| {
                output_path.set_text(&crate::paths::certificates_dir().display().to_string());
                set_folder_status(
                    &output_status,
                    &tr("Cliquez Valider pour l'enregistrer."),
                    false,
                );
                *pending.borrow_mut() = PendingFolder::Default;
            });
        }
    }
    let folder_row = GtkBox::new(Orientation::Horizontal, 8);
    let folder_btn = Button::with_label(&tr("Ouvrir le dossier des certificats"));
    folder_row.append(&folder_btn);
    let folder_status = Label::new(None);
    folder_status.set_xalign(0.0);
    folder_status.set_wrap(true);
    folder_status.add_css_class("dim-label");
    folder_row.append(&folder_status);
    content.append(&folder_row);
    folder_btn.connect_clicked(move |_| {
        let folder = super::lot_ui::certificates_folder();
        match crate::certificate::open_folder(&folder) {
            Ok(()) => folder_status.set_text(&folder.display().to_string()),
            Err(e) => folder_status.set_text(&e),
        }
    });
    // Sur la clé : recopie des lots clôturés absents de la partition
    // d'export (partition absente ou pleine à la clôture, clé réécrite…).
    if crate::paths::dedicated_data_dir() {
        let export_note = Label::new(Some(&tr(
            "Les lots clôturés sont copiés sur la partition d'export (SECMET-EXP) à la \
             clôture ; ceux qui y manquent peuvent être recopiés depuis l'archive de la clé.",
        )));
        export_note.set_wrap(true);
        export_note.set_xalign(0.0);
        export_note.add_css_class("dim-label");
        content.append(&export_note);
        let export_row = GtkBox::new(Orientation::Horizontal, 8);
        let export_btn =
            Button::with_label(&tr("Recopier les lots clôturés sur la partition d'export"));
        export_row.append(&export_btn);
        let export_status = Label::new(None);
        export_status.set_xalign(0.0);
        export_status.set_wrap(true);
        export_status.add_css_class("dim-label");
        export_row.append(&export_status);
        content.append(&export_row);
        export_btn.connect_clicked(move |_| {
            export_status.set_text(&super::lot_ui::export_archived_text(None));
        });
        cleanup_section(parent, &content);
    }

    // ── Clé de signature des certificats (D11) ──
    signing_key_section(&content);

    // ── Lots (poste installé ; sur la clé live les lots sont toujours actifs) ──
    let lots_check = CheckButton::with_label(&tr("Mode lot : regrouper les effacements en lots"));
    lots_check.set_active(current.lots_enabled);
    if !crate::paths::dedicated_data_dir() {
        let lots_title = Label::new(None);
        lots_title.set_markup(&format!("<b>{}</b>", tr("Lots")));
        lots_title.set_xalign(0.0);
        lots_title.set_margin_top(6);
        content.append(&lots_title);
        content.append(&lots_check);
        let lots_note = Label::new(Some(&tr(
            "Au lancement, SecMet propose d'ouvrir ou de continuer un lot. Les disques effacés \
             au fil des sessions (brancher, effacer, débrancher, recommencer) y sont enregistrés \
             avec leur certificat ; la clôture produit le certificat groupé et le manifeste \
             signé.",
        )));
        lots_note.set_wrap(true);
        lots_note.set_xalign(0.0);
        lots_note.add_css_class("dim-label");
        content.append(&lots_note);
    }

    // ── Mises à jour ──
    let update_title = Label::new(None);
    update_title.set_markup(&format!("<b>{}</b>", tr("Mises à jour")));
    update_title.set_xalign(0.0);
    update_title.set_margin_top(6);
    content.append(&update_title);
    let update_check = CheckButton::with_label(&tr("Vérifier automatiquement les mises à jour"));
    update_check.set_active(current.check_updates);
    content.append(&update_check);
    let update_note = Label::new(Some(&tr(
        "Une requête HTTPS par jour vers le canal de publication, sans donnée personnelle.",
    )));
    update_note.set_wrap(true);
    update_note.set_xalign(0.0);
    update_note.add_css_class("dim-label");
    content.append(&update_note);
    let update_status = Label::new(None);
    update_status.set_xalign(0.0);
    update_status.set_wrap(true);
    content.append(&update_status);
    let update_notes = Label::new(None);
    update_notes.set_xalign(0.0);
    update_notes.set_wrap(true);
    update_notes.add_css_class("dim-label");
    update_notes.set_visible(false);
    content.append(&update_notes);
    let update_buttons = GtkBox::new(Orientation::Horizontal, 8);
    let check_now_btn = Button::with_label(&tr("Vérifier maintenant"));
    let install_btn = Button::with_label(&tr("Installer la mise à jour"));
    install_btn.add_css_class("suggested-action");
    install_btn.set_visible(false);
    let relaunch_btn = Button::with_label(&tr("Relancer SecMet"));
    relaunch_btn.set_visible(false);
    let open_page_btn = Button::with_label(&tr("Ouvrir la page de téléchargement"));
    open_page_btn.set_visible(false);
    update_buttons.append(&check_now_btn);
    update_buttons.append(&install_btn);
    update_buttons.append(&relaunch_btn);
    update_buttons.append(&open_page_btn);
    content.append(&update_buttons);
    // Raison du blocage, sous le bouton masqué, et clôture du lot ouvert
    // depuis les Réglages (D14).
    let install_reason = Label::new(None);
    install_reason.set_xalign(0.0);
    install_reason.set_wrap(true);
    install_reason.add_css_class("dim-label");
    install_reason.set_visible(false);
    content.append(&install_reason);
    let close_lot_btn = Button::with_label(&tr("Clôturer le lot…"));
    close_lot_btn.set_halign(gtk4::Align::Start);
    close_lot_btn.set_visible(false);
    content.append(&close_lot_btn);
    // AppImage (D15) : pas d'installation par le gestionnaire de paquets.
    let appimage = crate::update_install::running_from_appimage();
    let appimage_note = Label::new(Some(&tr(
        "AppImage : téléchargez la nouvelle AppImage et remplacez le fichier ; le composant \
         système sera mis à jour au lancement suivant.",
    )));
    appimage_note.set_xalign(0.0);
    appimage_note.set_wrap(true);
    appimage_note.add_css_class("dim-label");
    appimage_note.set_visible(false);
    content.append(&appimage_note);
    // Installation possible : paquet connu pour ce poste, version plus
    // récente, et rien qui la bloque (relu en continu, voir plus bas).
    let package_kind = crate::update_install::package_kind();
    let last_state: Rc<RefCell<UpdateState>> = Rc::new(RefCell::new(update::load_state()));
    let refresh_install = {
        let install_btn = install_btn.clone();
        let relaunch_btn = relaunch_btn.clone();
        let reason = install_reason.clone();
        let close_lot_btn = close_lot_btn.clone();
        let appimage_note = appimage_note.clone();
        let last_state = last_state.clone();
        let operations_active = operations_active.clone();
        Rc::new(move || {
            let available = last_state.borrow().available().is_some();
            let installable = package_kind.is_some() && available;
            let blocker = install_blocker_now(&operations_active);
            // Le bouton garde sa place pendant l'installation (insensible).
            install_btn.set_visible(
                installable && (blocker.is_none() || blocker == Some(Blocker::Installing)),
            );
            let text = blocker
                .as_ref()
                .filter(|_| installable)
                .and_then(blocker_text);
            reason.set_text(text.as_deref().unwrap_or_default());
            reason.set_visible(text.is_some());
            close_lot_btn.set_visible(installable && matches!(blocker, Some(Blocker::OpenLot(_))));
            relaunch_btn.set_visible(busy::restart_pending());
            appimage_note.set_visible(appimage && available);
        })
    };
    let render_update = {
        let status = update_status.clone();
        let notes = update_notes.clone();
        let open_btn = open_page_btn.clone();
        let last_state = last_state.clone();
        let refresh_install = refresh_install.clone();
        Rc::new(move |state: &UpdateState| {
            let (markup, page, note) = describe_update(state);
            status.set_markup(&markup);
            notes.set_text(note.as_deref().unwrap_or_default());
            notes.set_visible(note.is_some());
            open_btn.set_visible(page.is_some());
            open_btn.set_tooltip_text(page.as_deref());
            *last_state.borrow_mut() = state.clone();
            refresh_install();
        })
    };
    render_update(&update::load_state());
    {
        let parent = parent.clone();
        let refresh_install = refresh_install.clone();
        close_lot_btn.connect_clicked(move |_| {
            let refresh_install = refresh_install.clone();
            super::lot_ui::close_dialog(&parent, Some(Box::new(move || refresh_install())));
        });
    }
    {
        // Téléchargement, vérification (SHA256SUMS signé) puis installation
        // sous pkexec, hors du fil GTK ; l'état est affiché au fil de l'eau.
        let status = update_status.clone();
        let check_btn = check_now_btn.clone();
        let operations_active = operations_active.clone();
        let refresh_install = refresh_install.clone();
        install_btn.connect_clicked(move |btn| {
            let (Some(kind), Some(latest)) =
                (package_kind, update::load_state().available().cloned())
            else {
                return;
            };
            // Revérifié au clic : une opération a pu démarrer, ou un lot
            // s'ouvrir, depuis le dernier affichage.
            if install_blocker_now(&operations_active).is_some() {
                refresh_install();
                return;
            }
            let install_guard = busy::begin_install();
            btn.set_sensitive(false);
            check_btn.set_sensitive(false);
            status.set_text(&trf(
                "Téléchargement de la version {version}…",
                &[("version", &latest.version)],
            ));
            let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
            let version = latest.version.clone();
            std::thread::spawn(move || {
                let outcome = crate::update_install::download_and_verify(&latest, kind)
                    .and_then(|path| crate::update_install::install(kind, &path));
                let _ = tx.send(outcome);
            });
            let btn = btn.clone();
            let status = status.clone();
            let check_btn = check_btn.clone();
            let refresh_install = refresh_install.clone();
            let mut install_guard = Some(install_guard);
            glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                let outcome = match rx.try_recv() {
                    Ok(o) => o,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        return glib::ControlFlow::Continue
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        Err("installation interrompue".to_string())
                    }
                };
                install_guard.take();
                check_btn.set_sensitive(true);
                btn.set_sensitive(true);
                match outcome {
                    Ok(_) => {
                        // L'ancien binaire tourne encore : plus d'installation
                        // ni d'ouverture de lot avant la relance.
                        busy::set_restart_pending();
                        status.set_text(&trf(
                            "Version {version} installée. Relancez SecMet pour l'utiliser.",
                            &[("version", &version)],
                        ));
                    }
                    Err(e) => {
                        status.set_text(&trf("Mise à jour impossible : {error}", &[("error", &e)]));
                    }
                }
                refresh_install();
                glib::ControlFlow::Break
            });
        });
    }
    {
        let parent = parent.clone();
        relaunch_btn.connect_clicked(move |_| {
            // Application à instance unique : la nouvelle instance doit
            // démarrer après la fin de celle-ci, sinon elle lui passe la
            // main et sort. Lancement différé, détaché, puis fermeture.
            // Le binaire vient d'être remplacé sur disque : /proc/self/exe
            // pointe sur « /usr/bin/secmet (deleted) ». On relance le
            // fichier installé, sinon « secmet » du PATH.
            let exe = std::env::current_exe()
                .ok()
                .map(|p| {
                    p.to_string_lossy()
                        .trim_end_matches(" (deleted)")
                        .to_string()
                })
                .filter(|p| std::path::Path::new(p).is_file())
                .unwrap_or_else(|| "secmet".to_string());
            let _ = std::process::Command::new("sh")
                .arg("-c")
                .arg("sleep 2; exec \"$0\"")
                .arg(exe)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            if let Some(app) = parent.application() {
                app.quit();
            } else {
                std::process::exit(0);
            }
        });
    }
    {
        let render = render_update.clone();
        let status = update_status.clone();
        check_now_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            status.set_text(&tr("Vérification en cours…"));
            let btn = btn.clone();
            let render = render.clone();
            super::update_ui::run_check(move |state| {
                render(&state);
                btn.set_sensitive(true);
            });
        });
    }
    {
        let status = update_status.clone();
        open_page_btn.connect_clicked(move |btn| {
            let Some(url) = btn.tooltip_text() else {
                return;
            };
            if let Err(e) = crate::host_env::launch_default_for_uri(&url) {
                status.set_text(&trf(
                    "Impossible d'ouvrir le navigateur : {error}",
                    &[("error", &e.to_string())],
                ));
            }
        });
    }

    let pending: Rc<RefCell<PendingLogo>> = Rc::new(RefCell::new(PendingLogo::Keep));

    {
        let dialog = dialog.clone();
        let preview = preview.clone();
        let status = logo_status.clone();
        let pending = pending.clone();
        choose_btn.connect_clicked(move |_| {
            let chooser = FileChooserDialog::new(
                Some(&tr("Choisir le logo")),
                Some(&dialog),
                FileChooserAction::Open,
                &[
                    (&tr("Annuler"), ResponseType::Cancel),
                    (&tr("Ouvrir"), ResponseType::Accept),
                ],
            );
            let filter = FileFilter::new();
            filter.set_name(Some(&tr("Images")));
            for pattern in ["*.png", "*.jpg", "*.jpeg", "*.gif", "*.bmp", "*.webp", "*.tif", "*.tiff"]
            {
                filter.add_pattern(pattern);
            }
            chooser.add_filter(&filter);
            let preview = preview.clone();
            let status = status.clone();
            let pending = pending.clone();
            chooser.connect_response(move |chooser, response| {
                if response == ResponseType::Accept {
                    if let Some(path) = chooser.file().and_then(|f| f.path()) {
                        match pdf_logo::convert(&path) {
                            Ok(bytes) => {
                                set_preview(&preview, &bytes);
                                let (w, h) = pdf_logo::dimensions(&bytes).unwrap_or((0, 0));
                                status.set_text(&i18n::trf(
                                    "Logo choisi : {file} ({w}×{h} px) — cliquez Valider pour l'enregistrer",
                                    &[
                                        (
                                            "file",
                                            &path
                                                .file_name()
                                                .map(|n| n.to_string_lossy().to_string())
                                                .unwrap_or_default(),
                                        ),
                                        ("w", &w.to_string()),
                                        ("h", &h.to_string()),
                                    ],
                                ));
                                *pending.borrow_mut() = PendingLogo::Custom(path);
                            }
                            Err(e) => {
                                status.set_text(&i18n::trf(
                                    "Image refusée : {error}",
                                    &[("error", &e)],
                                ));
                            }
                        }
                    }
                }
                chooser.close();
            });
            chooser.show();
        });
    }
    {
        let preview = preview.clone();
        let status = logo_status.clone();
        let pending = pending.clone();
        default_btn.connect_clicked(move |_| {
            set_preview(&preview, secmet_attest::pdf::LOGO_JPEG);
            status.set_text(&tr("Logo SecMet — cliquez Valider pour l'enregistrer"));
            *pending.borrow_mut() = PendingLogo::Default;
        });
    }

    // ── Boutons ──
    let button_box = GtkBox::new(Orientation::Horizontal, 8);
    button_box.set_halign(gtk4::Align::End);
    button_box.set_margin_top(10);
    let cancel_btn = Button::with_label(&tr("Annuler"));
    let apply_btn = Button::with_label(&tr("Valider"));
    apply_btn.add_css_class("suggested-action");
    button_box.append(&cancel_btn);
    button_box.append(&apply_btn);
    content.append(&button_box);

    // Le contenu défile si l'écran est bas (portable 1366×768 avec barre) :
    // les boutons restent visibles, la hauteur suit celle de l'écran.
    let scroller = gtk4::ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&content));
    let outer = GtkBox::new(Orientation::Vertical, 0);
    outer.append(&scroller);
    button_box.set_margin_end(18);
    button_box.set_margin_bottom(14);
    content.remove(&button_box);
    outer.append(&button_box);
    dialog.set_child(Some(&outer));
    let screen_height = parent
        .surface()
        .and_then(|s| gtk4::gdk::Display::default().and_then(|d| d.monitor_at_surface(&s)))
        .map(|m| m.geometry().height())
        .unwrap_or(900);
    dialog.set_default_size(560, 720.min(screen_height - 120).max(420));

    {
        let dialog = dialog.clone();
        cancel_btn.connect_clicked(move |_| dialog.close());
    }
    {
        let dialog = dialog.clone();
        let parent = parent.clone();
        let status = logo_status.clone();
        let lang_combo = lang_combo.clone();
        let lang_locked = lang_locked.clone();
        let operations_active = operations_active.clone();
        apply_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            let logo_outcome = match std::mem::take(&mut *pending.borrow_mut()) {
                PendingLogo::Keep => Ok(()),
                PendingLogo::Custom(path) => pdf_logo::install(&path).map(|_| ()),
                PendingLogo::Default => {
                    pdf_logo::reset();
                    Ok(())
                }
            };
            if let Err(e) = logo_outcome {
                status.set_text(&i18n::trf(
                    "Logo non enregistré : {error}",
                    &[("error", &e)],
                ));
                btn.set_sensitive(true);
                return;
            }
            let chosen = lang_combo
                .active_id()
                .map(|id| Lang::from_code(&id))
                .unwrap_or_default();
            let mut saved = settings::load();
            let lots_just_enabled = lots_check.is_active() && !saved.lots_enabled;
            // Revérifié au clic : une opération a pu démarrer depuis.
            let locked = lang_locked();
            let language_changed = !locked && Lang::from_code(&saved.language) != chosen;
            if !locked {
                saved.language = chosen.code().to_string();
            }
            saved.operator_name = operator_entry.text().trim().to_string();
            saved.check_updates = update_check.is_active();
            saved.lots_enabled = lots_check.is_active();
            match std::mem::take(&mut *pending_folder.borrow_mut()) {
                PendingFolder::Keep => {}
                PendingFolder::Custom(choice) => {
                    saved.certificates_output = Some(choice.dir);
                    saved.certificates_output_mount = choice.mount;
                }
                PendingFolder::Default => {
                    saved.certificates_output = None;
                    saved.certificates_output_mount = None;
                }
            }
            settings::save(&saved);
            super::update_ui::refresh_indicator();
            dialog.close();
            if language_changed {
                i18n::set_current(chosen);
                if let Some(app) = parent.application() {
                    parent.destroy();
                    build_ui(&app);
                }
            } else if lots_just_enabled
                && !super::lot_ui::is_active()
                && !operations_active.get()
                && !busy::restart_pending()
            {
                // Mode lot coché à l'instant : dialogue de lot tout de suite,
                // sans relancer l'application (sauf mise à jour installée en
                // attente de relance : le lot s'ouvrira sous la nouvelle
                // version).
                super::lot_ui::startup(&parent, Box::new(|| {}));
            }
        });
    }

    // Pendant l'installation, la fenêtre ne se ferme pas : sinon un
    // effacement pourrait démarrer pendant qu'apt ou dnf remplace secmet et
    // secmet-helper.
    dialog.connect_close_request(|_| {
        if busy::installing() {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    // État relu en continu tant que la fenêtre vit : opérations qui
    // démarrent ou finissent, lot ouvert ou clôturé, installation.
    let alive = Rc::new(Cell::new(true));
    {
        let alive = alive.clone();
        dialog.connect_destroy(move |_| alive.set(false));
    }
    {
        let weak = dialog.downgrade();
        glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            if !alive.get() || weak.upgrade().is_none() {
                return glib::ControlFlow::Break;
            }
            refresh_install();
            apply_lang_lock();
            let installing = busy::installing();
            cancel_btn.set_sensitive(!installing);
            apply_btn.set_sensitive(!installing);
            glib::ControlFlow::Continue
        });
    }

    dialog.present();
}
