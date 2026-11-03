//! Canaris et motifs de validation, dérivés de ChaCha20 (RFC 8439).
//!
//! Principe : avant une méthode firmware, un motif unique par bloc (le
//! *canari*) est écrit sur un échantillon de blocs ; après, aucun canari ne
//! doit subsister. Pour une écriture hôte, chaque passe écrit un flux
//! pseudo-aléatoire recomputable par LBA ; la validation vérifie que les
//! blocs échantillonnés portent bien le motif de la dernière passe (ou des
//! zéros, après la passe finale). Un contrôleur qui acquitte sans écrire, ou
//! un firmware qui ignore la commande, est ainsi pris en défaut.
//!
//! Tout est dérivé d'un **seed de session** de 32 octets et du LBA : le
//! helper peut recalculer n'importe quel bloc attendu sans rien stocker
//! d'autre que le seed (consigné dans le rapport).
//!
//! Aucune I/O ici : le helper lit et écrit, ce module calcule et compare.

use chacha20::cipher::{KeyIvInit, StreamCipher};
use chacha20::ChaCha20;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::{CoreError, Result};

/// Seed de session : 32 octets aléatoires fournis par le helper.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SessionSeed([u8; 32]);

impl SessionSeed {
    /// Depuis 32 octets (issus de `/dev/urandom` côté helper).
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Les octets, pour le rapport.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Hexadécimal minuscule, pour le rapport.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Depuis l'hexadécimal d'un rapport.
    pub fn from_hex(text: &str) -> Option<Self> {
        if text.len() != 64 || !text.is_ascii() {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }

    /// Sous-clé par usage : SHA-256(seed ‖ étiquette). Les canaris, les
    /// passes d'écriture et l'échantillonnage ne partagent jamais une clé.
    fn subkey(&self, label: &[u8]) -> [u8; 32] {
        Sha256::new()
            .chain_update(self.0)
            .chain_update(label)
            .finalize()
            .into()
    }
}

impl Drop for SessionSeed {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for SessionSeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SessionSeed({}…)", &self.to_hex()[..8])
    }
}

/// Règles d'échantillonnage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SamplingRules {
    /// Blocs tirés pseudo-aléatoirement (en plus du premier, du milieu et du dernier).
    pub random_samples: u32,
    /// Minimum accepté.
    pub min_samples: u32,
}

impl Default for SamplingRules {
    fn default() -> Self {
        Self {
            random_samples: 1000,
            min_samples: 100,
        }
    }
}

/// Ce que la validation attend dans chaque bloc échantillonné.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerificationTarget {
    /// Aucun canari ne subsiste (méthodes firmware).
    NoCanary,
    /// Le motif de la passe `pass` (0-based) est présent (écriture hôte sans passe de zéros).
    PatternOfPass {
        /// Index de la dernière passe pseudo-aléatoire.
        pass: u8,
    },
    /// Zéros partout, après `passes` passes pseudo-aléatoires (dont la
    /// dernière a été validée avant la passe de zéros).
    ZerosAfterPasses {
        /// Passes pseudo-aléatoires effectuées avant les zéros.
        passes: u8,
    },
}

/// Spécification de vérification d'une étape du plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VerificationSpec {
    /// Échantillonnage.
    pub sampling: SamplingRules,
    /// Attendu.
    pub target: VerificationTarget,
    /// Écrire les canaris avant l'étape (méthodes firmware).
    pub write_canaries: bool,
    /// Exiger l'absence de signatures (table de partitions, FS).
    pub require_no_signatures: bool,
}

/// Flux ChaCha20 positionné à l'octet `offset` d'un flux keyé par `key`
/// et un nonce de 12 octets dérivé de `tag` (4 octets) et `lba`.
fn stream(key: &[u8; 32], tag: [u8; 4], lba: u64) -> ChaCha20 {
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(&tag);
    nonce[4..].copy_from_slice(&lba.to_le_bytes());
    ChaCha20::new(key.into(), &nonce.into())
}

/// Remplit `block` avec le canari du bloc `lba`. Déterministe, unique par
/// (seed, lba), indistinguable d'aléatoire.
pub fn fill_canary(seed: &SessionSeed, lba: u64, block: &mut [u8]) {
    block.fill(0);
    stream(&seed.subkey(b"canary"), *b"cnry", lba).apply_keystream(block);
}

/// Canari du bloc `lba`, alloué.
pub fn canary(seed: &SessionSeed, lba: u64, block_size: u64) -> Vec<u8> {
    let mut block = vec![0u8; block_size as usize];
    fill_canary(seed, lba, &mut block);
    block
}

/// Remplit `block` avec le motif de la passe `pass` pour le bloc `lba`.
/// Le motif ne dépend que de (seed, pass, lba) : un flux ChaCha20 par bloc,
/// keyé par la passe et noncé par le LBA. Aucun `seek` dans un flux unique :
/// le compteur IETF de ChaCha20 plafonne à 256 Gio, ce qui ferait paniquer
/// la bibliothèque au-delà sur un disque de taille courante.
pub fn fill_pass_block(seed: &SessionSeed, pass: u8, lba: u64, block_size: u64, block: &mut [u8]) {
    let _ = block_size;
    block.fill(0);
    stream(&seed.subkey(&pass_label(pass)), *b"pass", lba).apply_keystream(block);
}

/// Remplit `buffer` avec les motifs de la passe `pass` des blocs contigus à
/// partir de `first_lba` : le helper écrit par gros morceaux séquentiels
/// sans recalculer bloc par bloc. Un reliquat de `buffer` plus court qu'un
/// bloc reçoit le début du motif du bloc suivant.
pub fn fill_pass_range(
    seed: &SessionSeed,
    pass: u8,
    first_lba: u64,
    block_size: u64,
    buffer: &mut [u8],
) {
    let key = seed.subkey(&pass_label(pass));
    let chunk = usize::try_from(block_size).unwrap_or(usize::MAX).max(1);
    for (i, block) in buffer.chunks_mut(chunk).enumerate() {
        let lba = first_lba.saturating_add(i as u64);
        block.fill(0);
        stream(&key, *b"pass", lba).apply_keystream(block);
    }
}

fn pass_label(pass: u8) -> [u8; 5] {
    [b'p', b'a', b's', b's', pass]
}

/// Blocs échantillonnés : premier, milieu, dernier, puis `rules.random_samples`
/// blocs tirés du flux `sample`, triés et dédoublonnés. Même seed, même
/// géométrie → mêmes blocs, avant et après l'effacement.
pub fn sample_lbas(
    seed: &SessionSeed,
    block_count: u64,
    rules: &SamplingRules,
) -> Result<Vec<u64>> {
    if block_count == 0 {
        return Err(CoreError::NoBlocks);
    }
    if rules.random_samples < rules.min_samples {
        return Err(CoreError::TooFewSamples {
            requested: rules.random_samples,
            minimum: rules.min_samples,
        });
    }
    let mut lbas = vec![0, block_count / 2, block_count - 1];
    let mut cipher = stream(&seed.subkey(b"sample"), *b"smpl", 0);
    let mut word = [0u8; 8];
    for _ in 0..rules.random_samples {
        word.fill(0);
        cipher.apply_keystream(&mut word);
        // Biais modulo < 2^-40 pour tout disque réel (block_count ≪ 2^64).
        lbas.push(u64::from_le_bytes(word) % block_count);
    }
    lbas.sort_unstable();
    lbas.dedup();
    Ok(lbas)
}

/// Ce qu'un bloc lu contient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockContent {
    /// Le canari de ce LBA, intact.
    CanaryIntact,
    /// Entièrement à zéro.
    Zeros,
    /// Le motif de la passe `pass`.
    PatternOfPass(u8),
    /// Autre chose (données résiduelles, motif d'une autre passe, bruit).
    Other,
}

/// Classe le contenu de `block` (LBA `lba`), en testant les passes `0..passes`.
pub fn classify(seed: &SessionSeed, lba: u64, block: &[u8], passes: u8) -> BlockContent {
    if block.iter().all(|&b| b == 0) {
        return BlockContent::Zeros;
    }
    let block_size = block.len() as u64;
    let mut expected = vec![0u8; block.len()];
    fill_canary(seed, lba, &mut expected);
    if expected == block {
        return BlockContent::CanaryIntact;
    }
    for pass in 0..passes {
        fill_pass_block(seed, pass, lba, block_size, &mut expected);
        if expected == block {
            return BlockContent::PatternOfPass(pass);
        }
    }
    BlockContent::Other
}

/// Un bloc lu par le helper, ou l'échec de sa lecture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleRead {
    /// LBA.
    pub lba: u64,
    /// Contenu lu ; `None` si la lecture a échoué.
    pub data: Option<Vec<u8>>,
}

/// Rapport de validation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidationReport {
    /// Blocs examinés.
    pub checked: usize,
    /// LBA illisibles.
    pub read_errors: Vec<u64>,
    /// LBA dont le canari est encore présent.
    pub canaries_intact: Vec<u64>,
    /// LBA sans le motif attendu (écriture hôte).
    pub pattern_mismatch: Vec<u64>,
    /// LBA non à zéro alors que des zéros étaient attendus.
    pub not_zeroed: Vec<u64>,
}

impl ValidationReport {
    /// Aucune anomalie.
    pub fn passed(&self) -> bool {
        self.read_errors.is_empty()
            && self.canaries_intact.is_empty()
            && self.pattern_mismatch.is_empty()
            && self.not_zeroed.is_empty()
    }
}

/// Valide des blocs lus contre `target`. `expected_lbas` est l'échantillon
/// prévu ([`sample_lbas`]) : un bloc hors échantillon est une erreur, un
/// bloc d'échantillon absent compte comme non lu.
pub fn validate(
    seed: &SessionSeed,
    block_size: u64,
    expected_lbas: &[u64],
    reads: &[SampleRead],
    target: VerificationTarget,
) -> Result<ValidationReport> {
    if block_size == 0 || block_size % 4 != 0 {
        return Err(CoreError::InvalidBlockSize(block_size));
    }
    let mut report = ValidationReport::default();
    let mut seen = std::collections::BTreeSet::new();
    let passes = match target {
        VerificationTarget::NoCanary => 0,
        VerificationTarget::PatternOfPass { pass } => pass.saturating_add(1),
        VerificationTarget::ZerosAfterPasses { passes } => passes,
    };
    for read in reads {
        if expected_lbas.binary_search(&read.lba).is_err() {
            return Err(CoreError::UnexpectedLba(read.lba));
        }
        seen.insert(read.lba);
        report.checked += 1;
        let Some(data) = &read.data else {
            report.read_errors.push(read.lba);
            continue;
        };
        if data.len() as u64 != block_size {
            return Err(CoreError::BlockSizeMismatch {
                lba: read.lba,
                expected: block_size,
                actual: data.len(),
            });
        }
        let content = classify(seed, read.lba, data, passes);
        match target {
            VerificationTarget::NoCanary => {
                if content == BlockContent::CanaryIntact {
                    report.canaries_intact.push(read.lba);
                }
            }
            VerificationTarget::PatternOfPass { pass } => {
                if content != BlockContent::PatternOfPass(pass) {
                    report.pattern_mismatch.push(read.lba);
                }
            }
            VerificationTarget::ZerosAfterPasses { .. } => {
                if content != BlockContent::Zeros {
                    report.not_zeroed.push(read.lba);
                }
            }
        }
    }
    for &lba in expected_lbas {
        if !seen.contains(&lba) {
            report.read_errors.push(lba);
        }
    }
    report.read_errors.sort_unstable();
    report.read_errors.dedup();
    Ok(report)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn seed(byte: u8) -> SessionSeed {
        SessionSeed::from_bytes([byte; 32])
    }

    #[test]
    fn seed_round_trips_hex_and_redacts_debug() {
        let s = seed(0xab);
        let hex = s.to_hex();
        assert_eq!(hex.len(), 64);
        assert_eq!(SessionSeed::from_hex(&hex), Some(s.clone()));
        assert_eq!(SessionSeed::from_hex("zz"), None);
        assert_eq!(format!("{s:?}"), "SessionSeed(abababab…)");
    }

    #[test]
    fn canaries_are_deterministic_unique_per_lba_and_per_seed() {
        let a = canary(&seed(1), 7, 512);
        assert_eq!(a, canary(&seed(1), 7, 512));
        assert_ne!(a, canary(&seed(1), 8, 512));
        assert_ne!(a, canary(&seed(2), 7, 512));
        assert_eq!(a.len(), 512);
        let ones: u32 = a.iter().map(|b| b.count_ones()).sum();
        assert!((1700..2400).contains(&ones), "{ones} bits à 1 sur 4096");
        let mut filled = vec![0xffu8; 512];
        fill_canary(&seed(1), 7, &mut filled);
        assert_eq!(filled, a);
    }

    #[test]
    fn pass_stream_is_continuous_across_blocks_and_differs_per_pass() {
        let s = seed(3);
        let mut range = vec![0u8; 4096 * 3];
        fill_pass_range(&s, 0, 10, 4096, &mut range);
        for i in 0..3u64 {
            let mut block = vec![0u8; 4096];
            fill_pass_block(&s, 0, 10 + i, 4096, &mut block);
            assert_eq!(
                &range[(i as usize) * 4096..(i as usize + 1) * 4096],
                &block[..],
                "bloc {i}"
            );
        }
        let mut other_pass = vec![0u8; 4096];
        fill_pass_block(&s, 1, 10, 4096, &mut other_pass);
        assert_ne!(&range[..4096], &other_pass[..]);
        let mut canary_block = vec![0u8; 4096];
        fill_canary(&s, 10, &mut canary_block);
        assert_ne!(
            &range[..4096],
            &canary_block[..],
            "canari et passe ne partagent pas de clé"
        );
    }

    #[test]
    fn pass_pattern_covers_disks_beyond_256_gib_without_panic() {
        // Régression : un seek dans un flux ChaCha20 IETF unique plafonnait
        // à 256 Gio (compteur 32 bits × 64 octets) et paniquait au-delà.
        let s = seed(4);
        let block_size = 4096u64;
        let last_4tib = (4u64 << 40) / block_size - 1;
        let at_256gib = (256u64 << 30) / block_size;
        let mut a = vec![0u8; 4096];
        let mut b = vec![0u8; 4096];
        fill_pass_block(&s, 0, last_4tib, block_size, &mut a);
        fill_pass_block(&s, 0, at_256gib, block_size, &mut b);
        assert_ne!(a, b);
        assert!(a.iter().any(|&x| x != 0) && b.iter().any(|&x| x != 0));
        let mut range = vec![0u8; 4096 * 2];
        fill_pass_range(&s, 0, last_4tib - 1, block_size, &mut range);
        assert_eq!(&range[4096..], &a[..]);
        assert_eq!(
            classify(&s, last_4tib, &a, 1),
            BlockContent::PatternOfPass(0)
        );
    }

    #[test]
    fn samples_cover_edges_are_sorted_and_reproducible() {
        let rules = SamplingRules::default();
        let lbas = sample_lbas(&seed(7), 1_000_000, &rules).unwrap();
        assert_eq!(lbas.first(), Some(&0));
        assert_eq!(lbas.last(), Some(&999_999));
        assert!(lbas.contains(&500_000));
        assert!(lbas.windows(2).all(|w| w[0] < w[1]));
        assert!(lbas.len() > 990 && lbas.len() <= 1003);
        assert_eq!(lbas, sample_lbas(&seed(7), 1_000_000, &rules).unwrap());
        assert_ne!(lbas, sample_lbas(&seed(8), 1_000_000, &rules).unwrap());
        let tiny = sample_lbas(&seed(7), 2, &rules).unwrap();
        assert_eq!(tiny, [0, 1]);
        assert_eq!(sample_lbas(&seed(7), 0, &rules), Err(CoreError::NoBlocks));
        let few = SamplingRules {
            random_samples: 10,
            min_samples: 100,
        };
        assert!(matches!(
            sample_lbas(&seed(7), 100, &few),
            Err(CoreError::TooFewSamples { .. })
        ));
    }

    #[test]
    fn classify_recognizes_every_content() {
        let s = seed(5);
        assert_eq!(classify(&s, 3, &[0u8; 512], 2), BlockContent::Zeros);
        assert_eq!(
            classify(&s, 3, &canary(&s, 3, 512), 2),
            BlockContent::CanaryIntact
        );
        let mut p1 = vec![0u8; 512];
        fill_pass_block(&s, 1, 3, 512, &mut p1);
        assert_eq!(classify(&s, 3, &p1, 2), BlockContent::PatternOfPass(1));
        assert_eq!(
            classify(&s, 3, &p1, 1),
            BlockContent::Other,
            "passe 1 non testée"
        );
        assert_eq!(
            classify(&s, 4, &canary(&s, 3, 512), 2),
            BlockContent::Other,
            "canari d'un autre LBA"
        );
        assert_eq!(classify(&s, 3, &[0x5a; 512], 2), BlockContent::Other);
    }

    fn reads(
        s: &SessionSeed,
        lbas: &[u64],
        make: impl Fn(u64) -> Option<Vec<u8>>,
    ) -> Vec<SampleRead> {
        let _ = s;
        lbas.iter()
            .map(|&lba| SampleRead {
                lba,
                data: make(lba),
            })
            .collect()
    }

    #[test]
    fn firmware_validation_fails_on_surviving_canary_and_passes_when_gone() {
        let s = seed(9);
        let lbas = sample_lbas(
            &s,
            10_000,
            &SamplingRules {
                random_samples: 100,
                min_samples: 100,
            },
        )
        .unwrap();
        // Tout effacé (zéros ou bruit) : réussi.
        let ok = reads(&s, &lbas, |lba| {
            Some(if lba % 2 == 0 {
                vec![0; 512]
            } else {
                vec![0x33; 512]
            })
        });
        let report = validate(&s, 512, &lbas, &ok, VerificationTarget::NoCanary).unwrap();
        assert!(report.passed());
        assert_eq!(report.checked, lbas.len());
        // Un canari survit : échec nommant le LBA.
        let kept = lbas[lbas.len() / 2];
        let partial = reads(&s, &lbas, |lba| {
            Some(if lba == kept {
                canary(&s, lba, 512)
            } else {
                vec![0; 512]
            })
        });
        let report = validate(&s, 512, &lbas, &partial, VerificationTarget::NoCanary).unwrap();
        assert!(!report.passed());
        assert_eq!(report.canaries_intact, [kept]);
    }

    #[test]
    fn host_validation_checks_last_pass_then_zeros() {
        let s = seed(11);
        let lbas = sample_lbas(
            &s,
            5_000,
            &SamplingRules {
                random_samples: 100,
                min_samples: 100,
            },
        )
        .unwrap();
        let pass1 = |lba: u64| {
            let mut b = vec![0u8; 512];
            fill_pass_block(&s, 1, lba, 512, &mut b);
            Some(b)
        };
        let report = validate(
            &s,
            512,
            &lbas,
            &reads(&s, &lbas, pass1),
            VerificationTarget::PatternOfPass { pass: 1 },
        )
        .unwrap();
        assert!(report.passed());
        // Un contrôleur paresseux a laissé des zéros : échec.
        let lazy = reads(&s, &lbas, |lba| {
            if lba == lbas[1] {
                Some(vec![0; 512])
            } else {
                pass1(lba)
            }
        });
        let report = validate(
            &s,
            512,
            &lbas,
            &lazy,
            VerificationTarget::PatternOfPass { pass: 1 },
        )
        .unwrap();
        assert_eq!(report.pattern_mismatch, [lbas[1]]);
        // Après la passe de zéros : tout doit être nul.
        let zeros = reads(&s, &lbas, |_| Some(vec![0; 512]));
        assert!(validate(
            &s,
            512,
            &lbas,
            &zeros,
            VerificationTarget::ZerosAfterPasses { passes: 2 }
        )
        .unwrap()
        .passed());
        let report = validate(
            &s,
            512,
            &lbas,
            &reads(&s, &lbas, pass1),
            VerificationTarget::ZerosAfterPasses { passes: 2 },
        )
        .unwrap();
        assert_eq!(report.not_zeroed.len(), lbas.len());
    }

    #[test]
    fn read_errors_missing_and_foreign_blocks_are_reported() {
        let s = seed(13);
        let lbas = sample_lbas(
            &s,
            1_000,
            &SamplingRules {
                random_samples: 100,
                min_samples: 100,
            },
        )
        .unwrap();
        let mut partial = reads(&s, &lbas, |_| Some(vec![0; 512]));
        partial[0].data = None;
        let dropped = partial.pop().unwrap().lba;
        let report = validate(&s, 512, &lbas, &partial, VerificationTarget::NoCanary).unwrap();
        assert_eq!(report.read_errors, [lbas[0], dropped]);
        assert!(!report.passed());
        let foreign = vec![SampleRead {
            lba: 999_999,
            data: Some(vec![0; 512]),
        }];
        assert_eq!(
            validate(&s, 512, &lbas, &foreign, VerificationTarget::NoCanary),
            Err(CoreError::UnexpectedLba(999_999))
        );
        let short = vec![SampleRead {
            lba: lbas[0],
            data: Some(vec![0; 100]),
        }];
        assert!(matches!(
            validate(&s, 512, &lbas, &short, VerificationTarget::NoCanary),
            Err(CoreError::BlockSizeMismatch { .. })
        ));
        assert_eq!(
            validate(&s, 510, &lbas, &[], VerificationTarget::NoCanary),
            Err(CoreError::InvalidBlockSize(510))
        );
    }
}
