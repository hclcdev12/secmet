//! Commandes admin NVMe (NVM Express Base 2.0) par `NVME_IOCTL_ADMIN_CMD` :
//! Identify, Get Log Page, Sanitize, Format NVM.
//!
//! Remplace `nvme id-ctrl`, `nvme id-ns`, `nvme smart-log`, `nvme sanitize`,
//! `nvme sanitize-log` et `nvme format --ses=N`. La construction des
//! commandes est pure et testée ; l'envoi passe par [`crate::sys::nvme_admin`].

pub mod decode;

pub use decode::{HealthLog, IdentifyController, IdentifyNamespace, SanitizeLog, SanitizeState};

use std::fs::File;
use std::os::fd::AsFd;

use crate::aligned::AlignedBuffer;
use crate::sys::{self, NvmeOutcome, NvmePassthruCmd};
use crate::{DeviceError, Result};

/// Opcodes admin.
pub const OP_GET_LOG_PAGE: u8 = 0x02;
/// Identify.
pub const OP_IDENTIFY: u8 = 0x06;
/// Format NVM.
pub const OP_FORMAT_NVM: u8 = 0x80;
/// Sanitize.
pub const OP_SANITIZE: u8 = 0x84;

/// Pages de log.
pub const LOG_SMART_HEALTH: u8 = 0x02;
/// Sanitize Status.
pub const LOG_SANITIZE_STATUS: u8 = 0x81;

/// Tous les namespaces (`NSID FFFFFFFFh`).
pub const ALL_NAMESPACES: u32 = 0xffff_ffff;

const SHORT_TIMEOUT_MS: u32 = 10_000;
const SANITIZE_START_TIMEOUT_MS: u32 = 60_000;
/// Format NVM bloque jusqu'à la fin : un user-data erase peut durer des heures.
const FORMAT_TIMEOUT_MS: u32 = 4 * 3600 * 1000;

/// Une commande admin construite, avant attachement de son tampon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminCommand {
    /// Opcode.
    pub opcode: u8,
    /// Namespace.
    pub nsid: u32,
    /// CDW10.
    pub cdw10: u32,
    /// CDW11.
    pub cdw11: u32,
    /// Octets transférés depuis le contrôleur (0 si aucun).
    pub data_len: u32,
    /// Délai en millisecondes.
    pub timeout_ms: u32,
}

/// Identify avec un CNS donné (00h namespace, 01h contrôleur), 4096 octets.
pub fn identify(cns: u8, nsid: u32) -> AdminCommand {
    AdminCommand {
        opcode: OP_IDENTIFY,
        nsid,
        cdw10: u32::from(cns),
        cdw11: 0,
        data_len: decode::IDENTIFY_SIZE as u32,
        timeout_ms: SHORT_TIMEOUT_MS,
    }
}

/// Get Log Page de `bytes` octets (multiple de 4) : NUMD = dwords − 1,
/// moitié basse en CDW10 31:16, moitié haute en CDW11 15:0.
pub fn get_log_page(lid: u8, nsid: u32, bytes: u32) -> AdminCommand {
    let numd = bytes / 4 - 1;
    AdminCommand {
        opcode: OP_GET_LOG_PAGE,
        nsid,
        cdw10: u32::from(lid) | (numd & 0xffff) << 16,
        cdw11: numd >> 16,
        data_len: bytes,
        timeout_ms: SHORT_TIMEOUT_MS,
    }
}

/// Actions Sanitize (CDW10 SANACT, bits 2:0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SanitizeAction {
    /// 001b : sortir du mode Failure.
    ExitFailureMode,
    /// 010b : Block Erase.
    BlockErase,
    /// 011b : Overwrite (`passes` passes, motif `pattern`).
    Overwrite {
        /// Nombre de passes (1 à 16).
        passes: u8,
        /// Motif 32 bits (CDW11).
        pattern: u32,
    },
    /// 100b : Crypto Erase.
    CryptoErase,
}

/// Commande Sanitize ; porte sur tout le contrôleur (NSID 0). AUSE reste à 0 :
/// un échec n'est pas masqué par une sortie automatique du mode Failure.
pub fn sanitize(action: SanitizeAction) -> Result<AdminCommand> {
    let (sanact, owpass, pattern) = match action {
        SanitizeAction::ExitFailureMode => (0b001, 0, 0),
        SanitizeAction::BlockErase => (0b010, 0, 0),
        SanitizeAction::Overwrite { passes, pattern } => {
            if !(1..=16).contains(&passes) {
                return Err(DeviceError::InvalidArgument(
                    "Sanitize Overwrite : 1 à 16 passes".to_string(),
                ));
            }
            (0b011, u32::from(passes & 0x0f), pattern)
        }
        SanitizeAction::CryptoErase => (0b100, 0, 0),
    };
    Ok(AdminCommand {
        opcode: OP_SANITIZE,
        nsid: 0,
        cdw10: sanact | owpass << 4,
        cdw11: pattern,
        data_len: 0,
        timeout_ms: SANITIZE_START_TIMEOUT_MS,
    })
}

/// Secure Erase Setting de Format NVM (CDW10 SES, bits 11:9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureErase {
    /// 000b : aucun effacement sécurisé (format simple).
    None,
    /// 001b : user data erase (`--ses=1`).
    UserData,
    /// 010b : cryptographic erase (`--ses=2`).
    Cryptographic,
}

/// Format NVM conservant le format LBA courant `lbaf` (0..63, réparti entre
/// CDW10 3:0 et 13:12), sans métadonnées ni protection.
pub fn format(nsid: u32, lbaf: u8, ses: SecureErase) -> AdminCommand {
    let ses = match ses {
        SecureErase::None => 0b000,
        SecureErase::UserData => 0b001,
        SecureErase::Cryptographic => 0b010,
    };
    let lbaf = u32::from(lbaf);
    AdminCommand {
        opcode: OP_FORMAT_NVM,
        nsid,
        cdw10: (lbaf & 0x0f) | ses << 9 | (lbaf >> 4 & 0x03) << 12,
        cdw11: 0,
        data_len: 0,
        timeout_ms: FORMAT_TIMEOUT_MS,
    }
}

/// Un namespace NVMe ouvert (`/dev/nvmeXnY`).
pub struct NvmeDevice<'a> {
    file: &'a File,
    nsid: u32,
}

impl<'a> NvmeDevice<'a> {
    /// Le namespace derrière `file`.
    pub fn new(file: &'a File) -> Result<Self> {
        let nsid = sys::nvme_namespace_id(file.as_fd())
            .map_err(|e| DeviceError::io("NVME_IOCTL_ID", e))?;
        Ok(Self { file, nsid })
    }

    /// Namespace ID.
    pub fn nsid(&self) -> u32 {
        self.nsid
    }

    /// Exécute une commande admin et renvoie son tampon de données.
    pub fn run(&self, command: AdminCommand) -> Result<AlignedBuffer> {
        let mut data = AlignedBuffer::new(command.data_len as usize);
        let mut raw = NvmePassthruCmd {
            opcode: command.opcode,
            nsid: command.nsid,
            cdw10: command.cdw10,
            cdw11: command.cdw11,
            data_len: command.data_len,
            addr: if command.data_len == 0 {
                0
            } else {
                data.as_mut_ptr() as u64
            },
            timeout_ms: command.timeout_ms,
            ..NvmePassthruCmd::default()
        };
        // `data` vit jusqu'au retour et contient `data_len` octets : contrat de `sys::nvme_admin`.
        match sys::nvme_admin(self.file.as_fd(), &mut raw).map_err(|e| {
            DeviceError::io(
                format!("NVME_IOCTL_ADMIN_CMD (opcode 0x{:02x})", command.opcode),
                e,
            )
        })? {
            NvmeOutcome::Completed(_) => Ok(data),
            NvmeOutcome::Status(code) => Err(DeviceError::NvmeStatus {
                code,
                meaning: status_meaning(code),
            }),
        }
    }

    /// Identify Controller, décodé.
    pub fn identify_controller(&self) -> Result<IdentifyController> {
        Ok(decode::identify_controller(&self.run(identify(0x01, 0))?)?)
    }

    /// Identify Namespace, décodé.
    pub fn identify_namespace(&self) -> Result<IdentifyNamespace> {
        Ok(decode::identify_namespace(
            &self.run(identify(0x00, self.nsid))?,
        )?)
    }

    /// SMART / Health Information log.
    pub fn health(&self) -> Result<HealthLog> {
        let log = self.run(get_log_page(
            LOG_SMART_HEALTH,
            ALL_NAMESPACES,
            decode::HEALTH_LOG_SIZE as u32,
        ))?;
        Ok(decode::health(&log)?)
    }

    /// Sanitize Status log.
    pub fn sanitize_log(&self) -> Result<SanitizeLog> {
        let log = self.run(get_log_page(LOG_SANITIZE_STATUS, 0, 512))?;
        Ok(decode::sanitize_log(&log)?)
    }

    /// Démarre un sanitize ; il continue en arrière-plan, à suivre par
    /// [`Self::sanitize_log`]. Efface **tous** les namespaces du contrôleur.
    pub fn start_sanitize(&self, action: SanitizeAction) -> Result<()> {
        self.run(sanitize(action)?).map(|_| ())
    }

    /// Format NVM avec effacement sécurisé, en conservant le format LBA courant.
    /// Bloque jusqu'à la fin. Selon FNA bit 1, peut porter sur tous les namespaces.
    pub fn format(&self, ses: SecureErase) -> Result<()> {
        let namespace = self.identify_namespace()?;
        self.run(format(self.nsid, namespace.lba_format_index, ses))
            .map(|_| ())
    }
}

/// Signification des statuts NVMe courants (SCT 0 generic, SCT 1 command specific).
pub fn status_meaning(code: u16) -> &'static str {
    // Le statut remonté par le noyau conserve DNR (bit 14), More (bit 13)
    // et CRD (bits 12:11) : seuls SCT (10:8) et SC (7:0) portent le sens.
    match code & 0x7ff {
        0x000 => "succès",
        0x001 => "opcode invalide",
        0x002 => "champ invalide",
        0x004 => "erreur de transfert",
        0x005 => "commande abandonnée (power loss)",
        0x006 => "erreur interne",
        0x007 => "commande abandonnée (abort requested)",
        0x00b => "namespace invalide ou non attaché",
        0x00c => "séquence de commandes invalide",
        0x012 => "commande interdite sur un namespace actif",
        0x016 => "ANA : path non accessible",
        0x01f => "tentative hors plage",
        0x082 => "lecture seule",
        0x10a => "format invalide",
        0x10b => "firmware activation requiert reset",
        0x11d => "sanitize en cours",
        0x11e => "échec du sanitize précédent (mode Failure)",
        0x123 => "sanitize interdit tant qu'une commande Format est en cours",
        _ => "autre",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn identify_commands() {
        let ctrl = identify(0x01, 0);
        assert_eq!(
            (ctrl.opcode, ctrl.nsid, ctrl.cdw10, ctrl.data_len),
            (0x06, 0, 1, 4096)
        );
        assert_eq!(identify(0x00, 3).nsid, 3);
    }

    #[test]
    fn get_log_page_encodes_dword_count() {
        let smart = get_log_page(0x02, 0xffff_ffff, 512);
        assert_eq!(smart.cdw10, 0x007f_0002);
        assert_eq!(smart.cdw11, 0);
        let big = get_log_page(0x81, 0, 512 * 1024);
        assert_eq!(big.cdw10, 0xffff_0081);
        assert_eq!(big.cdw11, 1);
    }

    #[test]
    fn sanitize_actions() {
        assert_eq!(sanitize(SanitizeAction::BlockErase).unwrap().cdw10, 0b010);
        assert_eq!(sanitize(SanitizeAction::CryptoErase).unwrap().cdw10, 0b100);
        assert_eq!(
            sanitize(SanitizeAction::ExitFailureMode).unwrap().cdw10,
            0b001
        );
        let ow = sanitize(SanitizeAction::Overwrite {
            passes: 3,
            pattern: 0xdead_beef,
        })
        .unwrap();
        assert_eq!(ow.cdw10, 0b011 | 3 << 4);
        assert_eq!(ow.cdw11, 0xdead_beef);
        assert!(sanitize(SanitizeAction::Overwrite {
            passes: 0,
            pattern: 0
        })
        .is_err());
        assert!(sanitize(SanitizeAction::Overwrite {
            passes: 17,
            pattern: 0
        })
        .is_err());
        let c = sanitize(SanitizeAction::BlockErase).unwrap();
        assert_eq!((c.opcode, c.nsid, c.data_len), (0x84, 0, 0));
    }

    #[test]
    fn format_keeps_the_lba_format_and_sets_ses() {
        assert_eq!(format(1, 1, SecureErase::Cryptographic).cdw10, 0x0401);
        assert_eq!(format(1, 0, SecureErase::UserData).cdw10, 0x0200);
        assert_eq!(format(1, 0, SecureErase::None).cdw10, 0x0000);
        assert_eq!(format(1, 17, SecureErase::UserData).cdw10, 0x1201);
        assert_eq!(format(1, 0, SecureErase::None).opcode, 0x80);
    }

    #[test]
    fn known_statuses_have_a_meaning() {
        assert_eq!(status_meaning(0x11d), "sanitize en cours");
        assert_eq!(status_meaning(0x7ff), "autre");
    }
}
