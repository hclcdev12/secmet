//! JSON d'attestation arbitraire : désérialisation, canonisation, vérification,
//! bornage des listes, rendu du texte QR.
#![no_main]
use libfuzzer_sys::fuzz_target;
use secmet_attest::{canonical, export, qr, sign::SignedAttestation};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let _ = export::verify_json(text, &[]);
    if let Ok(signed) = serde_json::from_str::<SignedAttestation>(text) {
        let value = serde_json::to_value(&signed.payload).unwrap_or_default();
        let _ = canonical::to_string(&value);
        let _ = signed.payload.validate();
        let bounded = signed.payload.clone().bounded();
        let _ = bounded.validate();
        let _ = qr::encode_text(&signed);
    }
});
