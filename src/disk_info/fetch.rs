//! Lecture SMART via le helper (`ScanDevice`), sans smartctl.

use crate::disk_info::from_helper::report_from_scan;
use crate::disk_info::smart_models::DiskSmartReport;
use crate::helper_client;

/// Scanne le disque (`/dev/sdb` ou `sdb`) et construit le rapport d'affichage.
pub fn fetch_disk_smart_report(disk: &str) -> Result<DiskSmartReport, String> {
    let helper = helper_client::ensure()?;
    let scan = helper_client::scan_device(&helper, disk, helper_client::default_preferences())
        .map_err(String::from)?;
    report_from_scan(&scan)
}
