//! Levée automatique d'un mot de passe ATA laissé par SecMet.
//!
//! Un SECURITY ERASE UNIT interrompu (coupure d'alimentation, arrêt brutal)
//! laisse le disque avec le mot de passe aléatoire posé juste avant, conservé
//! dans le coffre ([`PasswordVault`]). À chaque scan, si le disque rapporte
//! un mot de passe actif et que son numéro de série figure dans le coffre,
//! le helper déverrouille puis lève ce mot de passe, et retire l'entrée du
//! coffre. Un mot de passe étranger (absent du coffre) n'est jamais touché.

use secmet_device::ata::{security, AtaDevice};
use secmet_device::block::BlockDevice;
use serde::{Deserialize, Serialize};

use crate::identity::Scan;
use crate::vault::PasswordVault;

/// Issue d'une tentative de levée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStatus {
    /// Mot de passe levé, coffre nettoyé ; le disque doit être réinspecté.
    Recovered,
    /// Impossible pour l'instant (gelé, compteur de tentatives expiré).
    Deferred,
    /// Le disque a refusé la commande ; l'entrée du coffre est conservée.
    Failed,
}

/// Compte rendu d'une levée, exposé dans la réponse de `ScanDevice`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recovery {
    /// Issue.
    pub status: RecoveryStatus,
    /// Explication lisible (journal et UI).
    pub detail: String,
}

impl Recovery {
    fn new(status: RecoveryStatus, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
        }
    }
}

/// Tente la levée pour le disque de `scan`, ouvert dans `device`.
///
/// `None` : rien à faire (pas ATA, pas de mot de passe actif, ou mot de
/// passe inconnu du coffre).
pub fn recover_ata_password(
    device: &BlockDevice,
    scan: &Scan,
    vault: &PasswordVault,
) -> Option<Recovery> {
    let ata = scan.caps.ata?;
    if !ata.security_enabled {
        return None;
    }
    let serial = scan.identity.serial.as_str();
    if serial.is_empty() {
        return None;
    }
    let password = match vault.load(serial) {
        Ok(Some(password)) => password,
        Ok(None) => return None,
        Err(e) => {
            return Some(Recovery::new(
                RecoveryStatus::Failed,
                format!("coffre illisible pour {serial} : {e}"),
            ))
        }
    };
    if ata.security_frozen {
        return Some(Recovery::new(
            RecoveryStatus::Deferred,
            "mot de passe SecMet connu, mais le disque est gelé (FROZEN) : dégel requis",
        ));
    }
    if ata.security_attempts_expired {
        return Some(Recovery::new(
            RecoveryStatus::Deferred,
            "mot de passe SecMet connu, mais le compteur de tentatives est expiré : \
             cycle d'alimentation du disque requis",
        ));
    }
    let drive = AtaDevice::for_transport(device.file(), scan.entry.transport);
    // Un refus du disque met l'entrée de côté : pas de nouvelle tentative
    // automatique au scan suivant (cinq refus gèlent le disque).
    if ata.security_locked {
        if let Err(e) = drive.security_unlock(security::Identifier::User, &password) {
            let _ = vault.mark_failed(serial);
            return Some(Recovery::new(
                RecoveryStatus::Failed,
                format!("SECURITY UNLOCK refusé : {e} (entrée mise de côté, pas de nouvel essai)"),
            ));
        }
    }
    if let Err(e) = drive.security_disable_password(security::Identifier::User, &password) {
        let _ = vault.mark_failed(serial);
        return Some(Recovery::new(
            RecoveryStatus::Failed,
            format!(
                "SECURITY DISABLE PASSWORD refusé : {e} (entrée mise de côté, pas de nouvel essai)"
            ),
        ));
    }
    match drive.identify() {
        Ok(identify) if !identify.security.enabled => {
            let cleanup = match vault.remove(serial) {
                Ok(()) => String::new(),
                Err(e) => format!(" (entrée du coffre non retirée : {e})"),
            };
            Some(Recovery::new(
                RecoveryStatus::Recovered,
                format!("mot de passe ATA SecMet levé sur {serial}{cleanup}"),
            ))
        }
        Ok(_) => Some(Recovery::new(
            RecoveryStatus::Failed,
            "le disque rapporte encore un mot de passe actif après SECURITY DISABLE PASSWORD",
        )),
        Err(e) => Some(Recovery::new(
            RecoveryStatus::Failed,
            format!("relecture IDENTIFY impossible après la levée : {e}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn statuses_serialize_in_snake_case() {
        let json = serde_json::to_string(&Recovery::new(RecoveryStatus::Deferred, "x")).unwrap();
        assert_eq!(json, r#"{"status":"deferred","detail":"x"}"#);
    }
}
