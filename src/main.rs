//! Application graphique d'effacement sécurisé de disques (GTK4).
//!
//! Les opérations privilégiées passent par le daemon `secmet-helper serve`,
//! lancé une seule fois via PolicyKit (voir `helper_client`).

use secmet::app_identity::APP_NAME;
use secmet::migrate;
use secmet::pkexec_gate;
use secmet::ui::SecMetApp;

fn main() {
    migrate::run_user_data_migration();

    if !pkexec_gate::check_pkexec_available() {
        eprintln!("❌ ERREUR: La commande 'pkexec' n'est pas disponible sur ce système.");
        eprintln!("Veuillez installer le paquet 'policykit-1'.");
        std::process::exit(1);
    }

    if let Err(msg) = pkexec_gate::check_pkexec_setuid() {
        eprintln!("❌ ERREUR PolicyKit:\n{msg}");
        eprintln!("Corrigez avec: make fix-pkexec puis make verify-polkit");
        std::process::exit(1);
    }

    println!("🚀 Lancement de {APP_NAME}…");
    let app = SecMetApp::new();
    app.run();
}
