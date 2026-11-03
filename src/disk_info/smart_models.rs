//! Modèles domaine pour l'affichage type QDiskInfo (données issues de smartctl --json).

/// État de santé affiché.
/// Base: logique QDiskInfo (Good/Caution/Bad/Unknown), enrichie d'un niveau Excellent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealthStatus {
    Excellent,
    Good,
    Caution,
    Bad,
    #[default]
    Unknown,
}

impl HealthStatus {
    pub fn label_fr(self) -> &'static str {
        match self {
            Self::Excellent => "Excellent",
            Self::Good => "Bon",
            Self::Caution => "Attention",
            Self::Bad => "Mauvais",
            Self::Unknown => "Inconnu",
        }
    }

    /// Libellé dans la langue de l'interface.
    pub fn label(self) -> String {
        crate::i18n::tr(self.label_fr())
    }

    /// Couleurs Qt utilisées par QDiskInfo (`Qt::green/yellow/red/gray`).
    pub fn color_hex(self) -> &'static str {
        match self {
            Self::Excellent => "#00FF00",
            Self::Good => "#00FF00",
            Self::Caution => "#FFFF00",
            Self::Bad => "#FF0000",
            Self::Unknown => "#A0A0A4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrStatus {
    Good,
    Caution,
    Bad,
}

impl AttrStatus {
    pub fn color_hex(self) -> &'static str {
        match self {
            Self::Good => "#00FF00",
            Self::Caution => "#FFFF00",
            Self::Bad => "#FF0000",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiskProtocol {
    Ata,
    Nvme,
    Scsi,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub struct DiskIdentity {
    pub model_name: String,
    pub serial_number: String,
    pub firmware: String,
    pub device_node: String,
    pub device_type: String,
    pub protocol: DiskProtocol,
    pub capacity_bytes: Option<u64>,
    pub rotation_rate: Option<i64>,
    pub logical_block_size: Option<u64>,
}

impl DiskIdentity {
    pub fn is_ssd_like(&self) -> bool {
        matches!(self.rotation_rate, Some(0)) || self.protocol == DiskProtocol::Nvme
    }
}

#[derive(Debug, Clone, Default)]
pub struct TemperatureInfo {
    pub current_celsius: Option<i32>,
    pub lifetime_min_celsius: Option<i32>,
    pub lifetime_max_celsius: Option<i32>,
    pub drive_trip_celsius: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct SmartAttributeRow {
    pub id: u32,
    /// Nom logique (smartctl utile, sinon mapping ID) pour matching / vie restante.
    pub name: String,
    /// Libellé tableau (espaces, ou « Vendor Specific (0xAE) »).
    pub display_name: String,
    pub value: i32,
    pub worst: i32,
    pub threshold: i32,
    pub raw_value: u64,
    pub raw_string: String,
    pub status: AttrStatus,
}

#[derive(Debug, Clone)]
pub struct NvmeSmartEntry {
    pub key: String,
    pub display_name: String,
    pub raw: i64,
    pub status: AttrStatus,
}

#[derive(Debug, Clone)]
pub struct ScsiErrorCounters {
    pub operation: String,
    pub errors_corrected_by_eccfast: i64,
    pub errors_corrected_by_eccdelayed: i64,
    pub errors_corrected_by_rereads_rewrites: i64,
    pub total_errors_corrected: i64,
    pub correction_algorithm_invocations: i64,
    pub gigabytes_processed: String,
    pub total_uncorrected_errors: i64,
    pub status: AttrStatus,
}

#[derive(Debug, Clone)]
pub struct SelfTestEntry {
    pub test_type: String,
    pub status: String,
    pub power_on_hours: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct HealthFlags {
    pub health_passed: bool,
    pub caution_flag: bool,
    pub bad_flag: bool,
    pub model_known: bool,
    pub smart_status_known: bool,
}

/// Pourcentage affiché sous Health Status.
/// Wear = usure constructeur (QDiskInfo) ; SecMetScore = indice interne fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthPercent {
    Wear(u32),
    SecMetScore(u32),
}

impl HealthPercent {
    pub fn value(self) -> u32 {
        match self {
            Self::Wear(v) | Self::SecMetScore(v) => v,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiskSmartReport {
    pub identity: DiskIdentity,
    pub smart_passed: Option<bool>,
    pub health_flags: HealthFlags,
    pub temperature: TemperatureInfo,
    pub health: HealthStatus,
    /// Vie restante constructeur (SSD/NVMe) — logique QDiskInfo.
    pub remaining_life_percent: Option<u32>,
    /// Affichage unifié : wear prioritaire, sinon indice SecMet (jamais Unknown).
    pub health_percent: Option<HealthPercent>,
    pub power_on_hours: Option<u64>,
    pub power_cycle_count: Option<u64>,
    pub total_reads_mb: Option<i64>,
    pub total_writes_mb: Option<i64>,
    pub attributes: Vec<SmartAttributeRow>,
    pub nvme_entries: Vec<NvmeSmartEntry>,
    pub scsi_errors: Vec<ScsiErrorCounters>,
    pub self_tests: Vec<SelfTestEntry>,
    pub scsi_grown_defect_list: Option<i64>,
    /// Sortie texte smartctl (pour seuils NVMe Warning/Critical Comp. Temp.).
    pub smartctl_output_lines: Vec<String>,
}

impl Default for DiskSmartReport {
    fn default() -> Self {
        Self {
            identity: DiskIdentity::default(),
            smart_passed: None,
            health_flags: HealthFlags::default(),
            temperature: TemperatureInfo::default(),
            health: HealthStatus::Unknown,
            remaining_life_percent: None,
            health_percent: None,
            power_on_hours: None,
            power_cycle_count: None,
            total_reads_mb: None,
            total_writes_mb: None,
            attributes: Vec::new(),
            nvme_entries: Vec::new(),
            scsi_errors: Vec::new(),
            self_tests: Vec::new(),
            scsi_grown_defect_list: None,
            smartctl_output_lines: Vec::new(),
        }
    }
}

/// Page SMART / Health NVMe (valeurs optionnelles, logique QDiskInfo).
#[derive(Debug, Clone, Default)]
pub struct NvmeHealthLog {
    pub critical_warning: Option<i64>,
    pub temperature: Option<i64>,
    pub available_spare: Option<i64>,
    pub available_spare_threshold: Option<i64>,
    pub percentage_used: Option<i64>,
    pub data_units_read: Option<f64>,
    pub data_units_written: Option<f64>,
    pub host_reads: Option<i64>,
    pub host_writes: Option<i64>,
    pub controller_busy_time: Option<i64>,
    pub power_cycles: Option<i64>,
    pub power_on_hours: Option<i64>,
    pub unsafe_shutdowns: Option<i64>,
    pub media_errors: Option<i64>,
    pub num_err_log_entries: Option<i64>,
    pub warning_temp_time: Option<i64>,
    pub critical_comp_time: Option<i64>,
}
