//! Classification d'un bloc relu (canari, motif de passe, zéros, autre)
//! avec seed, LBA et nombre de passes arbitraires ; cohérence avec le
//! générateur : un canari produit doit être reconnu comme tel.
#![no_main]
use libfuzzer_sys::fuzz_target;
use secmet_core::canary::{classify, fill_canary, BlockContent, SessionSeed};

fuzz_target!(|data: &[u8]| {
    if data.len() < 41 {
        return;
    }
    let mut seed_bytes = [0u8; 32];
    seed_bytes.copy_from_slice(&data[..32]);
    let seed = SessionSeed::from_bytes(seed_bytes);
    let lba = u64::from_le_bytes(data[32..40].try_into().unwrap_or([0; 8]));
    let passes = data[40] % 8;
    let block = &data[41..];
    let _ = classify(&seed, lba, block, passes);

    // Propriété : un canari généré pour (seed, lba) est reconnu.
    if block.len() >= 512 && block.len() % 512 == 0 {
        let mut generated = vec![0u8; block.len()];
        fill_canary(&seed, lba, &mut generated);
        assert_eq!(classify(&seed, lba, &generated, passes), BlockContent::CanaryIntact);
    }
});
