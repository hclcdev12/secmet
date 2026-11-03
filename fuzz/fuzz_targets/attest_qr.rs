//! Texte de QR arbitraire : Base45, zlib borné, JSON, vérification.
#![no_main]
use libfuzzer_sys::fuzz_target;
use secmet_attest::{base45, qr, sign};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let _ = base45::decode(text);
    if let Ok(signed) = qr::decode_text(text) {
        let _ = sign::verify(&signed, &[]);
        let _ = qr::encode_text(&signed);
    }
});
