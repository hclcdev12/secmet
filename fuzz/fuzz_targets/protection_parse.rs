//! /proc/self/mountinfo et /proc/swaps arbitraires.
#![no_main]
use libfuzzer_sys::fuzz_target;
use secmet_device::protection::{parse_mountinfo, parse_swaps};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let _ = parse_mountinfo(text);
    let _ = parse_swaps(text);
});
