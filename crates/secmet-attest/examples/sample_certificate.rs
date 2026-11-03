//! Génère un lot de démonstration :
//! `cargo run -p secmet-attest --example sample_certificate -- <dossier> [fr|en|de]`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use secmet_attest::export;
use secmet_attest::lang::Lang;
use secmet_attest::model::*;
use secmet_attest::sign::SigningIdentity;

fn main() {
    let dir = std::env::args().nth(1).expect("dossier de sortie");
    let lang = match std::env::args().nth(2).as_deref() {
        Some("en") => Lang::En,
        Some("de") => Lang::De,
        _ => Lang::Fr,
    };
    let identity = SigningIdentity::from_seed(&[42u8; 32]);
    let base = Attestation {
        version: SCHEMA_VERSION,
        certificate_id: "SEC-20261008-DEMO-sdb".into(),
        session_id: "SEC-20261008-DEMO".into(),
        issued_at: "2026-10-08 11:30:00".into(),
        operator: "Jean Dupont".into(),
        software: "secmet 0.2.0".into(),
        disk: DiskFacts {
            name: "sdb".into(),
            model: "Samsung SSD 870 EVO 500GB".into(),
            serial: "S6PYNX0T305592W".into(),
            firmware: "SVT02B6Q".into(),
            media: "SSD SATA".into(),
            transport: "sata".into(),
            capacity: "500 GB".into(),
            size_bytes: Some(500_107_862_016),
            block_size: Some(512),
        },
        erasure: ErasureFacts {
            method_code: "ATA_SANITIZE_BLOCK_ERASE".into(),
            method_label: "ATA SANITIZE BLOCK ERASE EXT".into(),
            nist_level: NistLevel::Purge,
            policy: "nist-800-88r1".into(),
            outcome: Outcome::Success,
            started_at: Some("2026-10-08 11:00:00".into()),
            finished_at: Some("2026-10-08 11:04:12".into()),
            duration_seconds: Some(252),
            caveats: vec![],
        },
        validation: ValidationFacts {
            kind: "canaries".into(),
            seed: Some("ab".repeat(32)),
            samples: 1003,
            anomalies: 0,
            evidence_sha256: "9f".repeat(32),
            residual_signatures: vec![],
            verify_status: "OK".into(),
        },
        bench: BenchFacts {
            manufacturer: "Dell Inc.".into(),
            model: "OptiPlex 7090".into(),
            serial: "ABC1234".into(),
        },
        report_sha256: None,
    };
    let mut hdd = base.clone();
    hdd.certificate_id = "SEC-20261008-DEMO-sdc".into();
    hdd.disk = DiskFacts {
        name: "sdc".into(),
        model: "WDC WD10EZEX-08WN4A0".into(),
        serial: "WD-WCC6Y4ABC123".into(),
        firmware: "01.01A01".into(),
        media: "HDD-USB".into(),
        transport: "usb".into(),
        capacity: "1 TB".into(),
        size_bytes: Some(1_000_204_886_016),
        block_size: Some(512),
    };
    hdd.erasure.method_code = "HOST_OVERWRITE_1_ZERO".into();
    hdd.erasure.method_label = "Écriture hôte 1 passe pseudo-aléatoire + zéros".into();
    hdd.erasure.nist_level = NistLevel::Clear;
    hdd.erasure.duration_seconds = Some(9_812);
    hdd.erasure.caveats =
        vec!["pont USB : aucune commande firmware envoyée, écriture hôte uniquement".into()];
    let mut failed = base.clone();
    failed.certificate_id = "SEC-20261008-DEMO-nvme0n1".into();
    failed.disk.name = "nvme0n1".into();
    failed.disk.serial = "NVME-FAIL-001".into();
    failed.disk.media = "SSD NVMe".into();
    failed.disk.transport = "nvme".into();
    failed.erasure.method_code = "NVME_SANITIZE_BLOCK_ERASE".into();
    failed.erasure.outcome = Outcome::Failed;
    failed.erasure.nist_level = NistLevel::NotClassified;
    failed.validation.anomalies = 3;
    failed.validation.residual_signatures = vec!["PTTYPE=gpt".into()];
    let out = export::export_with_logo(
        &identity,
        std::path::Path::new(&dir),
        "SEC-20261008-DEMO-lot.pdf",
        &[base, hdd, failed],
        Some("SEC-20261008-DEMO-lot"),
        None,
        lang,
    )
    .unwrap();
    println!("{}", out.pdf.display());
    for j in &out.json {
        println!("{}", j.display());
    }
}
