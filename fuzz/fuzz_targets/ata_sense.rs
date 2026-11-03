//! Sense SCSI arbitraire : registres ATA, clé/ASC/ASCQ, état SANITIZE.
#![no_main]
use libfuzzer_sys::fuzz_target;
use secmet_device::ata::{registers_from_sense, sense_code, SanitizeRefusal, SanitizeStatus};

fuzz_target!(|data: &[u8]| {
    if let Some(code) = sense_code(data) {
        let _ = code.rejects_cdb();
    }
    if let Some(registers) = registers_from_sense(data) {
        let _ = registers.failed();
        let _ = registers.lba_mid_high();
        let status = SanitizeStatus::from_registers(registers.count, registers.lba);
        let _ = status.percent();
        let refusal = SanitizeRefusal::from_lba(registers.lba);
        let _ = SanitizeStatus::from_refusal(refusal);
        let _ = refusal.to_string();
    }
});
