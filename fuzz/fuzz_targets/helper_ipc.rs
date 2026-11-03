//! Trames NDJSON reçues par le helper (bornes, JSON invalide, commandes).
#![no_main]
use libfuzzer_sys::fuzz_target;
use secmet_helper::ipc::read_request;
use std::io::BufReader;

fuzz_target!(|data: &[u8]| {
    let mut reader = BufReader::new(data);
    // Plusieurs trames possibles dans une entrée : on lit jusqu'à épuisement.
    for _ in 0..4 {
        match read_request(&mut reader) {
            Ok(request) => {
                let _ = format!("{:?}", request.cmd);
            }
            Err(_) => break,
        }
    }
});
