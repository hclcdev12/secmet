//! Autorisation / élévation au démarrage et pour les actions sensibles.
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Box as GtkBox, CheckButton, Label};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use super::dialogs::{confirm_choice, show_error_dialog, show_fatal_dialog};
use super::disk_list::{load_disks_into_list, update_host_notice, LoadOutcome};
use super::testdisk_ui::update_testdisk_button_state;
use super::types::{CondemnContext, DiskWidget, JobRecord};
use gtk4::Button;

use crate::i18n::{tr, trf};
use crate::system_install::{self, BundleComponents, InstallNeed};

pub(super) fn authorize_for_action(
    window: &ApplicationWindow,
    smart_auth_ok: &Rc<Cell<bool>>,
    title: &str,
) -> bool {
    // Session déjà ouverte : aucun nouveau prompt.
    if crate::helper_client::is_active() {
        smart_auth_ok.set(true);
        return true;
    }
    // Invite Polkit déjà affichée par un chargement en cours : ne pas
    // bloquer le fil GTK derrière elle.
    if crate::helper_client::is_starting() {
        show_error_dialog(
            window,
            title,
            &tr("Authentification administrateur en cours : patientez avant de relancer cette action."),
        );
        return false;
    }
    match crate::helper_client::ensure() {
        Ok(_) => {
            smart_auth_ok.set(true);
            true
        }
        Err(e) => {
            smart_auth_ok.set(false);
            show_error_dialog(window, title, &e);
            false
        }
    }
}

/// État du composant système vis-à-vis du démarrage de l'application.
enum ComponentState {
    /// Composant prêt : poursuite directe de l'autorisation.
    Ready,
    /// Composant à installer (policy manquante/incohérente — `detail` porte
    /// l'erreur) ou à mettre à jour (bundle différent de l'installé).
    Prompt(InstallNeed, BundleComponents, String),
}

fn system_component_state() -> Result<ComponentState, String> {
    match crate::paths::verify_polkit_policy() {
        Ok(()) => {
            if let Some(components) = system_install::bundled_components() {
                let need = system_install::check_install_needed(&components);
                if matches!(need, InstallNeed::Update | InstallNeed::Downgrade) {
                    return Ok(ComponentState::Prompt(need, components, String::new()));
                }
            }
            Ok(ComponentState::Ready)
        }
        Err(e) => match system_install::bundled_components() {
            // AppImage : l'installation du composant manquant est proposée.
            Some(components) => Ok(ComponentState::Prompt(InstallNeed::Install, components, e)),
            // Hors AppImage (.deb / dev) : le paquet est à réinstaller.
            None => Err(format!("{e}\nRéinstallez le paquet SecMet.")),
        },
    }
}

/// Propose l'installation / la mise à jour du composant système puis poursuit
/// (`after`) ou referme l'application selon la réponse et la criticité.
///
/// Un seul mot de passe : le script d'installation enchaîne sur le démarrage
/// du helper (`helper_client::ensure_installing`), la session est ouverte
/// quand `after` charge les disques.
fn prompt_system_component<F>(
    window: &ApplicationWindow,
    need: InstallNeed,
    components: BundleComponents,
    detail: String,
    after: F,
) where
    F: FnOnce() + 'static,
{
    if !detail.is_empty() {
        eprintln!("ℹ️ Installation du composant système proposée : {detail}");
    }
    let after: After = Rc::new(RefCell::new(Some(Box::new(after))));
    ask_system_component(window, need, components, after);
}

type After = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

fn run_after(after: &After) {
    if let Some(f) = after.borrow_mut().take() {
        f();
    }
}

fn ask_system_component(
    window: &ApplicationWindow,
    need: InstallNeed,
    components: BundleComponents,
    after: After,
) {
    let (title, message, answers) = match need {
        InstallNeed::Install => (
            tr("Installation de SecMet"),
            tr("Première utilisation de SecMet sur cet ordinateur : SecMet doit installer son composant système, qui donne accès aux disques et contient les outils d'effacement.\n\nLe mot de passe administrateur de l'ordinateur sera demandé une fois."),
            (tr("Installer"), tr("Quitter")),
        ),
        InstallNeed::Update => (
            tr("Mise à jour du composant système"),
            tr("Cette version de SecMet apporte une nouvelle version de son composant système, déjà installé sur cet ordinateur.\n\nLe mot de passe administrateur de l'ordinateur sera demandé une fois."),
            (tr("Mettre à jour"), tr("Plus tard")),
        ),
        InstallNeed::Downgrade => (
            tr("Composant système plus récent"),
            trf(
                "Le composant système installé sur cet ordinateur ({installed}) est plus récent que cette version de SecMet ({bundled}). Utilisez de préférence la version la plus récente de SecMet.\n\nContinuer avec cette version remplace le composant par le sien, plus ancien (mot de passe administrateur demandé une fois).",
                &[
                    (
                        "installed",
                        system_install::installed_version()
                            .unwrap_or_default()
                            .as_str(),
                    ),
                    ("bundled", system_install::BUNDLED_VERSION),
                ],
            ),
            (tr("Remplacer"), tr("Quitter")),
        ),
        InstallNeed::UpToDate => unreachable!("pas de prompt si le composant est à jour"),
    };

    let on_confirm = {
        let window = window.clone();
        let components = components.clone();
        let after = after.clone();
        move || install_then_continue(&window, need, components, after)
    };
    let on_decline = {
        let window = window.clone();
        move || {
            match need {
            // Version plus récente installée, remplacement refusé : quitter
            // (cette version ne parle pas forcément au composant récent).
            InstallNeed::Downgrade => window.close(),
            InstallNeed::Install => show_fatal_dialog(
                &window,
                &tr("Composant système absent"),
                &tr("Sans son composant système, SecMet ne peut pas accéder aux disques.\n\nL'application va se fermer."),
            ),
            // Mise à jour refusée : le composant installé reste fonctionnel.
            _ => run_after(&after),
        }
        }
    };
    confirm_choice(
        window,
        &title,
        &message,
        (&answers.0, &answers.1),
        on_confirm,
        on_decline,
    );
}

/// Installation + démarrage du helper hors du fil GTK (l'invite Polkit
/// bloque), puis suite du démarrage, nouvelle proposition si
/// l'authentification a été annulée, ou fermeture sur une vraie erreur.
fn install_then_continue(
    window: &ApplicationWindow,
    need: InstallNeed,
    components: BundleComponents,
    after: After,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    let appimage = components.appimage.clone();
    std::thread::spawn(move || {
        let result = crate::helper_client::ensure_installing(&appimage)
            .map(|_| ())
            .and_then(|()| crate::paths::verify_polkit_policy());
        let _ = tx.send(result);
    });
    let window = window.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(150), move || {
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err(tr("Installation interrompue."))
            }
        };
        match result {
            Ok(()) => run_after(&after),
            Err(e) if crate::helper_client::is_auth_cancelled(&e) => {
                // Mot de passe annulé ou refusé : on repropose au lieu de fermer.
                ask_system_component(&window, need, components.clone(), after.clone());
            }
            Err(e) => {
                eprintln!("❌ Installation du composant système impossible: {e}");
                show_fatal_dialog(
                    &window,
                    &tr("Installation impossible"),
                    &trf(
                        "{error}\n\nL'application va se fermer.",
                        &[("error", e.as_str())],
                    ),
                );
            }
        }
        glib::ControlFlow::Break
    });
}

/// Tout ce dont la fin du démarrage a besoin (repris tel quel quand
/// l'authentification annulée est reproposée).
#[derive(Clone)]
struct Startup {
    window: ApplicationWindow,
    disk_list_box: GtkBox,
    disk_widgets: Rc<RefCell<Vec<DiskWidget>>>,
    job_states: Rc<RefCell<HashMap<String, JobRecord>>>,
    smart_auth_ok: Rc<Cell<bool>>,
    select_all_button: CheckButton,
    select_all_updating: Rc<Cell<bool>>,
    selected_total_label: Label,
    operations_active: Rc<Cell<bool>>,
    testdisk_button: Button,
    refresh_button: Button,
    host_notice: Label,
    condemn: CondemnContext,
}

/// Fin du démarrage : session agent (1 seul prompt pkexec) puis chargement,
/// sans bloquer la fenêtre pendant l'invite Polkit.
fn finish_startup(ctx: Startup) {
    let window = ctx.window.clone();
    // Clé USB : lot à reprendre ou à ouvrir avant l'inventaire (annexe M).
    super::lot_ui::startup(
        &window,
        Box::new(move || {
            let done = ctx.clone();
            load_disks_into_list(
                &ctx.disk_list_box,
                &ctx.disk_widgets,
                &ctx.window,
                &ctx.job_states,
                &ctx.smart_auth_ok,
                &ctx.select_all_button,
                &ctx.select_all_updating,
                &ctx.selected_total_label,
                &ctx.operations_active,
                &ctx.condemn,
                Box::new(move |outcome| match outcome {
                    LoadOutcome::Loaded { root_traced } => {
                        update_host_notice(&done.host_notice, root_traced);
                        done.refresh_button.set_sensitive(true);
                        update_testdisk_button_state(
                            &done.testdisk_button,
                            &done.disk_widgets,
                            &done.operations_active,
                        );
                    }
                    LoadOutcome::AuthFailed(e) if crate::helper_client::is_auth_cancelled(&e) => {
                        // Mot de passe annulé ou refusé : on repropose au
                        // lieu de fermer l'application.
                        let window = done.window.clone();
                        let retry = done.clone();
                        confirm_choice(
                            &done.window,
                            &tr("Authentification annulée"),
                            &tr("SecMet a besoin du mot de passe administrateur de l'ordinateur pour accéder aux disques."),
                            (&tr("Réessayer"), &tr("Quitter")),
                            move || finish_startup(retry),
                            move || window.close(),
                        );
                    }
                    LoadOutcome::AuthFailed(e) => {
                        eprintln!("❌ Autorisation administrateur refusée ou impossible: {e}");
                        show_fatal_dialog(
                            &done.window,
                            &tr("Autorisation refusée"),
                            &trf(
                                "{error}\n\nL'application va se fermer.",
                                &[("error", e.as_str())],
                            ),
                        );
                    }
                    LoadOutcome::InventoryFailed(e) => {
                        eprintln!("❌ Inventaire disques impossible: {e}");
                        show_fatal_dialog(
                            &done.window,
                            &tr("Inventaire impossible"),
                            &trf(
                                "{error}\n\nL'application va se fermer.",
                                &[("error", e.as_str())],
                            ),
                        );
                    }
                }),
            );
        }),
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_startup_authorization(
    window: &ApplicationWindow,
    disk_list_box: &GtkBox,
    disk_widgets: &Rc<RefCell<Vec<DiskWidget>>>,
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    smart_auth_ok: &Rc<Cell<bool>>,
    select_all_button: &CheckButton,
    select_all_updating: &Rc<Cell<bool>>,
    selected_total_label: &Label,
    operations_active: &Rc<Cell<bool>>,
    testdisk_button: &Button,
    refresh_button: &Button,
    host_notice: &Label,
    condemn: &CondemnContext,
) {
    let ctx = Startup {
        window: window.clone(),
        disk_list_box: disk_list_box.clone(),
        disk_widgets: disk_widgets.clone(),
        job_states: job_states.clone(),
        smart_auth_ok: smart_auth_ok.clone(),
        select_all_button: select_all_button.clone(),
        select_all_updating: select_all_updating.clone(),
        selected_total_label: selected_total_label.clone(),
        operations_active: operations_active.clone(),
        testdisk_button: testdisk_button.clone(),
        refresh_button: refresh_button.clone(),
        host_notice: host_notice.clone(),
        condemn: condemn.clone(),
    };

    glib::idle_add_local_once(move || {
        let window = ctx.window.clone();
        let finish = move || finish_startup(ctx);
        match system_component_state() {
            Ok(ComponentState::Ready) => finish(),
            Ok(ComponentState::Prompt(need, components, detail)) => {
                prompt_system_component(&window, need, components, detail, finish);
            }
            Err(e) => {
                eprintln!("❌ Autorisation administrateur refusée ou impossible: {e}");
                show_fatal_dialog(
                    &window,
                    &tr("Autorisation refusée"),
                    &trf(
                        "{error}\n\nL'application va se fermer.",
                        &[("error", e.as_str())],
                    ),
                );
            }
        }
    });
}
