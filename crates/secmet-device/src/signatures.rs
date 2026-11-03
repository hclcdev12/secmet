//! Signatures résiduelles en tête et en queue de disque : table de
//! partitions, systèmes de fichiers, conteneurs. Remplace `blkid` dans la
//! vérification post-effacement de SecMet.
//!
//! Lecture seule du premier mébioctet (couvre btrfs à 64 Kio, ZFS à 128 Kio)
//! et du dernier bloc logique (GPT de secours). Sous-ensemble volontaire de
//! libblkid : les structures attendues sur un disque de poste ou de serveur.
//! Un disque effacé ne doit en présenter **aucune** ; la preuve principale
//! reste la lecture par sondes SHA-256, hors de ce module.

/// Octets lus en tête de disque.
pub const HEAD_BYTES: usize = 1 << 20;

/// Famille de signature détectée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SignatureType {
    /// Table de partitions MBR (0x55AA et au moins une entrée typée).
    Mbr,
    /// En-tête GPT primaire (LBA 1).
    Gpt,
    /// En-tête GPT de secours (dernier LBA).
    GptBackup,
    /// ext2 / ext3 / ext4.
    Ext4,
    /// NTFS.
    Ntfs,
    /// FAT12 / FAT16 / FAT32.
    Fat,
    /// exFAT.
    ExFat,
    /// btrfs.
    Btrfs,
    /// ZFS (uberblock du premier label).
    Zfs,
    /// XFS.
    Xfs,
    /// LUKS 1 ou 2.
    Luks,
    /// Physical volume LVM2.
    LvmPv,
    /// Swap Linux.
    Swap,
    /// Superbloc md RAID 1.x.
    MdRaid,
    /// ISO 9660.
    Iso9660,
}

impl SignatureType {
    /// Libellé proche de `TYPE=` / `PTTYPE=` de blkid, pour les journaux.
    pub fn label(self) -> &'static str {
        match self {
            Self::Mbr => "PTTYPE=dos",
            Self::Gpt => "PTTYPE=gpt",
            Self::GptBackup => "PTTYPE=gpt (secours)",
            Self::Ext4 => "TYPE=ext4",
            Self::Ntfs => "TYPE=ntfs",
            Self::Fat => "TYPE=vfat",
            Self::ExFat => "TYPE=exfat",
            Self::Btrfs => "TYPE=btrfs",
            Self::Zfs => "TYPE=zfs_member",
            Self::Xfs => "TYPE=xfs",
            Self::Luks => "TYPE=crypto_LUKS",
            Self::LvmPv => "TYPE=LVM2_member",
            Self::Swap => "TYPE=swap",
            Self::MdRaid => "TYPE=linux_raid_member",
            Self::Iso9660 => "TYPE=iso9660",
        }
    }
}

/// Une signature et sa position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature {
    /// Famille.
    pub kind: SignatureType,
    /// Décalage absolu sur le disque, en octets.
    pub offset: u64,
}

fn at(buffer: &[u8], offset: usize, magic: &[u8]) -> bool {
    buffer.get(offset..offset + magic.len()) == Some(magic)
}

/// Analyse `head` (début du disque) et `tail` (dernier bloc, situé à
/// `tail_offset`) pour une taille de bloc logique `block_size`. Pure.
pub fn scan_buffers(head: &[u8], tail: &[u8], tail_offset: u64, block_size: u64) -> Vec<Signature> {
    let mut found = Vec::new();
    let mut push = |kind: SignatureType, offset: u64| found.push(Signature { kind, offset });

    let boot_signature = at(head, 510, &[0x55, 0xaa]);
    if at(head, 3, b"NTFS    ") {
        push(SignatureType::Ntfs, 0);
    } else if at(head, 3, b"EXFAT   ") {
        push(SignatureType::ExFat, 0);
    } else if boot_signature && (at(head, 54, b"FAT1") || at(head, 82, b"FAT32   ")) {
        push(SignatureType::Fat, 0);
    } else if boot_signature {
        // MBR : au moins une entrée de partition avec un type non nul.
        let has_entry = (0..4).any(|i| head.get(446 + 16 * i + 4).is_some_and(|&t| t != 0));
        if has_entry {
            push(SignatureType::Mbr, 0);
        }
    }
    if at(head, 0, b"XFSB") {
        push(SignatureType::Xfs, 0);
    }
    if at(head, 0, b"LUKS\xba\xbe") {
        push(SignatureType::Luks, 0);
    }
    let lba1 = block_size as usize;
    if at(head, lba1, b"EFI PART") {
        push(SignatureType::Gpt, block_size);
    }
    // LVM2 : « LABELONE » dans l'un des 4 premiers secteurs.
    for sector in 0..4usize {
        if at(head, sector * 512, b"LABELONE") {
            push(SignatureType::LvmPv, (sector * 512) as u64);
            break;
        }
    }
    // ext2/3/4 : superbloc à 1024, magic 0xEF53 à +56.
    if at(head, 1024 + 56, &[0x53, 0xef]) {
        push(SignatureType::Ext4, 1024);
    }
    // Swap : magic en fin de page (4 Kio, ou 64 Kio sur certaines architectures).
    for page in [4096usize, 65536] {
        if at(head, page - 10, b"SWAPSPACE2") || at(head, page - 10, b"SWAP-SPACE") {
            push(SignatureType::Swap, (page - 10) as u64);
            break;
        }
    }
    // md 1.x : magic 0xa92b4efc LE à 4096 (1.2) ou 0 (1.1).
    for offset in [4096usize, 0] {
        if at(head, offset, &[0xfc, 0x4e, 0x2b, 0xa9]) {
            push(SignatureType::MdRaid, offset as u64);
            break;
        }
    }
    if at(head, 32768 + 1, b"CD001") {
        push(SignatureType::Iso9660, 32768);
    }
    if at(head, 65536 + 64, b"_BHRfS_M") {
        push(SignatureType::Btrfs, 65536);
    }
    // ZFS : le premier label occupe 256 Kio, ses uberblocks commencent à
    // 128 Kio, magic 0x00bab10c (LE ou BE selon la machine créatrice).
    let uberblocks = 128 * 1024;
    if at(head, uberblocks, &[0x0c, 0xb1, 0xba, 0x00])
        || at(head, uberblocks, &[0x00, 0xba, 0xb1, 0x0c])
    {
        push(SignatureType::Zfs, uberblocks as u64);
    }
    if at(tail, 0, b"EFI PART") {
        push(SignatureType::GptBackup, tail_offset);
    }
    found
}

/// Texte façon blkid pour les journaux (vide si aucune signature).
pub fn describe(path: &std::path::Path, found: &[Signature]) -> String {
    found
        .iter()
        .map(|s| format!("{}: {} @ {}", path.display(), s.kind.label(), s.offset))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn blank() -> (Vec<u8>, Vec<u8>) {
        (vec![0u8; HEAD_BYTES], vec![0u8; 512])
    }

    fn kinds(head: &[u8], tail: &[u8]) -> Vec<SignatureType> {
        scan_buffers(head, tail, 1 << 30, 512)
            .iter()
            .map(|s| s.kind)
            .collect()
    }

    #[test]
    fn blank_media_has_no_signature() {
        let (mut head, tail) = blank();
        assert!(kinds(&head, &tail).is_empty());
        // Un 0x55AA seul, sans entrée de partition, n'est pas une MBR.
        head[510] = 0x55;
        head[511] = 0xaa;
        assert!(kinds(&head, &tail).is_empty());
    }

    #[test]
    fn detects_partition_tables_primary_and_backup() {
        let (mut head, mut tail) = blank();
        head[510] = 0x55;
        head[511] = 0xaa;
        head[446 + 4] = 0x83;
        assert_eq!(kinds(&head, &tail), [SignatureType::Mbr]);
        head[512..520].copy_from_slice(b"EFI PART");
        tail[..8].copy_from_slice(b"EFI PART");
        let found = scan_buffers(&head, &tail, 999_999_488, 512);
        assert_eq!(
            found.iter().map(|s| s.kind).collect::<Vec<_>>(),
            [
                SignatureType::Mbr,
                SignatureType::Gpt,
                SignatureType::GptBackup
            ]
        );
        assert_eq!(found[2].offset, 999_999_488);
        let mut head4k = vec![0u8; HEAD_BYTES];
        head4k[4096..4104].copy_from_slice(b"EFI PART");
        assert_eq!(scan_buffers(&head4k, &[0; 4096], 0, 4096)[0].offset, 4096);
    }

    #[test]
    fn detects_filesystems_and_containers() {
        let cases: Vec<(SignatureType, usize, &[u8])> = vec![
            (SignatureType::Ntfs, 3, b"NTFS    "),
            (SignatureType::ExFat, 3, b"EXFAT   "),
            (SignatureType::Xfs, 0, b"XFSB"),
            (SignatureType::Luks, 0, b"LUKS\xba\xbe"),
            (SignatureType::LvmPv, 512, b"LABELONE"),
            (SignatureType::Ext4, 1024 + 56, &[0x53, 0xef]),
            (SignatureType::Swap, 4096 - 10, b"SWAPSPACE2"),
            (SignatureType::MdRaid, 4096, &[0xfc, 0x4e, 0x2b, 0xa9]),
            (SignatureType::Iso9660, 32769, b"CD001"),
            (SignatureType::Btrfs, 65536 + 64, b"_BHRfS_M"),
            (SignatureType::Zfs, 128 * 1024, &[0x0c, 0xb1, 0xba, 0x00]),
            (SignatureType::Zfs, 128 * 1024, &[0x00, 0xba, 0xb1, 0x0c]),
        ];
        for (kind, offset, magic) in cases {
            let (mut head, tail) = blank();
            head[offset..offset + magic.len()].copy_from_slice(magic);
            assert_eq!(kinds(&head, &tail), [kind], "{kind:?}");
        }
        let (mut head, tail) = blank();
        head[510] = 0x55;
        head[511] = 0xaa;
        head[54..58].copy_from_slice(b"FAT1");
        assert_eq!(kinds(&head, &tail), [SignatureType::Fat]);
        head[54..58].fill(0);
        head[82..90].copy_from_slice(b"FAT32   ");
        assert_eq!(kinds(&head, &tail), [SignatureType::Fat]);
    }

    #[test]
    fn short_buffers_never_panic() {
        assert!(scan_buffers(&[], &[], 0, 512).is_empty());
        assert!(scan_buffers(&[0x55; 100], &[1], 0, 512).is_empty());
    }

    #[test]
    fn describe_is_blkid_like() {
        let found = [Signature {
            kind: SignatureType::Gpt,
            offset: 512,
        }];
        assert_eq!(
            describe(std::path::Path::new("/dev/sdz"), &found),
            "/dev/sdz: PTTYPE=gpt @ 512"
        );
        assert_eq!(describe(std::path::Path::new("/dev/sdz"), &[]), "");
        assert_eq!(SignatureType::Zfs.label(), "TYPE=zfs_member");
    }
}
