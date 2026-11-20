//! Complétion de la table GPT après l'écriture de l'image.
//!
//! L'image publiée fait 11 Gio : sa GPT de secours se trouve à la fin de
//! l'image, pas à la fin de la clé (14, 16, 32 Go…). Linux s'en accommode
//! (« Alternate GPT is invalid, using primary GPT ») mais Windows, à la
//! première ré-énumération du disque (fin de l'écriture, rebranchement),
//! « répare » la table : il réécrit l'en-tête primaire et, ce faisant,
//! écrase la signature `55 AA` du MBR protecteur, ce qui rend la clé
//! illisible pour Linux et non démarrable (constaté sur Windows 11 25H2).
//!
//! Le worker termine donc le travail lui-même, pendant l'écriture : copie
//! des entrées et en-tête de secours en fin de disque (écrits avant le
//! premier bloc, pour que le système ne voie jamais une table incomplète),
//! en-tête primaire corrigé (`AlternateLBA` et `LastUsableLBA` au bout du
//! disque, CRC recalculé) et entrée protectrice du MBR étendue au disque.
//! Windows n'a plus rien à réparer ; le premier démarrage de la clé
//! (`secmet-first-boot`) agrandit ensuite la partition d'export comme
//! avant. L'empreinte affichée reste celle de l'image : la relecture
//! compare ces secteurs à part.
//!
//! Au passage, les partitions système (`SECMET-SYS`) et Linux
//! (`SECMET-DATA`) reçoivent les attributs GPT qui les cachent à Windows
//! ([`windows_attributes`]) : sans lettre de lecteur, Windows ne propose
//! plus de « formater » la partition Linux et ne monte plus la partition
//! système. Les images à partir de la 0.5.0 les portent déjà
//! (`make_key_image.sh`) : les octets sont alors inchangés ; une image plus
//! ancienne les reçoit ici. La partition d'export reste visible.

use std::io;

/// Taille de secteur de l'image (GPT à secteurs de 512 octets).
pub const SECTOR: u64 = 512;
const HEADER_SIGNATURE: &[u8; 8] = b"EFI PART";

/// Attribut GPT « lecture seule » (bit 60, propre au type de partition,
/// convention Microsoft).
pub const ATTR_READ_ONLY: u64 = 1 << 60;
/// Attribut GPT « masquée » (bit 62, convention Microsoft).
pub const ATTR_HIDDEN: u64 = 1 << 62;
/// Attribut GPT « pas de lettre de lecteur par défaut » (bit 63,
/// convention Microsoft ; « no-auto » pour systemd).
pub const ATTR_NO_DRIVE_LETTER: u64 = 1 << 63;
/// Type « EFI System » (C12A7328-F81F-11D2-BA4B-00A0C93EC93B), tel
/// qu'écrit sur le disque (trois premiers champs en petit-boutiste).
const TYPE_ESP: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];
/// Type « Linux filesystem data » (0FC63DAF-8483-4772-8E79-3D69D8477DE4).
const TYPE_LINUX: [u8; 16] = [
    0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47, 0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4,
];

/// Nom UTF-16LE d'une entrée GPT (octets 56 à 127), sans les zéros finaux.
fn entry_name(entry: &[u8]) -> String {
    let units: Vec<u16> = entry[56..128]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Attributs à poser sur une entrée de la clé SecMet pour que Windows ne
/// la monte pas (Annexe M §M.3) : partition système (EFI System
/// `SECMET-SYS`) lecture seule, masquée, sans lettre ; partition de données
/// (Linux `SECMET-DATA`) masquée, sans lettre. Rien pour les autres, en
/// particulier la partition d'export, qui doit rester lisible sous
/// Windows. Les bits 48 à 63 dépendent du type et sont ignorés par le
/// firmware (UEFI §5.3.3) ; le bit 1 (« No Block IO Protocol », qui
/// rendrait la clé non démarrable) n'est jamais posé.
pub fn windows_attributes(entry: &[u8]) -> u64 {
    let kind: &[u8] = &entry[..16];
    match entry_name(entry).as_str() {
        "SECMET-SYS" if kind == TYPE_ESP => ATTR_READ_ONLY | ATTR_HIDDEN | ATTR_NO_DRIVE_LETTER,
        "SECMET-DATA" if kind == TYPE_LINUX => ATTR_HIDDEN | ATTR_NO_DRIVE_LETTER,
        _ => 0,
    }
}

/// CRC-32 IEEE (celui de la GPT, de zlib et d'Ethernet).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// En-tête GPT primaire lu et contrôlé.
struct Header {
    raw: Vec<u8>,
    size: usize,
    alternate_lba: u64,
    entries_lba: u64,
    entries_count: u32,
    entry_size: u32,
}

impl Header {
    fn parse(sector: &[u8]) -> Result<Self, String> {
        if &sector[..8] != HEADER_SIGNATURE {
            return Err("pas d'en-tête GPT".into());
        }
        let size = u32_at(sector, 12) as usize;
        if !(92..=SECTOR as usize).contains(&size) {
            return Err(format!("taille d'en-tête GPT invalide ({size})"));
        }
        let mut zeroed = sector[..size].to_vec();
        zeroed[16..20].fill(0);
        if crc32(&zeroed) != u32_at(sector, 16) {
            return Err("CRC de l'en-tête GPT invalide".into());
        }
        if u64_at(sector, 24) != 1 {
            return Err("en-tête GPT primaire absent du secteur 1".into());
        }
        let entries_count = u32_at(sector, 80);
        let entry_size = u32_at(sector, 84);
        if entry_size < 128
            || !entry_size.is_multiple_of(8)
            || entries_count == 0
            || entries_count > 1024
        {
            return Err("table d'entrées GPT invalide".into());
        }
        Ok(Header {
            raw: sector.to_vec(),
            size,
            alternate_lba: u64_at(sector, 32),
            entries_lba: u64_at(sector, 72),
            entries_count,
            entry_size,
        })
    }

    /// Copie de cet en-tête avec les champs de position modifiés et le
    /// CRC recalculé.
    fn rewrite(
        &self,
        my_lba: u64,
        alternate_lba: u64,
        last_usable: u64,
        entries_lba: u64,
    ) -> Vec<u8> {
        let mut h = self.raw.clone();
        h[24..32].copy_from_slice(&my_lba.to_le_bytes());
        h[32..40].copy_from_slice(&alternate_lba.to_le_bytes());
        h[48..56].copy_from_slice(&last_usable.to_le_bytes());
        h[72..80].copy_from_slice(&entries_lba.to_le_bytes());
        h[16..20].fill(0);
        let crc = crc32(&h[..self.size]);
        h[16..20].copy_from_slice(&crc.to_le_bytes());
        h
    }
}

/// Vrai si le secteur 0 est un MBR protecteur de GPT (signature `55 AA`,
/// une entrée de type `EE`).
fn is_protective_mbr(sector: &[u8]) -> bool {
    sector[510] == 0x55 && sector[511] == 0xAA && (0..4).any(|i| sector[0x1BE + 16 * i + 4] == 0xEE)
}

/// Secteurs à écrire pour compléter la GPT d'une image de `image_bytes`
/// octets sur un disque de `disk_bytes` octets.
pub struct Plan {
    /// Début de l'image tel qu'elle le contient : du secteur 0 à la fin de
    /// la table des entrées.
    pub pristine_head: Vec<u8>,
    /// Même zone complétée (entrée protectrice, en-tête primaire, attributs
    /// Windows des entrées).
    pub head: Vec<u8>,
    /// Premier secteur de la zone de secours (entrées puis en-tête, contiguë
    /// jusqu'au dernier secteur du disque).
    pub tail_lba: u64,
    /// Entrées de secours suivies de l'en-tête de secours.
    pub tail: Vec<u8>,
    /// Ligne pour le journal.
    pub message: String,
}

/// Prépare la complétion à partir du début de l'image (`first` : au moins
/// les secteurs 0, 1 et la table des entrées) : `None` si l'image ne porte
/// pas de GPT (image MBR), si sa copie de secours est déjà au bout du
/// disque ou si la table des entrées n'est pas dans `first`. Rien n'est
/// écrit ici : l'écriture suit l'ordre du worker (secours d'abord, pour
/// qu'une lecture de la table par le système ne la trouve jamais
/// incomplète).
pub fn plan(first: &[u8], disk_bytes: u64) -> io::Result<Option<Plan>> {
    let bad = |m: String| io::Error::new(io::ErrorKind::InvalidData, m);
    if first.len() < 2 * SECTOR as usize || !is_protective_mbr(&first[..SECTOR as usize]) {
        return Ok(None);
    }
    let header = Header::parse(&first[SECTOR as usize..2 * SECTOR as usize]).map_err(bad)?;
    let sectors = disk_bytes / SECTOR;
    let last_lba = sectors - 1;
    if header.alternate_lba == last_lba {
        return Ok(None);
    }
    let entries_bytes = u64::from(header.entries_count) * u64::from(header.entry_size);
    let entries_sectors = entries_bytes.div_ceil(SECTOR);
    let eo = header.entries_lba * SECTOR;
    let eend = eo + entries_sectors * SECTOR;
    if eend > first.len() as u64 {
        return Ok(None);
    }
    let pristine_entries = &first[eo as usize..eend as usize];
    if crc32(&pristine_entries[..entries_bytes as usize]) != u32_at(&header.raw, 88) {
        return Err(bad("CRC des entrées GPT invalide".into()));
    }
    // Attributs Windows (voir `windows_attributes`), ajoutés à ceux de
    // l'image (octets 48 à 55 de chaque entrée), puis CRC des entrées
    // recalculé dans les deux en-têtes.
    let mut entries = pristine_entries.to_vec();
    let mut hidden = Vec::new();
    for i in 0..header.entries_count as usize {
        let e = &mut entries[i * header.entry_size as usize..(i + 1) * header.entry_size as usize];
        let wanted = windows_attributes(e);
        let attrs = u64_at(e, 48);
        if attrs | wanted != attrs {
            e[48..56].copy_from_slice(&(attrs | wanted).to_le_bytes());
            hidden.push(entry_name(e));
        }
    }
    let mut header = header;
    let entries_crc = crc32(&entries[..entries_bytes as usize]);
    header.raw[88..92].copy_from_slice(&entries_crc.to_le_bytes());
    let backup_entries_lba = last_lba - entries_sectors;
    let last_usable = backup_entries_lba - 1;
    if last_usable <= u64_at(&header.raw, 40) || header.alternate_lba > last_lba {
        return Err(bad(format!(
            "disque trop petit pour la GPT de secours ({sectors} secteurs)"
        )));
    }
    // L'espace utilisable ne doit pas rétrécir sous la dernière partition.
    let mut used_end = 0u64;
    for i in 0..header.entries_count as usize {
        let e = &entries[i * header.entry_size as usize..];
        if e[..16].iter().any(|&b| b != 0) {
            used_end = used_end.max(u64_at(e, 40));
        }
    }
    if used_end > last_usable {
        return Err(bad(
            "une partition dépasse l'espace utilisable du disque".into()
        ));
    }

    let pristine_head = first[..eend as usize].to_vec();
    let mut head = pristine_head.clone();
    // L'entrée protectrice du MBR couvre tout le disque (au plus 2^32 - 1
    // secteurs), comme l'exige la spécification.
    for i in 0..4 {
        let e = 0x1BE + 16 * i;
        if head[e + 4] == 0xEE {
            let span = u32::try_from(last_lba).unwrap_or(u32::MAX);
            head[e + 12..e + 16].copy_from_slice(&span.to_le_bytes());
        }
    }
    let primary = header.rewrite(1, last_lba, last_usable, header.entries_lba);
    head[SECTOR as usize..2 * SECTOR as usize].copy_from_slice(&primary);
    head[eo as usize..eend as usize].copy_from_slice(&entries);
    let backup = header.rewrite(last_lba, 1, last_usable, backup_entries_lba);
    let mut tail = entries;
    tail.extend_from_slice(&backup);
    let hidden_note = if hidden.is_empty() {
        String::new()
    } else {
        format!(" ; masquées pour Windows : {}", hidden.join(", "))
    };
    Ok(Some(Plan {
        pristine_head,
        head,
        tail_lba: backup_entries_lba,
        tail,
        message: format!(
            "GPT complétée : copie de secours au secteur {last_lba}, {} utilisables (l'image en déclarait {}){hidden_note}",
            crate::fmt_size((last_usable + 1) * SECTOR),
            crate::fmt_size((header.alternate_lba + 1) * SECTOR)
        ),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Début d'image GPT de `image_sectors` secteurs : MBR protecteur,
    /// en-tête primaire, 128 entrées dont une partition de 2048 à
    /// `image_sectors - 40`.
    fn image_head(image_sectors: u64) -> Vec<u8> {
        let mut entries = vec![0u8; 128 * 128];
        entries[0] = 1; // type GUID non nul : entrée utilisée
        entries[32..40].copy_from_slice(&2048u64.to_le_bytes());
        entries[40..48].copy_from_slice(&(image_sectors - 40).to_le_bytes());
        head_with_entries(image_sectors, &entries)
    }

    /// Entrée GPT de type `kind`, nommée `name`, avec les attributs `attrs`.
    fn entry(kind: [u8; 16], name: &str, first: u64, last: u64, attrs: u64) -> Vec<u8> {
        let mut e = vec![0u8; 128];
        e[..16].copy_from_slice(&kind);
        e[16] = 0x42; // GUID unique quelconque
        e[32..40].copy_from_slice(&first.to_le_bytes());
        e[40..48].copy_from_slice(&last.to_le_bytes());
        e[48..56].copy_from_slice(&attrs.to_le_bytes());
        for (i, u) in name.encode_utf16().enumerate() {
            e[56 + 2 * i..58 + 2 * i].copy_from_slice(&u.to_le_bytes());
        }
        e
    }

    /// Image au schéma de la clé SecMet : système, données, export, BIOS
    /// boot ; `sys_attrs` et `data_attrs` déjà présents dans l'image.
    fn secmet_head(image_sectors: u64, sys_attrs: u64, data_attrs: u64) -> Vec<u8> {
        const TYPE_BASIC: [u8; 16] = [
            0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26,
            0x99, 0xC7,
        ];
        const TYPE_BIOS: [u8; 16] = *b"Hah!IdontNeedEFI";
        let mut entries = Vec::new();
        entries.extend(entry(TYPE_ESP, "SECMET-SYS", 4096, 8191, sys_attrs));
        entries.extend(entry(TYPE_LINUX, "SECMET-DATA", 8192, 12287, data_attrs));
        entries.extend(entry(
            TYPE_BASIC,
            "SECMET-EXPORT",
            12288,
            image_sectors - 40,
            0,
        ));
        entries.extend(entry(TYPE_BIOS, "BIOS boot", 2048, 4095, 0));
        entries.resize(128 * 128, 0);
        head_with_entries(image_sectors, &entries)
    }

    fn head_with_entries(image_sectors: u64, entries: &[u8]) -> Vec<u8> {
        let mut d = vec![0u8; 34 * SECTOR as usize];
        d[0x1BE + 4] = 0xEE;
        d[0x1BE + 8..0x1BE + 12].copy_from_slice(&1u32.to_le_bytes());
        d[0x1BE + 12..0x1BE + 16].copy_from_slice(&((image_sectors - 1) as u32).to_le_bytes());
        d[510] = 0x55;
        d[511] = 0xAA;
        let mut h = vec![0u8; 512];
        h[..8].copy_from_slice(HEADER_SIGNATURE);
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        h[12..16].copy_from_slice(&92u32.to_le_bytes());
        h[24..32].copy_from_slice(&1u64.to_le_bytes());
        h[32..40].copy_from_slice(&(image_sectors - 1).to_le_bytes());
        h[40..48].copy_from_slice(&34u64.to_le_bytes());
        h[48..56].copy_from_slice(&(image_sectors - 34).to_le_bytes());
        h[72..80].copy_from_slice(&2u64.to_le_bytes());
        h[80..84].copy_from_slice(&128u32.to_le_bytes());
        h[84..88].copy_from_slice(&128u32.to_le_bytes());
        h[88..92].copy_from_slice(&crc32(entries).to_le_bytes());
        let crc = crc32(&h[..92]);
        h[16..20].copy_from_slice(&crc.to_le_bytes());
        d[512..1024].copy_from_slice(&h);
        d[1024..1024 + entries.len()].copy_from_slice(entries);
        d
    }

    fn attrs_of(entries: &[u8], i: usize) -> u64 {
        u64_at(&entries[i * 128..], 48)
    }

    fn header_crc_ok(h: &[u8]) -> bool {
        let mut z = h[..92].to_vec();
        z[16..20].fill(0);
        crc32(&z) == u32_at(h, 16)
    }

    #[test]
    fn crc32_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn plans_backup_at_disk_end() {
        let image = 4096u64;
        let total = 8192u64;
        let first = image_head(image);
        let p = plan(&first, total * SECTOR).unwrap().expect("plan");
        assert!(p.message.starts_with("GPT complétée"), "{}", p.message);
        assert!(!p.message.contains("masquées"), "{}", p.message);
        // Du secteur 0 à la fin des entrées (2 + 32 secteurs).
        assert_eq!(p.pristine_head, first[..34 * 512].to_vec());
        assert_eq!(p.head.len(), 34 * 512);
        // Image sans partition SecMet : entrées inchangées.
        assert_eq!(&p.head[1024..], &first[1024..]);
        // MBR protecteur : signature intacte, entrée étendue au disque.
        assert_eq!(&p.head[510..512], &[0x55, 0xAA]);
        assert_eq!(u32_at(&p.head, 0x1BE + 12), (total - 1) as u32);
        assert_eq!(&p.head[..0x1BE], &first[..0x1BE]);
        // En-tête primaire corrigé.
        let prim = &p.head[512..];
        assert!(header_crc_ok(prim));
        assert_eq!(u64_at(prim, 24), 1);
        assert_eq!(u64_at(prim, 32), total - 1);
        assert_eq!(u64_at(prim, 48), total - 34);
        assert_eq!(u64_at(prim, 72), 2);
        // Secours : entrées puis en-tête, jusqu'au dernier secteur.
        assert_eq!(p.tail_lba, total - 33);
        assert_eq!(p.tail.len(), 33 * 512);
        assert_eq!(&p.tail[..128 * 128], &first[1024..1024 + 128 * 128]);
        let b = &p.tail[32 * 512..];
        assert_eq!(&b[..8], HEADER_SIGNATURE);
        assert!(header_crc_ok(b));
        assert_eq!(u64_at(b, 24), total - 1);
        assert_eq!(u64_at(b, 32), 1);
        assert_eq!(u64_at(b, 72), total - 33);
        assert_eq!(u64_at(b, 48), total - 34);
    }

    #[test]
    fn hides_system_and_data_partitions_from_windows() {
        let image = 16384u64;
        let total = 32768u64;
        // Image antérieure à la 0.5.0 : aucun attribut ; bit 0 « Required »
        // déjà posé sur la partition de données, à conserver.
        let first = secmet_head(image, 0, 1);
        let p = plan(&first, total * SECTOR).unwrap().expect("plan");
        assert!(
            p.message
                .ends_with("masquées pour Windows : SECMET-SYS, SECMET-DATA"),
            "{}",
            p.message
        );
        let entries = &p.head[1024..];
        assert_eq!(attrs_of(entries, 0), (1 << 60) | (1 << 62) | (1 << 63));
        assert_eq!(attrs_of(entries, 1), 1 | (1 << 62) | (1 << 63));
        // Export et BIOS boot : intacts ; jamais le bit 1 (No Block IO).
        assert_eq!(attrs_of(entries, 2), 0);
        assert_eq!(attrs_of(entries, 3), 0);
        for i in 0..4 {
            assert_eq!(attrs_of(entries, i) & 0b10, 0);
        }
        // Seuls les octets 48 à 55 des deux entrées changent.
        for (i, (a, b)) in entries.iter().zip(&first[1024..]).enumerate() {
            let changed = (i < 256) && (48..56).contains(&(i % 128));
            if !changed {
                assert_eq!(a, b, "octet {i} des entrées modifié");
            }
        }
        // CRC des entrées et des en-têtes cohérents, secours identique.
        let crc = crc32(&entries[..128 * 128]);
        assert_eq!(u32_at(&p.head[512..], 88), crc);
        assert!(header_crc_ok(&p.head[512..]));
        assert_eq!(&p.tail[..128 * 128], entries);
        let backup = &p.tail[32 * 512..];
        assert_eq!(u32_at(backup, 88), crc);
        assert!(header_crc_ok(backup));
        // L'image d'origine sert toujours à l'empreinte.
        assert_eq!(p.pristine_head, first);
    }

    #[test]
    fn keeps_attributes_already_in_the_image() {
        let image = 16384u64;
        let sys = (1u64 << 60) | (1 << 62) | (1 << 63);
        let data = (1u64 << 62) | (1 << 63);
        let first = secmet_head(image, sys, data);
        let p = plan(&first, 32768 * SECTOR).unwrap().expect("plan");
        assert!(!p.message.contains("masquées"), "{}", p.message);
        // Image 0.5.0 : entrées recopiées octet pour octet (octets 48 à 55
        // compris) dans la table primaire et dans la copie de secours.
        assert_eq!(&p.head[1024..], &first[1024..]);
        assert_eq!(&p.tail[..128 * 128], &first[1024..1024 + 128 * 128]);
        assert_eq!(attrs_of(&p.tail, 0), sys);
        assert_eq!(attrs_of(&p.tail, 1), data);
    }

    #[test]
    fn attributes_follow_type_and_name() {
        let sys = entry(TYPE_ESP, "SECMET-SYS", 1, 2, 0);
        assert_eq!(
            windows_attributes(&sys),
            ATTR_READ_ONLY | ATTR_HIDDEN | ATTR_NO_DRIVE_LETTER
        );
        let data = entry(TYPE_LINUX, "SECMET-DATA", 1, 2, 0);
        assert_eq!(
            windows_attributes(&data),
            ATTR_HIDDEN | ATTR_NO_DRIVE_LETTER
        );
        // Autre nom, ou bon nom sur un autre type : rien.
        assert_eq!(windows_attributes(&entry(TYPE_ESP, "EFI", 1, 2, 0)), 0);
        assert_eq!(
            windows_attributes(&entry(TYPE_LINUX, "SECMET-SYS", 1, 2, 0)),
            0
        );
        assert_eq!(
            windows_attributes(&entry(TYPE_LINUX, "SECMET-EXPORT", 1, 2, 0)),
            0
        );
    }

    #[test]
    fn leaves_mbr_images_and_exact_size_alone() {
        let image = 4096u64;
        assert!(plan(&image_head(image), image * SECTOR).unwrap().is_none());
        let mut plain = vec![0u8; 34 * 512];
        plain[510] = 0x55;
        plain[511] = 0xAA;
        plain[0x1BE + 4] = 0x0C;
        assert!(plan(&plain, 8192 * 512).unwrap().is_none());
        // Table des entrées hors du début fourni : on n'y touche pas.
        assert!(plan(&image_head(image)[..1024], 8192 * 512)
            .unwrap()
            .is_none());
    }

    #[test]
    fn rejects_corrupt_header() {
        let mut d = image_head(4096);
        d[600] ^= 1;
        assert!(plan(&d, 8192 * SECTOR).is_err());
    }
}
