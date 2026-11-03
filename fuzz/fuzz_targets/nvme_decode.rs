//! Identify Controller / Namespace, journal SMART et journal Sanitize NVMe.
#![no_main]
use libfuzzer_sys::fuzz_target;
use secmet_device::nvme::decode;

fuzz_target!(|data: &[u8]| {
    let _ = decode::identify_controller(data);
    let _ = decode::identify_namespace(data);
    if let Ok(health) = decode::health(data) {
        let _ = health.read_only();
    }
    let _ = decode::sanitize_log(data);
});
