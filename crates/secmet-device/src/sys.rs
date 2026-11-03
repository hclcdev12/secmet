//! Interface noyau brute : structures UAPI et ioctl, et rien d'autre.
//!
//! Chaque `unsafe` de la crate qui parle au noyau est ici, réduit à l'appel
//! système lui-même. Les layouts reproduisent `scsi/sg.h`,
//! `linux/nvme_ioctl.h` et `linux/fs.h` sur x86_64 ; les tests en bas de
//! fichier les épinglent (taille, offsets, numéros d'ioctl).

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd};

/// `struct nvme_passthru_cmd` (alias `nvme_admin_cmd`), 72 octets.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct NvmePassthruCmd {
    /// Opcode admin.
    pub opcode: u8,
    /// Flags (FUSE, PSDT) — 0.
    pub flags: u8,
    /// Réservé.
    pub rsvd1: u16,
    /// Namespace ID.
    pub nsid: u32,
    /// CDW2.
    pub cdw2: u32,
    /// CDW3.
    pub cdw3: u32,
    /// Adresse du tampon de métadonnées (0).
    pub metadata: u64,
    /// Adresse du tampon de données (0 si aucune).
    pub addr: u64,
    /// Longueur des métadonnées.
    pub metadata_len: u32,
    /// Longueur des données.
    pub data_len: u32,
    /// CDW10.
    pub cdw10: u32,
    /// CDW11.
    pub cdw11: u32,
    /// CDW12.
    pub cdw12: u32,
    /// CDW13.
    pub cdw13: u32,
    /// CDW14.
    pub cdw14: u32,
    /// CDW15.
    pub cdw15: u32,
    /// Délai en millisecondes (0 = défaut du pilote).
    pub timeout_ms: u32,
    /// DW0 de la completion, rempli par le noyau.
    pub result: u32,
}

/// `struct sg_io_hdr`, 88 octets sur Linux 64 bits.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SgIoHdr {
    /// Toujours `'S'`.
    pub interface_id: i32,
    /// `SG_DXFER_*`.
    pub dxfer_direction: i32,
    /// Longueur du CDB.
    pub cmd_len: u8,
    /// Taille max du tampon de sense.
    pub mx_sb_len: u8,
    /// Nombre d'iovec (0 : tampon plat).
    pub iovec_count: u16,
    /// Longueur du transfert de données.
    pub dxfer_len: u32,
    /// Tampon de données.
    pub dxferp: *mut u8,
    /// CDB.
    pub cmdp: *const u8,
    /// Tampon de sense.
    pub sbp: *mut u8,
    /// Délai en millisecondes.
    pub timeout: u32,
    /// Flags SG.
    pub flags: u32,
    /// Identifiant utilisateur.
    pub pack_id: i32,
    /// Pointeur utilisateur.
    pub usr_ptr: *mut u8,
    /// Statut SCSI.
    pub status: u8,
    /// Statut masqué.
    pub masked_status: u8,
    /// Statut message.
    pub msg_status: u8,
    /// Octets de sense écrits.
    pub sb_len_wr: u8,
    /// Statut hôte.
    pub host_status: u16,
    /// Statut pilote.
    pub driver_status: u16,
    /// Octets non transférés.
    pub resid: i32,
    /// Durée en millisecondes.
    pub duration: u32,
    /// Informations.
    pub info: u32,
}

impl SgIoHdr {
    /// En-tête vierge, prêt à être rempli (`interface_id` déjà posé).
    pub fn zeroed() -> Self {
        Self {
            interface_id: i32::from(b'S'),
            dxfer_direction: SG_DXFER_NONE,
            cmd_len: 0,
            mx_sb_len: 0,
            iovec_count: 0,
            dxfer_len: 0,
            dxferp: std::ptr::null_mut(),
            cmdp: std::ptr::null(),
            sbp: std::ptr::null_mut(),
            timeout: 0,
            flags: 0,
            pack_id: 0,
            usr_ptr: std::ptr::null_mut(),
            status: 0,
            masked_status: 0,
            msg_status: 0,
            sb_len_wr: 0,
            host_status: 0,
            driver_status: 0,
            resid: 0,
            duration: 0,
            info: 0,
        }
    }
}

/// `SG_IO`.
pub const SG_IO: libc::c_ulong = 0x2285;
/// `SG_DXFER_NONE`.
pub const SG_DXFER_NONE: i32 = -1;
/// `SG_DXFER_TO_DEV`.
pub const SG_DXFER_TO_DEV: i32 = -2;
/// `SG_DXFER_FROM_DEV`.
pub const SG_DXFER_FROM_DEV: i32 = -3;
/// Bit `DRIVER_SENSE` de `driver_status` : des données de sense sont présentes.
pub const DRIVER_SENSE: u16 = 0x08;
/// Statut SCSI `CHECK CONDITION`.
pub const SCSI_CHECK_CONDITION: u8 = 0x02;

/// `NVME_IOCTL_ID` = `_IO('N', 0x40)`.
pub const NVME_IOCTL_ID: libc::c_ulong = 0x4e40;
/// `NVME_IOCTL_ADMIN_CMD` = `_IOWR('N', 0x41, struct nvme_admin_cmd)`.
pub const NVME_IOCTL_ADMIN_CMD: libc::c_ulong = 0xc048_4e41;

/// `BLKGETSIZE64` = `_IOR(0x12, 114, size_t)` : taille en octets.
pub const BLKGETSIZE64: libc::c_ulong = 0x8008_1272;
/// `BLKSSZGET` : taille de bloc logique.
pub const BLKSSZGET: libc::c_ulong = 0x1268;
/// `BLKFLSBUF` : vide les tampons du noyau pour ce périphérique (`blockdev --flushbufs`).
pub const BLKFLSBUF: libc::c_ulong = 0x1261;
/// `BLKRRPART` : relit la table de partitions (`blockdev --rereadpt`).
pub const BLKRRPART: libc::c_ulong = 0x125f;
/// `BLKROGET` : lecture seule ?
pub const BLKROGET: libc::c_ulong = 0x125e;

fn check(result: libc::c_int) -> io::Result<libc::c_int> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

/// Envoie une commande SCSI (ici un ATA PASS-THROUGH) par `SG_IO`.
///
/// L'appelant garantit que `hdr.cmdp` pointe `hdr.cmd_len` octets, `hdr.sbp`
/// `hdr.mx_sb_len` octets, et `hdr.dxferp` `hdr.dxfer_len` octets (ou est nul
/// si `dxfer_len` vaut 0), tous vivants pendant l'appel. Le noyau remplit les
/// champs de sortie (statuts, `sb_len_wr`, `resid`).
pub fn sg_io(fd: BorrowedFd<'_>, hdr: &mut SgIoHdr) -> io::Result<()> {
    // SAFETY: `fd` est un descripteur ouvert emprunté pour la durée de l'appel ;
    // `hdr` est un `sg_io_hdr` au layout `repr(C)` identique à l'UAPI, emprunté
    // exclusivement ; la validité des pointeurs qu'il contient est garantie par
    // l'appelant (contrat documenté ci-dessus). Le noyau n'accède qu'aux zones
    // décrites par ces pointeurs et leurs longueurs.
    check(unsafe { libc::ioctl(fd.as_raw_fd(), SG_IO, hdr as *mut SgIoHdr) })?;
    Ok(())
}

/// Envoie une commande admin NVMe ; renvoie DW0 de la completion.
/// Un retour positif de l'ioctl est le statut NVMe d'une commande refusée.
///
/// L'appelant garantit que `cmd.addr` vaut 0 ou pointe un tampon vivant d'au
/// moins `cmd.data_len` octets pendant l'appel.
pub fn nvme_admin(fd: BorrowedFd<'_>, cmd: &mut NvmePassthruCmd) -> io::Result<NvmeOutcome> {
    // SAFETY: `fd` est ouvert ; `cmd` est un `nvme_admin_cmd` `repr(C)` emprunté
    // exclusivement, que le noyau lit puis met à jour (`result`) ; le tampon de
    // données référencé par `addr` est garanti vivant par l'appelant.
    let status = check(unsafe {
        libc::ioctl(
            fd.as_raw_fd(),
            NVME_IOCTL_ADMIN_CMD,
            cmd as *mut NvmePassthruCmd,
        )
    })?;
    if status != 0 {
        return Ok(NvmeOutcome::Status(status as u16));
    }
    Ok(NvmeOutcome::Completed(cmd.result))
}

/// Issue d'une commande admin NVMe acceptée par le noyau.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NvmeOutcome {
    /// Succès ; DW0 de la completion.
    Completed(u32),
    /// Le contrôleur a répondu par un statut d'erreur (SCT << 8 | SC).
    Status(u16),
}

/// Namespace ID d'un périphérique bloc `/dev/nvmeXnY`.
pub fn nvme_namespace_id(fd: BorrowedFd<'_>) -> io::Result<u32> {
    // SAFETY: `NVME_IOCTL_ID` ne prend aucun argument et renvoie seulement une valeur.
    let nsid = check(unsafe { libc::ioctl(fd.as_raw_fd(), NVME_IOCTL_ID) })?;
    u32::try_from(nsid).map_err(|_| io::Error::other("namespace id invalide"))
}

/// Taille d'un périphérique bloc, en octets.
pub fn block_device_size(fd: BorrowedFd<'_>) -> io::Result<u64> {
    let mut size: u64 = 0;
    // SAFETY: `BLKGETSIZE64` écrit un `u64` à l'adresse fournie, qui pointe une
    // variable locale vivante et correctement alignée.
    check(unsafe { libc::ioctl(fd.as_raw_fd(), BLKGETSIZE64, &mut size as *mut u64) })?;
    Ok(size)
}

/// Taille de bloc logique d'un périphérique bloc.
pub fn logical_block_size(fd: BorrowedFd<'_>) -> io::Result<u32> {
    let mut size: libc::c_int = 0;
    // SAFETY: `BLKSSZGET` écrit un `int` à l'adresse fournie, variable locale vivante.
    check(unsafe { libc::ioctl(fd.as_raw_fd(), BLKSSZGET, &mut size as *mut libc::c_int) })?;
    u32::try_from(size).map_err(|_| io::Error::other("taille de bloc invalide"))
}

/// Le périphérique est-il en lecture seule (`BLKROGET`) ?
pub fn block_device_read_only(fd: BorrowedFd<'_>) -> io::Result<bool> {
    let mut ro: libc::c_int = 0;
    // SAFETY: `BLKROGET` écrit un `int` à l'adresse fournie, variable locale vivante.
    check(unsafe { libc::ioctl(fd.as_raw_fd(), BLKROGET, &mut ro as *mut libc::c_int) })?;
    Ok(ro != 0)
}

/// Vide les tampons noyau du périphérique (`blockdev --flushbufs`).
pub fn flush_buffers(fd: BorrowedFd<'_>) -> io::Result<()> {
    // SAFETY: `BLKFLSBUF` ne prend pas d'argument pointeur.
    check(unsafe { libc::ioctl(fd.as_raw_fd(), BLKFLSBUF, 0 as libc::c_ulong) })?;
    Ok(())
}

/// Demande au noyau de relire la table de partitions (`blockdev --rereadpt`).
pub fn reread_partition_table(fd: BorrowedFd<'_>) -> io::Result<()> {
    // SAFETY: `BLKRRPART` ne prend pas d'argument pointeur.
    check(unsafe { libc::ioctl(fd.as_raw_fd(), BLKRRPART, 0 as libc::c_ulong) })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::mem::{offset_of, size_of};

    /// Valeurs issues des en-têtes UAPI du noyau (x86_64).
    #[test]
    fn nvme_passthru_cmd_matches_the_kernel() {
        assert_eq!(size_of::<NvmePassthruCmd>(), 72);
        assert_eq!(offset_of!(NvmePassthruCmd, opcode), 0);
        assert_eq!(offset_of!(NvmePassthruCmd, flags), 1);
        assert_eq!(offset_of!(NvmePassthruCmd, nsid), 4);
        assert_eq!(offset_of!(NvmePassthruCmd, metadata), 16);
        assert_eq!(offset_of!(NvmePassthruCmd, addr), 24);
        assert_eq!(offset_of!(NvmePassthruCmd, metadata_len), 32);
        assert_eq!(offset_of!(NvmePassthruCmd, data_len), 36);
        assert_eq!(offset_of!(NvmePassthruCmd, cdw10), 40);
        assert_eq!(offset_of!(NvmePassthruCmd, cdw15), 60);
        assert_eq!(offset_of!(NvmePassthruCmd, timeout_ms), 64);
        assert_eq!(offset_of!(NvmePassthruCmd, result), 68);
    }

    #[test]
    fn sg_io_hdr_matches_the_kernel() {
        assert_eq!(size_of::<SgIoHdr>(), 88);
        let expected = [
            (offset_of!(SgIoHdr, interface_id), 0),
            (offset_of!(SgIoHdr, dxfer_direction), 4),
            (offset_of!(SgIoHdr, cmd_len), 8),
            (offset_of!(SgIoHdr, mx_sb_len), 9),
            (offset_of!(SgIoHdr, iovec_count), 10),
            (offset_of!(SgIoHdr, dxfer_len), 12),
            (offset_of!(SgIoHdr, dxferp), 16),
            (offset_of!(SgIoHdr, cmdp), 24),
            (offset_of!(SgIoHdr, sbp), 32),
            (offset_of!(SgIoHdr, timeout), 40),
            (offset_of!(SgIoHdr, flags), 44),
            (offset_of!(SgIoHdr, pack_id), 48),
            (offset_of!(SgIoHdr, usr_ptr), 56),
            (offset_of!(SgIoHdr, status), 64),
            (offset_of!(SgIoHdr, masked_status), 65),
            (offset_of!(SgIoHdr, msg_status), 66),
            (offset_of!(SgIoHdr, sb_len_wr), 67),
            (offset_of!(SgIoHdr, host_status), 68),
            (offset_of!(SgIoHdr, driver_status), 70),
            (offset_of!(SgIoHdr, resid), 72),
            (offset_of!(SgIoHdr, duration), 76),
            (offset_of!(SgIoHdr, info), 80),
        ];
        for (actual, wanted) in expected {
            assert_eq!(actual, wanted);
        }
    }

    /// `_IOC(dir, type, nr, size)` = dir << 30 | size << 16 | type << 8 | nr.
    #[test]
    fn ioctl_numbers_match_the_kernel() {
        let ioc = |dir: u64, kind: u8, nr: u8, size: usize| -> libc::c_ulong {
            (dir << 30 | (size as u64) << 16 | u64::from(kind) << 8 | u64::from(nr))
                as libc::c_ulong
        };
        assert_eq!(
            NVME_IOCTL_ADMIN_CMD,
            ioc(3, b'N', 0x41, size_of::<NvmePassthruCmd>())
        );
        assert_eq!(NVME_IOCTL_ID, ioc(0, b'N', 0x40, 0));
        assert_eq!(BLKGETSIZE64, ioc(2, 0x12, 114, size_of::<u64>()));
        assert_eq!(SG_IO, 0x2285);
        assert_eq!(BLKSSZGET, 0x1268);
        assert_eq!(BLKFLSBUF, 0x1261);
        assert_eq!(BLKRRPART, 0x125f);
        assert_eq!(BLKROGET, 0x125e);
    }

    #[test]
    fn zeroed_header_is_an_sg_header() {
        let hdr = SgIoHdr::zeroed();
        assert_eq!(hdr.interface_id, i32::from(b'S'));
        assert!(hdr.dxferp.is_null() && hdr.cmdp.is_null() && hdr.sbp.is_null());
    }
}
