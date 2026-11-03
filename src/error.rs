//! Erreurs typées (messages FR conservés pour l'UI).

use std::fmt;

/// Erreurs d'authentification / PolicyKit au démarrage.
#[derive(Debug, Clone)]
pub enum AuthError {
    PkexecMissing,
    PkexecNoSetuid { path: String },
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::PkexecMissing => write!(
                f,
                "La commande pkexec est introuvable. Installez le paquet policykit-1."
            ),
            AuthError::PkexecNoSetuid { path } => write!(
                f,
                "pkexec ({path}) n'a pas le bit setuid root requis.\n\
                 pkexec refuse de s'exécuter (code 127) sans ce bit.\n\
                 Corrigez avec: make fix-pkexec\n\
                 ou: sudo chmod u+s {path}\n\
                 ou: sudo apt install --reinstall policykit-1"
            ),
        }
    }
}

impl std::error::Error for AuthError {}
