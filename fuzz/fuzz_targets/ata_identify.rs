//! Secteur IDENTIFY DEVICE arbitraire.
#![no_main]
use libfuzzer_sys::fuzz_target;
use secmet_device::ata::identify;

fuzz_target!(|data: &[u8]| {
    if let Ok(id) = identify::decode(data) {
        let _ = format!("{:?}", id.media);
        let _ = (id.model.len(), id.serial.len(), id.firmware.len());
    }
});
