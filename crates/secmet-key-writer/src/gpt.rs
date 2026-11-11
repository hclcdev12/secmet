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

use std::io;

/// Taille de secteur de l'image (GPT à secteurs de 512 octets).
pub const SECTOR: u64 = 512;
const HEADER_SIGNATURE: &[u8; 8] = b"EFI PART";

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
    /// Secteurs 0 et 1 tels que l'image les contient.
    pub pristine_head: Vec<u8>,
    /// Secteurs 0 et 1 complétés (entrée protectrice, en-tête primaire).
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
    let entries = &first[eo as usize..eend as usize];
    if crc32(&entries[..entries_bytes as usize]) != u32_at(&header.raw, 88) {
        return Err(bad("CRC des entrées GPT invalide".into()));
    }
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

    let pristine_head = first[..2 * SECTOR as usize].to_vec();
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
    head[SECTOR as usize..].copy_from_slice(&primary);
    let backup = header.rewrite(last_lba, 1, last_usable, backup_entries_lba);
    let mut tail = entries.to_vec();
    tail.extend_from_slice(&backup);
    Ok(Some(Plan {
        pristine_head,
        head,
        tail_lba: backup_entries_lba,
        tail,
        message: format!(
            "GPT complétée : copie de secours au secteur {last_lba}, {} utilisables (l'image en déclarait {})",
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
        let mut d = vec![0u8; 34 * SECTOR as usize];
        d[0x1BE + 4] = 0xEE;
        d[0x1BE + 8..0x1BE + 12].copy_from_slice(&1u32.to_le_bytes());
        d[0x1BE + 12..0x1BE + 16].copy_from_slice(&((image_sectors - 1) as u32).to_le_bytes());
        d[510] = 0x55;
        d[511] = 0xAA;
        let mut entries = vec![0u8; 128 * 128];
        entries[0] = 1; // type GUID non nul : entrée utilisée
        entries[32..40].copy_from_slice(&2048u64.to_le_bytes());
        entries[40..48].copy_from_slice(&(image_sectors - 40).to_le_bytes());
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
        h[88..92].copy_from_slice(&crc32(&entries).to_le_bytes());
        let crc = crc32(&h[..92]);
        h[16..20].copy_from_slice(&crc.to_le_bytes());
        d[512..1024].copy_from_slice(&h);
        d[1024..1024 + entries.len()].copy_from_slice(&entries);
        d
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
        assert_eq!(p.pristine_head, first[..1024].to_vec());
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
