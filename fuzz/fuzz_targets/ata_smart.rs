//! SMART READ DATA / READ THRESHOLDS / RETURN STATUS arbitraires.
#![no_main]
use libfuzzer_sys::fuzz_target;
use secmet_device::ata::smart;

fuzz_target!(|data: &[u8]| {
    let _ = smart::decode_attributes(data);
    let _ = smart::decode_attribute_table(data);
    let _ = smart::decode_thresholds(data);
    if data.len() >= 2 {
        let _ = smart::status_from_registers(data[0], data[1]);
    }
});
