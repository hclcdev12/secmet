//! Module d'info disque style QDiskInfo, alimenté par le scan du helper
//! (IDENTIFY / SMART ATA ou page Health NVMe), sans smartctl.

pub mod ata_attr_names;
pub mod disk_health;
pub mod disk_temperature;
pub mod fetch;
pub mod from_helper;
pub mod smart_models;
pub mod ui;

pub use disk_health::{compute_health, resolve_health_percent, secmet_health_score, HealthOptions};
pub use disk_temperature::{resolve_thresholds, temperature_status, TemperatureThresholds};
pub use fetch::fetch_disk_smart_report;
pub use from_helper::report_from_scan;
pub use smart_models::{
    AttrStatus, DiskIdentity, DiskProtocol, DiskSmartReport, HealthFlags, HealthPercent,
    HealthStatus, TemperatureInfo,
};
pub use ui::health_ring::health_info_button_class;
