//! Autorisation / élévation au démarrage et pour les actions sensibles.
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Box as GtkBox, CheckButton, Label};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use super::dialogs::{confirm_action_dual, show_error_dialog, show_fatal_dialog};
use super::disk_list::{load_disks_into_list, update_host_notice, LoadOutcome};
use super::testdisk_ui::update_testdisk_button_state;
use super::types::{DiskWidget, JobRecord};
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
                if matches!(
                    system_install::check_install_needed(&components),
                    InstallNeed::Update
                ) {
                    return Ok(ComponentState::Prompt(
                        InstallNeed::Update,
                        components,
                        String::new(),
                    ));
                }
            }
            Ok(ComponentState::Ready)
        }
        Err(e) => match system_install::bundled_components() {
            // AppImage : l'installation du composant manquant est proposée.
            Some(components) => Ok(ComponentState::Prompt(InstallNeed::Install, components, e)),
            // Hors AppImage (.deb / dev) : comportement historique.
            None => Err(e),
        },
    }
}

/// Propose l'installation / la mise à jour du composant système puis poursuit
/// (`after`) ou referme l'application selon la réponse et la criticité.
fn prompt_system_component<F>(
    window: &ApplicationWindow,
    need: InstallNeed,
    components: BundleComponents,
    detail: String,
    after: F,
) where
    F: FnOnce() + 'static,
{
    let window = window.clone();
    let (title, message) = match need {
        InstallNeed::Install => (
            tr("Composant système requis"),
            trf(
                "SecMet doit installer son composant système sur cette machine :\n\
                 helper privilégié, policy PolicyKit et outils disque embarqués.\n\n\
                 Motif : {detail}\n\n\
                 Installer maintenant ?\n\
                 (un mot de passe administrateur sera demandé)",
                &[("detail", detail.as_str())],
            ),
        ),
        InstallNeed::Update => (
            tr("Mise à jour du composant système"),
            tr(
                "La version embarquée du composant système (helper, outils) diffère\n\
                de celle installée sur cette machine.\n\n\
                Mettre à jour maintenant ?\n\
                (un mot de passe administrateur sera demandé)",
            ),
        ),
        InstallNeed::UpToDate => unreachable!("pas de prompt si le composant est à jour"),
    };

    // La suite du démarrage est partagée entre les deux issues (mise à jour
    // refusée → on continue avec le composant existant, qui est cohérent).
    let after = Rc::new(RefCell::new(Some(after)));
    let take_after = after.clone();

    let window_confirm = window.clone();
    let components_confirm = components.clone();
    let on_confirm = move || {
        let result = system_install::run_installer(&components_confirm)
            .and_then(|()| crate::paths::verify_polkit_policy());
        if let Err(e) = result {
            eprintln!("❌ Installation du composant système impossible: {e}");
            show_fatal_dialog(
                &window_confirm,
                &tr("Installation impossible"),
                &trf(
                    "{error}\n\nL'application va se fermer.",
                    &[("error", e.as_str())],
                ),
            );
            return;
        }
        if let Some(f) = take_after.borrow_mut().take() {
            f();
        }
    };

    let on_decline: Box<dyn FnOnce()> = match need {
        InstallNeed::Install => {
            let window = window.clone();
            Box::new(move || {
                show_fatal_dialog(
                    &window,
                    &tr("Composant système absent"),
                    &tr("Sans le composant système, SecMet ne peut pas effectuer\n\
                         d'opérations sur les disques.\n\nL'application va se fermer."),
                );
            })
        }
        // Mise à jour refusée : le composant installé reste fonctionnel.
        _ => {
            let take_after = after.clone();
            Box::new(move || {
                if let Some(f) = take_after.borrow_mut().take() {
                    f();
                }
            })
        }
    };

    confirm_action_dual(&window, &title, &message, on_confirm, on_decline);
}

/// Fin du démarrage : session agent (1 seul prompt pkexec) puis chargement,
/// sans bloquer la fenêtre pendant l'invite Polkit.
#[allow(clippy::too_many_arguments)]
fn finish_startup(
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
) {
    let window_done = window.clone();
    let disk_widgets_done = disk_widgets.clone();
    let operations_active_done = operations_active.clone();
    let testdisk_button = testdisk_button.clone();
    let refresh_button = refresh_button.clone();
    let host_notice = host_notice.clone();
    let disk_list_box = disk_list_box.clone();
    let disk_widgets = disk_widgets.clone();
    let window_load = window.clone();
    let job_states = job_states.clone();
    let smart_auth_ok = smart_auth_ok.clone();
    let select_all_button = select_all_button.clone();
    let select_all_updating = select_all_updating.clone();
    let selected_total_label = selected_total_label.clone();
    let operations_active = operations_active.clone();
    // Clé USB : lot à reprendre ou à ouvrir avant l'inventaire (annexe M).
    super::lot_ui::startup(
        window,
        Box::new(move || {
            load_disks_into_list(
                &disk_list_box,
                &disk_widgets,
                &window_load,
                &job_states,
                &smart_auth_ok,
                &select_all_button,
                &select_all_updating,
                &selected_total_label,
                &operations_active,
                Box::new(move |outcome| match outcome {
                    LoadOutcome::Loaded { root_traced } => {
                        update_host_notice(&host_notice, root_traced);
                        refresh_button.set_sensitive(true);
                        update_testdisk_button_state(
                            &testdisk_button,
                            &disk_widgets_done,
                            &operations_active_done,
                        );
                    }
                    LoadOutcome::AuthFailed(e) => {
                        eprintln!("❌ Autorisation administrateur refusée ou impossible: {e}");
                        show_fatal_dialog(
                            &window_done,
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
                            &window_done,
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
) {
    let window = window.clone();
    let disk_list_box = disk_list_box.clone();
    let disk_widgets = disk_widgets.clone();
    let job_states = job_states.clone();
    let smart_auth_ok = smart_auth_ok.clone();
    let select_all_button = select_all_button.clone();
    let select_all_updating = select_all_updating.clone();
    let selected_total_label = selected_total_label.clone();
    let operations_active = operations_active.clone();
    let testdisk_button = testdisk_button.clone();
    let refresh_button = refresh_button.clone();
    let host_notice = host_notice.clone();

    glib::idle_add_local_once(move || {
        let finish = {
            let window = window.clone();
            let disk_list_box = disk_list_box.clone();
            let disk_widgets = disk_widgets.clone();
            let job_states = job_states.clone();
            let smart_auth_ok = smart_auth_ok.clone();
            let select_all_button = select_all_button.clone();
            let select_all_updating = select_all_updating.clone();
            let selected_total_label = selected_total_label.clone();
            let operations_active = operations_active.clone();
            let testdisk_button = testdisk_button.clone();
            let refresh_button = refresh_button.clone();
            let host_notice = host_notice.clone();
            move || {
                finish_startup(
                    &window,
                    &disk_list_box,
                    &disk_widgets,
                    &job_states,
                    &smart_auth_ok,
                    &select_all_button,
                    &select_all_updating,
                    &selected_total_label,
                    &operations_active,
                    &testdisk_button,
                    &refresh_button,
                    &host_notice,
                );
            }
        };

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
