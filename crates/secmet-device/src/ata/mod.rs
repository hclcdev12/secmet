//! Commandes ATA par SCSI/ATA Translation (SAT-4) : ATA PASS-THROUGH (16) et
//! ATA PASS-THROUGH (12) sur `SG_IO`.
//!
//! Couvre les disques SATA natifs (libata), les disques derrière un HBA SAS
//! (SATL du pilote) et les ponts USB-SATA (SATL du pont). Les ponts USB
//! n'acceptent pas tous le CDB de 16 octets : derrière un SATL
//! ([`CdbPolicy::AutoDetect`]), [`AtaDevice`] tente d'abord le 16 et bascule
//! sur le 12 quand le SATL rejette le CDB lui-même (`ILLEGAL REQUEST`, ASC
//! `INVALID COMMAND OPERATION CODE` ou `INVALID FIELD IN CDB`) pour une
//! commande 28 bits. Sur bus natif ([`CdbPolicy::PassThrough16Only`]) il n'y
//! a jamais de repli : un `ILLEGAL REQUEST` de libata signale une commande
//! refusée par le disque, pas un CDB inconnu, et un repli y bloquerait les
//! commandes 48 bits (SANITIZE) qui ne tiennent pas en 12 octets.
//!
//! Construction des CDB et lecture des registres de retour sont pures et
//! testées ; l'envoi passe par [`crate::sys::sg_io`].

pub mod identify;
pub mod security;
pub mod smart;

pub use identify::IdentifyDevice;
pub use security::Password;
pub use smart::{SmartAttributes, SmartStatus};

use std::cell::Cell;
use std::fs::File;
use std::os::fd::AsFd;

use crate::aligned::AlignedBuffer;
use crate::sys::{self, SgIoHdr};
use crate::sysfs::Transport;
use crate::{DeviceError, Result};

/// Taille d'un secteur logique ATA pour les commandes PIO (IDENTIFY, SMART, Security).
pub const SECTOR: usize = 512;

/// Opcodes SCSI.
const ATA_PASS_THROUGH_16: u8 = 0x85;
const ATA_PASS_THROUGH_12: u8 = 0xa1;

/// Commandes ATA (ACS-4).
pub const CMD_IDENTIFY_DEVICE: u8 = 0xec;
/// SMART.
pub const CMD_SMART: u8 = 0xb0;
/// SANITIZE DEVICE.
pub const CMD_SANITIZE_DEVICE: u8 = 0xb4;
/// SECURITY SET PASSWORD.
pub const CMD_SECURITY_SET_PASSWORD: u8 = 0xf1;
/// SECURITY UNLOCK.
pub const CMD_SECURITY_UNLOCK: u8 = 0xf2;
/// SECURITY ERASE PREPARE.
pub const CMD_SECURITY_ERASE_PREPARE: u8 = 0xf3;
/// SECURITY ERASE UNIT.
pub const CMD_SECURITY_ERASE_UNIT: u8 = 0xf4;
/// SECURITY FREEZE LOCK.
pub const CMD_SECURITY_FREEZE_LOCK: u8 = 0xf5;
/// SECURITY DISABLE PASSWORD.
pub const CMD_SECURITY_DISABLE_PASSWORD: u8 = 0xf6;

/// Features SMART et signature (LBA Mid 4Fh, LBA High C2h).
const SMART_READ_DATA: u16 = 0xd0;
const SMART_READ_THRESHOLDS: u16 = 0xd1;
const SMART_RETURN_STATUS: u16 = 0xda;
const SMART_SIGNATURE_LBA: u64 = 0x00c2_4f00;

/// Features SANITIZE DEVICE et signatures (ACS-4 §7.45).
const SANITIZE_STATUS_EXT: u16 = 0x0000;
const SANITIZE_CRYPTO_SCRAMBLE_EXT: u16 = 0x0011;
const SANITIZE_BLOCK_ERASE_EXT: u16 = 0x0012;
const SANITIZE_OVERWRITE_EXT: u16 = 0x0014;
const SANITIZE_FREEZE_LOCK_EXT: u16 = 0x0020;
const SANITIZE_ANTIFREEZE_LOCK_EXT: u16 = 0x0040;
const SIG_CRYPTO: u64 = 0x4372_7970; // "Cryp"
const SIG_BLOCK_ERASE: u64 = 0x426b_4572; // "BkEr"
const SIG_OVERWRITE: u64 = 0x4f57 << 32; // "OW" dans LBA 47:32
const SIG_FREEZE_LOCK: u64 = 0x4672_4c6b; // "FrLk"
const SIG_ANTIFREEZE: u64 = 0x416e_7469; // "Anti"

/// Délais.
/// Un HDD en veille peut mettre plus de 10 s à répondre à IDENTIFY ; un
/// délai trop court déclenche une réinitialisation SCSI en pleine inspection.
const SHORT_TIMEOUT_MS: u32 = 30_000;
const SANITIZE_START_TIMEOUT_MS: u32 = 60_000;

/// Sens du transfert de données.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    /// Aucune donnée ; les registres sont renvoyés (CK_COND).
    None,
    /// PIO data-in de `n` secteurs de 512 octets.
    In(u16),
    /// PIO data-out d'un secteur (commandes Security).
    Out,
}

/// Une commande ATA à passer au disque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtaCommand {
    /// Code de commande.
    pub command: u8,
    /// Registre Features (16 bits en 48 bits).
    pub features: u16,
    /// Registre Count.
    pub count: u16,
    /// LBA (48 bits).
    pub lba: u64,
    /// Registre Device.
    pub device: u8,
    /// Commande 48 bits (EXT).
    pub extend: bool,
    /// Transfert.
    pub transfer: Transfer,
    /// Délai en millisecondes.
    pub timeout_ms: u32,
}

impl AtaCommand {
    fn flags(&self) -> (u8, u8) {
        match self.transfer {
            // Non-data ; CK_COND : renvoyer les registres dans le sense.
            Transfer::None => (3, 0x20),
            // PIO data-in ; T_DIR depuis le disque, BYTE_BLOCK, T_LENGTH dans COUNT.
            Transfer::In(_) => (4, 0x0e),
            // PIO data-out ; T_DIR vers le disque, BYTE_BLOCK, T_LENGTH dans COUNT.
            Transfer::Out => (5, 0x06),
        }
    }

    /// Nombre de secteurs transférés.
    pub fn sectors(&self) -> usize {
        match self.transfer {
            Transfer::None => 0,
            Transfer::In(n) => usize::from(n),
            Transfer::Out => 1,
        }
    }
}

/// Variante de CDB utilisée par un périphérique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cdb {
    /// ATA PASS-THROUGH (16), 48 bits possibles.
    PassThrough16,
    /// ATA PASS-THROUGH (12), 28 bits seulement (vieux ponts USB).
    PassThrough12,
}

/// CDB ATA PASS-THROUGH (16) (SAT-4 §12.2.2).
pub fn cdb16(command: &AtaCommand) -> [u8; 16] {
    let (protocol, flags) = command.flags();
    let lba = command.lba.to_be_bytes(); // lba[2..8] = bits 47..0
    let [features_hi, features_lo] = command.features.to_be_bytes();
    let [count_hi, count_lo] = command.count.to_be_bytes();
    [
        ATA_PASS_THROUGH_16,
        protocol << 1 | u8::from(command.extend),
        flags,
        features_hi,
        features_lo,
        count_hi,
        count_lo,
        lba[4], // LBA 31:24
        lba[7], // LBA 7:0
        lba[3], // LBA 39:32
        lba[6], // LBA 15:8
        lba[2], // LBA 47:40
        lba[5], // LBA 23:16
        command.device,
        command.command,
        0,
    ]
}

/// CDB ATA PASS-THROUGH (12) (SAT-4 §12.2.3) : commandes 28 bits uniquement.
pub fn cdb12(command: &AtaCommand) -> Result<[u8; 12]> {
    if command.extend
        || command.features > 0xff
        || command.count > 0xff
        || command.lba > 0x0fff_ffff
    {
        return Err(DeviceError::NotRepresentable12(command.command));
    }
    let (protocol, flags) = command.flags();
    let lba = command.lba.to_le_bytes();
    Ok([
        ATA_PASS_THROUGH_12,
        protocol << 1,
        flags,
        command.features as u8,
        command.count as u8,
        lba[0],
        lba[1],
        lba[2],
        command.device | (lba[3] & 0x0f),
        command.command,
        0,
        0,
    ])
}

/// Registres ATA renvoyés par le disque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Registers {
    /// Registre Error.
    pub error: u8,
    /// Registre Count.
    pub count: u16,
    /// LBA.
    pub lba: u64,
    /// Registre Device.
    pub device: u8,
    /// Registre Status ; bit 0 = ERR.
    pub status: u8,
}

impl Registers {
    /// La commande a échoué (ERR dans Status).
    pub fn failed(&self) -> bool {
        self.status & 0x01 != 0
    }

    /// LBA Mid (LBA 15:8) et LBA High (LBA 23:16).
    pub fn lba_mid_high(&self) -> (u8, u8) {
        ((self.lba >> 8) as u8, (self.lba >> 16) as u8)
    }
}

/// Clé de sense (bits 3:0 de l'octet 1 ou 2 selon le format).
pub fn sense_key(sense: &[u8]) -> Option<u8> {
    let response = *sense.first()? & 0x7f;
    match response {
        0x70 | 0x71 => Some(*sense.get(2)? & 0x0f),
        0x72 | 0x73 => Some(*sense.get(1)? & 0x0f),
        _ => None,
    }
}

/// Clé de sense `ILLEGAL REQUEST`.
pub const SENSE_ILLEGAL_REQUEST: u8 = 0x05;
/// ASC `INVALID COMMAND OPERATION CODE` : le SATL ne connaît pas l'opcode.
pub const ASC_INVALID_OPCODE: u8 = 0x20;
/// ASC `INVALID FIELD IN CDB` : le SATL connaît l'opcode mais refuse le CDB.
pub const ASC_INVALID_FIELD_IN_CDB: u8 = 0x24;

/// Clé de sense, ASC et ASCQ d'un sense fixe (70h/71h) ou descripteur (72h/73h).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SenseCode {
    /// Clé de sense (4 bits).
    pub key: u8,
    /// Additional Sense Code.
    pub asc: u8,
    /// Additional Sense Code Qualifier.
    pub ascq: u8,
}

impl SenseCode {
    /// Le SATL a rejeté le CDB lui-même (opcode inconnu ou forme refusée),
    /// seul cas où essayer ATA PASS-THROUGH (12) a un sens. Les autres
    /// `ILLEGAL REQUEST` (LBA hors limites, paramètre invalide…) et toutes
    /// les autres clés désignent une commande comprise puis refusée.
    pub fn rejects_cdb(self) -> bool {
        self.key == SENSE_ILLEGAL_REQUEST
            && matches!(self.asc, ASC_INVALID_OPCODE | ASC_INVALID_FIELD_IN_CDB)
    }
}

/// Clé, ASC et ASCQ du sense, quel que soit son format.
pub fn sense_code(sense: &[u8]) -> Option<SenseCode> {
    let response = *sense.first()? & 0x7f;
    match response {
        0x70 | 0x71 => Some(SenseCode {
            key: *sense.get(2)? & 0x0f,
            asc: *sense.get(12)?,
            ascq: *sense.get(13)?,
        }),
        0x72 | 0x73 => Some(SenseCode {
            key: *sense.get(1)? & 0x0f,
            asc: *sense.get(2)?,
            ascq: *sense.get(3)?,
        }),
        _ => None,
    }
}

/// Registres ATA extraits du sense : descripteur « ATA Status Return »
/// (code 09h) en format descripteur (72h/73h), ou champs INFORMATION et
/// COMMAND-SPECIFIC INFORMATION en format fixe (70h/71h) avec ASC 00h /
/// ASCQ 1Dh « ATA PASS THROUGH INFORMATION AVAILABLE », comme le font
/// certains ponts USB (SAT-4 §12.2.2.7).
pub fn registers_from_sense(sense: &[u8]) -> Option<Registers> {
    let response = *sense.first()? & 0x7f;
    match response {
        0x72 | 0x73 => descriptor_registers(sense),
        0x70 | 0x71 => fixed_registers(sense),
        _ => None,
    }
}

fn descriptor_registers(sense: &[u8]) -> Option<Registers> {
    let total = 8 + usize::from(*sense.get(7)?);
    let mut offset = 8;
    while offset + 2 <= total.min(sense.len()) {
        let code = sense[offset];
        let length = usize::from(sense[offset + 1]);
        if code == 0x09 && length >= 0x0c {
            let d = sense.get(offset..offset + 14)?;
            let byte = |i: usize| u64::from(d[i]);
            let extend = d[2] & 0x01 != 0;
            let lba = byte(7)
                | byte(9) << 8
                | byte(11) << 16
                | if extend {
                    byte(6) << 24 | byte(8) << 32 | byte(10) << 40
                } else {
                    0
                };
            return Some(Registers {
                error: d[3],
                count: u16::from(d[5]) | if extend { u16::from(d[4]) << 8 } else { 0 },
                lba,
                device: d[12],
                status: d[13],
            });
        }
        offset += 2 + length;
    }
    None
}

fn fixed_registers(sense: &[u8]) -> Option<Registers> {
    // ASC 00h, ASCQ 1Dh : les registres sont dans INFORMATION (3..7) et
    // COMMAND-SPECIFIC INFORMATION (8..12).
    if sense.len() < 18 || sense[12] != 0x00 || sense[13] != 0x1d {
        return None;
    }
    // Octet 8 (SAT) : bit 7 EXTEND, bit 6 COUNT UPPER NON-ZERO, bit 5 LBA
    // UPPER NON-ZERO, bits 2:0 LOG INDEX. Les octets hauts eux-mêmes ne
    // sont pas transportés : si le pont signale qu'ils sont non nuls, les
    // registres ne sont pas représentables et on ne les invente pas.
    let extend = sense[8] & 0x80 != 0;
    if extend && sense[8] & 0x60 != 0 {
        return None;
    }
    let count = u16::from(sense[6]);
    let lba = u64::from(sense[9]) | u64::from(sense[10]) << 8 | u64::from(sense[11]) << 16;
    Some(Registers {
        error: sense[3],
        status: sense[4],
        device: sense[5],
        count,
        lba,
    })
}

/// Opérations SANITIZE DEVICE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SanitizeOperation {
    /// BLOCK ERASE EXT.
    BlockErase,
    /// CRYPTO SCRAMBLE EXT.
    CryptoScramble,
    /// OVERWRITE EXT : une passe de zéros, sans inversion.
    Overwrite,
}

/// État du moteur de sanitize (SANITIZE STATUS EXT).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SanitizeState {
    /// Aucune opération en cours ; la dernière, s'il y en a eu une, a réussi.
    Idle,
    /// Opération en cours ; `progress` sur 65 536.
    InProgress,
    /// La dernière opération a échoué (mode Failure, à sortir avec `clear_failure`).
    Failed,
    /// La dernière opération s'est achevée sans erreur (bit 15 de COUNT),
    /// distinct d'un disque qui n'a jamais rien lancé.
    Completed,
}

/// Motif d'abandon d'une commande SANITIZE DEVICE, renvoyé par le disque
/// dans LBA 7:0 quand ABRT est posé (ACS-4 §7.45.1.3, « Sanitize Device
/// Error Reason »).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SanitizeRefusal {
    /// 00h : le disque ne rapporte pas de motif.
    NotReported,
    /// 01h : la dernière opération sanitize a échoué (état Failure).
    Unsuccessful,
    /// 02h : sous-commande invalide ou non supportée par ce disque.
    Unsupported,
    /// 03h : Sanitize Freeze Lock actif (SD3), aucune opération acceptée
    /// jusqu'au prochain cycle d'alimentation.
    Frozen,
    /// 04h : Antifreeze Lock actif, SANITIZE FREEZE LOCK EXT refusé.
    AntifreezeLocked,
    /// Valeur hors norme.
    Unknown(u8),
}

impl SanitizeRefusal {
    /// Décode le registre LBA renvoyé avec l'abandon.
    pub fn from_lba(lba: u64) -> Self {
        match (lba & 0xff) as u8 {
            0x00 => Self::NotReported,
            0x01 => Self::Unsuccessful,
            0x02 => Self::Unsupported,
            0x03 => Self::Frozen,
            0x04 => Self::AntifreezeLocked,
            other => Self::Unknown(other),
        }
    }
}

impl std::fmt::Display for SanitizeRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotReported => f.write_str("motif non rapporté par le disque"),
            Self::Unsuccessful => {
                f.write_str("la dernière opération sanitize a échoué (état Failure)")
            }
            Self::Unsupported => f.write_str("sous-commande sanitize non supportée"),
            Self::Frozen => f.write_str(
                "disque gelé (Sanitize Freeze Lock) : cycle d'alimentation ou dégel requis",
            ),
            Self::AntifreezeLocked => f.write_str("Antifreeze Lock actif"),
            Self::Unknown(code) => write!(f, "motif inconnu 0x{code:02x}"),
        }
    }
}

/// Résultat de SANITIZE STATUS EXT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SanitizeStatus {
    /// État.
    pub state: SanitizeState,
    /// Sanitize Freeze Lock actif : aucune commande sanitize acceptée.
    pub frozen: bool,
    /// Antifreeze Lock actif.
    pub antifreeze: bool,
    /// Progression (LBA 15:0), significative en cours d'opération.
    pub progress: u16,
}

impl SanitizeStatus {
    /// Décode les registres de retour de SANITIZE STATUS EXT (ACS-4 §7.45.3).
    pub fn from_registers(count: u16, lba: u64) -> Self {
        let state = if count & (1 << 14) != 0 {
            SanitizeState::InProgress
        } else if count & (1 << 15) != 0 {
            SanitizeState::Completed
        } else {
            SanitizeState::Idle
        };
        Self {
            state,
            frozen: count & (1 << 13) != 0,
            antifreeze: count & (1 << 12) != 0,
            progress: lba as u16,
        }
    }

    /// État déduit d'un SANITIZE STATUS EXT abandonné par le disque, quand
    /// le motif suffit : état Failure, disque gelé ou Antifreeze Lock. `None`
    /// pour les motifs qui ne décrivent pas l'état du moteur (sous-commande
    /// non supportée, valeur inconnue) : l'erreur doit alors remonter.
    pub fn from_refusal(reason: SanitizeRefusal) -> Option<Self> {
        let idle = Self {
            state: SanitizeState::Idle,
            frozen: false,
            antifreeze: false,
            progress: 0,
        };
        match reason {
            // Sans motif, l'abandon de STATUS EXT est le comportement
            // normatif de l'état Failure (ACS-4 §7.45.3.4).
            SanitizeRefusal::Unsuccessful | SanitizeRefusal::NotReported => Some(Self {
                state: SanitizeState::Failed,
                ..idle
            }),
            SanitizeRefusal::Frozen => Some(Self {
                frozen: true,
                ..idle
            }),
            SanitizeRefusal::AntifreezeLocked => Some(Self {
                antifreeze: true,
                ..idle
            }),
            SanitizeRefusal::Unsupported | SanitizeRefusal::Unknown(_) => None,
        }
    }

    /// Pourcentage approximatif de progression.
    pub fn percent(&self) -> u8 {
        (u32::from(self.progress) * 100 / 65_536) as u8
    }
}

// --- Constructeurs de commandes (purs, testés). ---

/// IDENTIFY DEVICE : un secteur en entrée.
pub fn identify_device() -> AtaCommand {
    AtaCommand {
        command: CMD_IDENTIFY_DEVICE,
        features: 0,
        count: 1,
        lba: 0,
        device: 0,
        extend: false,
        transfer: Transfer::In(1),
        timeout_ms: SHORT_TIMEOUT_MS,
    }
}

/// SMART READ DATA : un secteur en entrée.
pub fn smart_read_data() -> AtaCommand {
    AtaCommand {
        command: CMD_SMART,
        features: SMART_READ_DATA,
        count: 1,
        lba: SMART_SIGNATURE_LBA,
        device: 0,
        extend: false,
        transfer: Transfer::In(1),
        timeout_ms: SHORT_TIMEOUT_MS,
    }
}

/// SMART READ THRESHOLDS : un secteur en entrée (seuils par attribut).
pub fn smart_read_thresholds() -> AtaCommand {
    AtaCommand {
        command: CMD_SMART,
        features: SMART_READ_THRESHOLDS,
        count: 1,
        lba: SMART_SIGNATURE_LBA,
        device: 0,
        extend: false,
        transfer: Transfer::In(1),
        timeout_ms: SHORT_TIMEOUT_MS,
    }
}

/// SMART RETURN STATUS : le verdict revient dans LBA Mid / LBA High.
pub fn smart_return_status() -> AtaCommand {
    AtaCommand {
        command: CMD_SMART,
        features: SMART_RETURN_STATUS,
        count: 0,
        lba: SMART_SIGNATURE_LBA,
        device: 0,
        extend: false,
        transfer: Transfer::None,
        timeout_ms: SHORT_TIMEOUT_MS,
    }
}

fn sanitize_device(features: u16, count: u16, lba: u64, timeout_ms: u32) -> AtaCommand {
    AtaCommand {
        command: CMD_SANITIZE_DEVICE,
        features,
        count,
        lba,
        device: 0x40,
        extend: true,
        transfer: Transfer::None,
        timeout_ms,
    }
}

/// SANITIZE STATUS EXT ; `clear_failure` pose CLEAR SANITIZE OPERATION FAILED.
pub fn sanitize_status_ext(clear_failure: bool) -> AtaCommand {
    sanitize_device(
        SANITIZE_STATUS_EXT,
        u16::from(clear_failure),
        0,
        SHORT_TIMEOUT_MS,
    )
}

/// Démarrage d'une opération sanitize. FAILURE MODE est laissé à 0 : un
/// échec met le disque en mode Failure, visible et rapporté.
pub fn sanitize(operation: SanitizeOperation) -> AtaCommand {
    match operation {
        SanitizeOperation::BlockErase => sanitize_device(
            SANITIZE_BLOCK_ERASE_EXT,
            0,
            SIG_BLOCK_ERASE,
            SANITIZE_START_TIMEOUT_MS,
        ),
        SanitizeOperation::CryptoScramble => sanitize_device(
            SANITIZE_CRYPTO_SCRAMBLE_EXT,
            0,
            SIG_CRYPTO,
            SANITIZE_START_TIMEOUT_MS,
        ),
        // OVERWRITE COUNT = 1 (COUNT 3:0), motif 00000000h dans LBA 31:0.
        SanitizeOperation::Overwrite => sanitize_device(
            SANITIZE_OVERWRITE_EXT,
            1,
            SIG_OVERWRITE,
            SANITIZE_START_TIMEOUT_MS,
        ),
    }
}

/// SANITIZE FREEZE LOCK EXT.
pub fn sanitize_freeze_lock() -> AtaCommand {
    sanitize_device(
        SANITIZE_FREEZE_LOCK_EXT,
        0,
        SIG_FREEZE_LOCK,
        SHORT_TIMEOUT_MS,
    )
}

/// SANITIZE ANTIFREEZE LOCK EXT.
pub fn sanitize_antifreeze_lock() -> AtaCommand {
    sanitize_device(
        SANITIZE_ANTIFREEZE_LOCK_EXT,
        0,
        SIG_ANTIFREEZE,
        SHORT_TIMEOUT_MS,
    )
}

/// Commande non-data générique (SECURITY ERASE PREPARE, FREEZE LOCK).
fn non_data(command: u8) -> AtaCommand {
    AtaCommand {
        command,
        features: 0,
        count: 0,
        lba: 0,
        device: 0,
        extend: false,
        transfer: Transfer::None,
        timeout_ms: SHORT_TIMEOUT_MS,
    }
}

/// Commande Security data-out (un secteur contenant le mot de passe).
fn security_out(command: u8, timeout_ms: u32) -> AtaCommand {
    AtaCommand {
        command,
        features: 0,
        count: 1,
        lba: 0,
        device: 0,
        extend: false,
        transfer: Transfer::Out,
        timeout_ms,
    }
}

/// SECURITY ERASE PREPARE : doit précéder immédiatement SECURITY ERASE UNIT.
pub fn security_erase_prepare() -> AtaCommand {
    non_data(CMD_SECURITY_ERASE_PREPARE)
}

/// Résultat d'une commande passée au disque.
#[derive(Debug)]
pub struct Completion {
    /// Données reçues (vide pour non-data et data-out).
    pub data: AlignedBuffer,
    /// Registres renvoyés, si le SATL les a transmis.
    pub registers: Option<Registers>,
}

/// Choix du CDB ATA PASS-THROUGH selon ce qui traduit les commandes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdbPolicy {
    /// Bus natif (libata) : ATA PASS-THROUGH (16) seulement, sans repli.
    PassThrough16Only,
    /// Derrière un SATL (pont USB, HBA SAS) : 16 d'abord, repli sur 12 si le
    /// SATL rejette le CDB de 16 octets.
    AutoDetect,
}

impl CdbPolicy {
    /// Politique adaptée au transport sysfs du disque.
    pub fn for_transport(transport: Transport) -> Self {
        if transport.behind_satl() {
            Self::AutoDetect
        } else {
            Self::PassThrough16Only
        }
    }
}

/// Un disque ATA ouvert (`/dev/sdX`), natif, derrière un HBA SAS ou un pont USB.
pub struct AtaDevice<'a> {
    file: &'a File,
    /// Variante de CDB retenue ; `None` tant que le 16 n'a pas été confirmé
    /// ou refusé par le périphérique (politique [`CdbPolicy::AutoDetect`]).
    cdb: Cell<Option<Cdb>>,
}

impl<'a> AtaDevice<'a> {
    /// Le disque derrière `file`, avec la politique de CDB donnée.
    pub fn new(file: &'a File, policy: CdbPolicy) -> Self {
        Self {
            file,
            cdb: Cell::new(match policy {
                CdbPolicy::PassThrough16Only => Some(Cdb::PassThrough16),
                CdbPolicy::AutoDetect => None,
            }),
        }
    }

    /// Le disque derrière `file`, politique déduite de son transport sysfs.
    pub fn for_transport(file: &'a File, transport: Transport) -> Self {
        Self::new(file, CdbPolicy::for_transport(transport))
    }

    /// Variante de CDB retenue, `None` tant qu'aucune commande n'a tranché.
    pub fn cdb_in_use(&self) -> Option<Cdb> {
        self.cdb.get()
    }

    /// Exécute `command` ; `out` est le secteur d'un data-out (mot de passe).
    /// Les copies alignées d'un data-out sont effacées après l'appel.
    pub fn run(&self, command: &AtaCommand, out: Option<&[u8; SECTOR]>) -> Result<Completion> {
        let variant = self.cdb.get().unwrap_or(Cdb::PassThrough16);
        match self.run_with(command, out, variant) {
            Ok(completion) => {
                if self.cdb.get().is_none() {
                    self.cdb.set(Some(variant));
                }
                Ok(completion)
            }
            // SATL ne connaissant pas le CDB de 16 octets : une seule
            // bascule, uniquement en détection automatique (jamais sur bus
            // natif), uniquement sur un rejet du CDB lui-même, uniquement
            // pour une commande exprimable en 12.
            Err(DeviceError::Scsi {
                sense: Some(code), ..
            }) if self.cdb.get().is_none()
                && variant == Cdb::PassThrough16
                && code.rejects_cdb()
                && cdb12(command).is_ok() =>
            {
                let completion = self.run_with(command, out, Cdb::PassThrough12)?;
                self.cdb.set(Some(Cdb::PassThrough12));
                Ok(completion)
            }
            Err(error) => Err(error),
        }
    }

    fn run_with(
        &self,
        command: &AtaCommand,
        out: Option<&[u8; SECTOR]>,
        variant: Cdb,
    ) -> Result<Completion> {
        let mut cdb_bytes = [0u8; 16];
        let cmd_len = match variant {
            Cdb::PassThrough16 => {
                cdb_bytes = cdb16(command);
                16u8
            }
            Cdb::PassThrough12 => {
                cdb_bytes[..12].copy_from_slice(&cdb12(command)?);
                12u8
            }
        };
        let mut sense = [0u8; 64];
        let bytes = command.sectors() * SECTOR;
        let mut data = AlignedBuffer::new(bytes);
        if let Some(out) = out {
            if bytes >= SECTOR {
                data[..SECTOR].copy_from_slice(out);
            }
        }
        let mut hdr = SgIoHdr::zeroed();
        hdr.dxfer_direction = match command.transfer {
            Transfer::None => sys::SG_DXFER_NONE,
            Transfer::In(_) => sys::SG_DXFER_FROM_DEV,
            Transfer::Out => sys::SG_DXFER_TO_DEV,
        };
        hdr.cmd_len = cmd_len;
        hdr.mx_sb_len = sense.len() as u8;
        hdr.dxfer_len = bytes as u32;
        hdr.dxferp = if bytes == 0 {
            std::ptr::null_mut()
        } else {
            data.as_mut_ptr()
        };
        hdr.cmdp = cdb_bytes.as_ptr();
        hdr.sbp = sense.as_mut_ptr();
        hdr.timeout = command.timeout_ms;

        // `cdb_bytes`, `sense` et `data` vivent jusqu'au retour et ont
        // exactement les longueurs annoncées : contrat de `sys::sg_io`.
        let sent = sys::sg_io(self.file.as_fd(), &mut hdr);
        if out.is_some() {
            data.wipe();
        }
        sent.map_err(|e| DeviceError::io(format!("SG_IO (ATA 0x{:02x})", command.command), e))?;

        let sense = &sense[..usize::from(hdr.sb_len_wr).min(sense.len())];
        let registers = registers_from_sense(sense);

        if hdr.host_status != 0 || hdr.driver_status & !sys::DRIVER_SENSE != 0 {
            return Err(scsi_error(&hdr, sense));
        }
        if let Some(registers) = registers {
            if registers.failed() {
                return Err(DeviceError::AtaAborted {
                    command: command.command,
                    error: registers.error,
                    status: registers.status,
                    lba: registers.lba,
                });
            }
        }
        // CHECK CONDITION sans registres ATA : échec au niveau SCSI (pont, SATL).
        if hdr.status != 0 && registers.is_none() {
            return Err(scsi_error(&hdr, sense));
        }
        if hdr.resid != 0 {
            return Err(DeviceError::IncompleteTransfer(hdr.resid));
        }
        Ok(Completion { data, registers })
    }

    /// IDENTIFY DEVICE, décodé.
    pub fn identify(&self) -> Result<IdentifyDevice> {
        let completion = self.run(&identify_device(), None)?;
        Ok(identify::decode(&completion.data)?)
    }

    /// SMART RETURN STATUS.
    pub fn smart_status(&self) -> Result<SmartStatus> {
        let completion = self.run(&smart_return_status(), None)?;
        let registers = completion.registers.ok_or(DeviceError::Unsupported(
            "SMART RETURN STATUS sans registres (SATL muet)",
        ))?;
        let (mid, high) = registers.lba_mid_high();
        Ok(smart::status_from_registers(mid, high)?)
    }

    /// SMART READ DATA, attributs décodés.
    pub fn smart_attributes(&self) -> Result<SmartAttributes> {
        let completion = self.run(&smart_read_data(), None)?;
        Ok(smart::decode_attributes(&completion.data)?)
    }

    /// Table complète des attributs SMART (toutes les entrées présentes),
    /// avec les seuils de SMART READ THRESHOLDS quand le disque les fournit.
    pub fn smart_attribute_table(&self) -> Result<Vec<smart::RawAttribute>> {
        let data = self.run(&smart_read_data(), None)?;
        let mut table = smart::decode_attribute_table(&data.data)?;
        if let Ok(thresholds) = self.run(&smart_read_thresholds(), None) {
            if let Ok(map) = smart::decode_thresholds(&thresholds.data) {
                for attribute in &mut table {
                    attribute.threshold = map.get(&attribute.id).copied();
                }
            }
        }
        Ok(table)
    }

    /// Exécute une sous-commande SANITIZE DEVICE ; un abandon est traduit
    /// avec le motif que le disque renvoie dans LBA 7:0.
    fn run_sanitize(&self, command: &AtaCommand) -> Result<Completion> {
        self.run(command, None).map_err(|error| match error {
            DeviceError::AtaAborted { lba, .. } => {
                DeviceError::SanitizeRefused(SanitizeRefusal::from_lba(lba))
            }
            other => other,
        })
    }

    /// SANITIZE STATUS EXT. Un abandon dont le motif décrit l'état du
    /// moteur (Failure, gelé, Antifreeze Lock) est rendu comme un état ;
    /// les autres motifs remontent en [`DeviceError::SanitizeRefused`].
    pub fn sanitize_status(&self) -> Result<SanitizeStatus> {
        match self.run_sanitize(&sanitize_status_ext(false)) {
            Ok(completion) => {
                let registers = completion.registers.ok_or(DeviceError::Unsupported(
                    "SANITIZE STATUS EXT sans registres",
                ))?;
                Ok(SanitizeStatus::from_registers(
                    registers.count,
                    registers.lba,
                ))
            }
            Err(DeviceError::SanitizeRefused(reason)) => {
                SanitizeStatus::from_refusal(reason).ok_or(DeviceError::SanitizeRefused(reason))
            }
            Err(error) => Err(error),
        }
    }

    /// Sort du mode Failure (CLEAR SANITIZE OPERATION FAILED).
    pub fn sanitize_clear_failure(&self) -> Result<()> {
        self.run_sanitize(&sanitize_status_ext(true)).map(|_| ())
    }

    /// Démarre une opération sanitize ; elle continue en arrière-plan, à
    /// suivre par [`Self::sanitize_status`].
    pub fn start_sanitize(&self, operation: SanitizeOperation) -> Result<()> {
        self.run_sanitize(&sanitize(operation)).map(|_| ())
    }

    fn security(&self, command: AtaCommand, control: u16, password: &Password) -> Result<()> {
        let mut block = zeroize::Zeroizing::new([0u8; SECTOR]);
        security::fill_password_block(&mut block, control, password);
        self.run(&command, Some(&block)).map(|_| ())
    }

    /// SECURITY SET PASSWORD (mot de passe utilisateur, niveau High : le mot
    /// de passe maître constructeur reste utilisable).
    pub fn security_set_user_password(&self, password: &Password) -> Result<()> {
        self.security(
            security_out(CMD_SECURITY_SET_PASSWORD, SHORT_TIMEOUT_MS),
            0,
            password,
        )
    }

    /// SECURITY UNLOCK.
    pub fn security_unlock(
        &self,
        identifier: security::Identifier,
        password: &Password,
    ) -> Result<()> {
        self.security(
            security_out(CMD_SECURITY_UNLOCK, SHORT_TIMEOUT_MS),
            identifier.control_bits(),
            password,
        )
    }

    /// SECURITY DISABLE PASSWORD.
    pub fn security_disable_password(
        &self,
        identifier: security::Identifier,
        password: &Password,
    ) -> Result<()> {
        self.security(
            security_out(CMD_SECURITY_DISABLE_PASSWORD, SHORT_TIMEOUT_MS),
            identifier.control_bits(),
            password,
        )
    }

    /// SECURITY ERASE PREPARE puis, immédiatement, SECURITY ERASE UNIT.
    /// Bloque jusqu'à la fin de l'effacement (`timeout_ms` : voir
    /// [`security::erase_timeout_ms`]).
    pub fn security_erase_unit(
        &self,
        password: &Password,
        enhanced: bool,
        timeout_ms: u32,
    ) -> Result<()> {
        self.run(&security_erase_prepare(), None)?;
        let control = if enhanced {
            security::ENHANCED_ERASE
        } else {
            0
        };
        self.security(
            security_out(CMD_SECURITY_ERASE_UNIT, timeout_ms),
            control,
            password,
        )
    }
}

fn scsi_error(hdr: &SgIoHdr, sense: &[u8]) -> DeviceError {
    let code = sense_code(sense);
    let detail = match code {
        Some(code) => format!(
            ", sense key 0x{:x} ({}), ASC 0x{:02x}/0x{:02x}",
            code.key,
            sense_key_name(code.key),
            code.asc,
            code.ascq
        ),
        None => String::new(),
    };
    DeviceError::Scsi {
        status: hdr.status,
        host: hdr.host_status,
        driver: hdr.driver_status,
        sense: code,
        detail,
    }
}

fn sense_key_name(key: u8) -> &'static str {
    match key {
        0x0 => "NO SENSE",
        0x1 => "RECOVERED ERROR",
        0x2 => "NOT READY",
        0x3 => "MEDIUM ERROR",
        0x4 => "HARDWARE ERROR",
        0x5 => "ILLEGAL REQUEST",
        0x6 => "UNIT ATTENTION",
        0x7 => "DATA PROTECT",
        0xb => "ABORTED COMMAND",
        _ => "autre",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn identify_device_cdb16() {
        assert_eq!(
            cdb16(&identify_device()),
            [0x85, 0x08, 0x0e, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0xec, 0]
        );
    }

    #[test]
    fn identify_device_cdb12_for_usb_bridges() {
        let cdb = cdb12(&identify_device()).expect("28 bits");
        assert_eq!(cdb, [0xa1, 0x08, 0x0e, 0, 1, 0, 0, 0, 0, 0xec, 0, 0]);
        // 48 bits : refusé en 12.
        assert!(matches!(
            cdb12(&sanitize_status_ext(false)),
            Err(DeviceError::NotRepresentable12(0xb4))
        ));
        // LBA > 28 bits : refusé aussi.
        let mut big = identify_device();
        big.lba = 1 << 28;
        assert!(cdb12(&big).is_err());
    }

    #[test]
    fn smart_cdbs_carry_the_signature() {
        let read = cdb16(&smart_read_data());
        assert_eq!(read[4], 0xd0);
        assert_eq!((read[10], read[12]), (0x4f, 0xc2), "LBA mid, LBA high");
        assert_eq!(read[14], 0xb0);
        let status = cdb16(&smart_return_status());
        assert_eq!((status[1], status[2]), (0x06, 0x20), "non-data, CK_COND");
        assert_eq!(status[4], 0xda);
        // En 12 octets, la signature doit se retrouver aux bons offsets.
        let read12 = cdb12(&smart_read_data()).expect("28 bits");
        assert_eq!((read12[6], read12[7]), (0x4f, 0xc2));
    }

    #[test]
    fn sanitize_cdbs_carry_their_signatures() {
        let block = cdb16(&sanitize(SanitizeOperation::BlockErase));
        assert_eq!(block[1], 0x07, "non-data, EXTEND");
        assert_eq!(block[4], 0x12);
        assert_eq!([block[7], block[12], block[10], block[8]], *b"BkEr");
        assert_eq!(block[14], 0xb4);

        let crypto = cdb16(&sanitize(SanitizeOperation::CryptoScramble));
        assert_eq!(crypto[4], 0x11);
        assert_eq!([crypto[7], crypto[12], crypto[10], crypto[8]], *b"Cryp");

        let overwrite = cdb16(&sanitize(SanitizeOperation::Overwrite));
        assert_eq!(overwrite[4], 0x14);
        assert_eq!([overwrite[11], overwrite[9]], *b"OW", "LBA 47:32");
        assert_eq!(
            [overwrite[7], overwrite[12], overwrite[10], overwrite[8]],
            [0; 4]
        );
        assert_eq!(overwrite[6], 1, "une passe");

        let status = cdb16(&sanitize_status_ext(true));
        assert_eq!((status[4], status[6]), (0x00, 0x01));
        assert_eq!(cdb16(&sanitize_freeze_lock())[4], 0x20);
        assert_eq!(cdb16(&sanitize_antifreeze_lock())[4], 0x40);
    }

    fn descriptor_sense(descriptor: [u8; 14]) -> Vec<u8> {
        let mut sense = vec![0x72, 0x01, 0x00, 0x1d, 0, 0, 0, 14];
        sense.extend_from_slice(&descriptor);
        sense
    }

    #[test]
    fn reads_registers_from_descriptor_sense() {
        let sense = descriptor_sense([
            0x09, 0x0c, 0x00, 0x00, 0, 0, 0, 0, 0, 0x4f, 0, 0xc2, 0, 0x50,
        ]);
        let registers = registers_from_sense(&sense).expect("descripteur");
        assert_eq!(registers.lba_mid_high(), (0x4f, 0xc2));
        assert!(!registers.failed());
        assert_eq!(sense_key(&sense), Some(1));

        // EXTEND : count 4000h (en cours), progression 8000h.
        let sense = descriptor_sense([
            0x09, 0x0c, 0x01, 0x00, 0x40, 0x00, 0, 0, 0, 0x80, 0, 0, 0x40, 0x50,
        ]);
        let registers = registers_from_sense(&sense).expect("descripteur");
        assert_eq!(registers.count, 0x4000);
        assert_eq!(registers.lba as u16, 0x8000);
        let status = SanitizeStatus::from_registers(registers.count, registers.lba);
        assert_eq!(status.state, SanitizeState::InProgress);
        assert_eq!(status.percent(), 50);
    }

    #[test]
    fn reads_registers_from_fixed_sense_of_usb_bridges() {
        // Format fixe 70h, sense key RECOVERED ERROR, ASC 00h / ASCQ 1Dh.
        let mut sense = vec![0u8; 18];
        sense[0] = 0x70;
        sense[2] = 0x01;
        sense[7] = 10;
        sense[3] = 0x00; // error
        sense[4] = 0x50; // status
        sense[5] = 0x40; // device
        sense[6] = 0x01; // count 7:0
        sense[9] = 0x00; // lba 7:0
        sense[10] = 0x4f; // lba 15:8
        sense[11] = 0xc2; // lba 23:16
        sense[12] = 0x00;
        sense[13] = 0x1d;
        let registers = registers_from_sense(&sense).expect("format fixe");
        assert_eq!(registers.lba_mid_high(), (0x4f, 0xc2));
        assert_eq!(registers.count, 1);
        assert!(!registers.failed());
        // Sans ASC/ASCQ SAT : aucun registre.
        sense[13] = 0x00;
        assert_eq!(registers_from_sense(&sense), None);
        assert_eq!(sense_key(&sense), Some(1));
    }

    #[test]
    fn error_bit_marks_failure() {
        let aborted =
            descriptor_sense([0x09, 0x0c, 0x01, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0x40, 0x51]);
        assert!(registers_from_sense(&aborted)
            .expect("descripteur")
            .failed());
        assert_eq!(registers_from_sense(&[]), None);
        assert_eq!(sense_key(&[]), None);
    }

    #[test]
    fn sanitize_status_decoding() {
        assert_eq!(
            SanitizeStatus::from_registers(1 << 15, 0).state,
            SanitizeState::Completed
        );
        assert_eq!(
            SanitizeStatus::from_registers(1 << 14, 0).state,
            SanitizeState::InProgress
        );
        assert_eq!(
            SanitizeStatus::from_registers(0, 0).state,
            SanitizeState::Idle
        );
        assert!(SanitizeStatus::from_registers(1 << 13, 0).frozen);
        assert!(SanitizeStatus::from_registers(1 << 12, 0).antifreeze);
    }

    #[test]
    fn sanitize_refusal_carries_the_abort_reason() {
        assert_eq!(
            SanitizeRefusal::from_lba(0x00),
            SanitizeRefusal::NotReported
        );
        assert_eq!(
            SanitizeRefusal::from_lba(0x01),
            SanitizeRefusal::Unsuccessful
        );
        assert_eq!(
            SanitizeRefusal::from_lba(0x02),
            SanitizeRefusal::Unsupported
        );
        assert_eq!(SanitizeRefusal::from_lba(0x03), SanitizeRefusal::Frozen);
        assert_eq!(
            SanitizeRefusal::from_lba(0x04),
            SanitizeRefusal::AntifreezeLocked
        );
        // Seuls les 8 bits bas portent le motif.
        assert_eq!(
            SanitizeRefusal::from_lba(0xab_cd03),
            SanitizeRefusal::Frozen
        );
        assert_eq!(
            SanitizeRefusal::from_lba(0x7f),
            SanitizeRefusal::Unknown(0x7f)
        );

        let failed = SanitizeStatus::from_refusal(SanitizeRefusal::Unsuccessful).expect("état");
        assert_eq!(failed.state, SanitizeState::Failed);
        assert!(!failed.frozen);
        assert_eq!(
            SanitizeStatus::from_refusal(SanitizeRefusal::NotReported)
                .expect("état")
                .state,
            SanitizeState::Failed
        );
        let frozen = SanitizeStatus::from_refusal(SanitizeRefusal::Frozen).expect("état");
        assert!(frozen.frozen && !frozen.antifreeze);
        assert_eq!(frozen.state, SanitizeState::Idle);
        let locked = SanitizeStatus::from_refusal(SanitizeRefusal::AntifreezeLocked).expect("état");
        assert!(locked.antifreeze && !locked.frozen);
        assert_eq!(
            SanitizeStatus::from_refusal(SanitizeRefusal::Unsupported),
            None
        );
        assert_eq!(
            SanitizeStatus::from_refusal(SanitizeRefusal::Unknown(9)),
            None
        );
        assert!(SanitizeRefusal::Frozen.to_string().contains("gelé"));
    }

    #[test]
    fn sense_code_is_read_in_both_formats_and_gates_the_cdb_fallback() {
        // Format fixe : clé en 2, ASC en 12, ASCQ en 13.
        let mut fixed = vec![0u8; 18];
        fixed[0] = 0x70;
        fixed[2] = SENSE_ILLEGAL_REQUEST;
        fixed[12] = ASC_INVALID_OPCODE;
        let code = sense_code(&fixed).expect("fixe");
        assert_eq!((code.key, code.asc, code.ascq), (0x5, 0x20, 0x00));
        assert!(code.rejects_cdb());

        // Format descripteur : clé en 1, ASC en 2, ASCQ en 3.
        let descriptor = [
            0x72,
            SENSE_ILLEGAL_REQUEST,
            ASC_INVALID_FIELD_IN_CDB,
            0x00,
            0,
            0,
            0,
            0,
        ];
        let code = sense_code(&descriptor).expect("descripteur");
        assert_eq!((code.key, code.asc), (0x5, 0x24));
        assert!(code.rejects_cdb());

        // ILLEGAL REQUEST pour un autre motif (LBA hors limites) : commande
        // comprise puis refusée, pas de repli.
        fixed[12] = 0x21;
        assert!(!sense_code(&fixed).expect("fixe").rejects_cdb());
        // Même ASC sous une autre clé : pas un rejet de CDB.
        fixed[2] = 0x0b;
        fixed[12] = ASC_INVALID_OPCODE;
        assert!(!sense_code(&fixed).expect("fixe").rejects_cdb());
        // Format inconnu, sense tronqué.
        assert_eq!(sense_code(&[0x7f]), None);
        assert_eq!(sense_code(&fixed[..10]), None);
    }

    #[test]
    fn cdb_policy_follows_the_transport_and_pins_native_buses_to_cdb16() {
        assert_eq!(
            CdbPolicy::for_transport(Transport::Sata),
            CdbPolicy::PassThrough16Only
        );
        assert_eq!(
            CdbPolicy::for_transport(Transport::Virtio),
            CdbPolicy::PassThrough16Only
        );
        assert_eq!(
            CdbPolicy::for_transport(Transport::Usb),
            CdbPolicy::AutoDetect
        );
        assert_eq!(
            CdbPolicy::for_transport(Transport::Sas),
            CdbPolicy::AutoDetect
        );

        let file = File::open("/dev/null").expect("/dev/null");
        let native = AtaDevice::for_transport(&file, Transport::Sata);
        assert_eq!(native.cdb_in_use(), Some(Cdb::PassThrough16));
        let bridged = AtaDevice::for_transport(&file, Transport::Usb);
        assert_eq!(bridged.cdb_in_use(), None);
    }
}
