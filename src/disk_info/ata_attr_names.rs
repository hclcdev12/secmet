//! Noms d'attributs ATA SMART : smartctl, table connue, fallback vendor-specific.

/// Indique si le nom fourni par smartctl est exploitable pour l'affichage / le matching.
pub fn is_useful_smartctl_name(name: &str) -> bool {
    let t = name.trim();
    if t.is_empty() {
        return false;
    }
    !matches!(
        t,
        "Unknown_Attribute"
            | "Unknown Attribute"
            | "Unknown"
            | "Vendor_Specific"
            | "Vendor Specific"
    )
}

/// Libellé d'affichage pour le tableau (espaces, hex vendor-specific).
pub fn ata_attr_display_name(id: u32, smartctl_name: &str) -> String {
    if is_useful_smartctl_name(smartctl_name) {
        return humanize_attr_name(smartctl_name);
    }
    if let Some(mapped) = known_ata_attr_name(id) {
        return humanize_attr_name(mapped);
    }
    format!("Vendor Specific (0x{id:02X})")
}

/// Nom logique effectif pour le matching (vie restante, I/O, etc.).
pub fn effective_ata_attr_name(id: u32, smartctl_name: &str) -> String {
    if is_useful_smartctl_name(smartctl_name) {
        return smartctl_name.to_string();
    }
    known_ata_attr_name(id)
        .map(str::to_string)
        .unwrap_or_default()
}

fn humanize_attr_name(name: &str) -> String {
    name.replace('_', " ")
}

/// Attributs ATA standard + IDs SSD vendor courants (CrystalDiskInfo / smartmontools).
pub fn known_ata_attr_name(id: u32) -> Option<&'static str> {
    Some(match id {
        // --- ATA / ACS courants ---
        1 => "Raw_Read_Error_Rate",
        2 => "Throughput_Performance",
        3 => "Spin_Up_Time",
        4 => "Start_Stop_Count",
        5 => "Reallocated_Sector_Ct",
        6 => "Read_Channel_Margin",
        7 => "Seek_Error_Rate",
        8 => "Seek_Time_Performance",
        9 => "Power_On_Hours",
        10 => "Spin_Retry_Count",
        11 => "Calibration_Retry_Count",
        12 => "Power_Cycle_Count",
        13 => "Read_Soft_Error_Rate",
        22 => "Current_Helium_Level",
        170 => "Available_Reservd_Space",
        171 => "Program_Fail_Count",
        172 => "Erase_Fail_Count",
        173 => "Wear_Leveling_Count",
        174 => "Unexpected_Power_Loss_Count",
        175 => "Program_Fail_Count_Chip",
        176 => "Erase_Fail_Count_Chip",
        177 => "Wear_Leveling_Count",
        178 => "Used_Rsvd_Blk_Cnt_Chip",
        179 => "Used_Rsvd_Blk_Cnt_Tot",
        180 => "Unused_Rsvd_Blk_Cnt_Tot",
        181 => "Program_Fail_Cnt_Total",
        182 => "Erase_Fail_Count_Total",
        183 => "Runtime_Bad_Block",
        184 => "End-to-End_Error",
        187 => "Reported_Uncorrect",
        188 => "Command_Timeout",
        189 => "High_Fly_Writes",
        190 => "Airflow_Temperature_Cel",
        191 => "G-Sense_Error_Rate",
        192 => "Power-Off_Retract_Count",
        193 => "Load_Cycle_Count",
        194 => "Temperature_Celsius",
        195 => "Hardware_ECC_Recovered",
        196 => "Reallocated_Event_Count",
        197 => "Current_Pending_Sector",
        198 => "Offline_Uncorrectable",
        199 => "UDMA_CRC_Error_Count",
        200 => "Multi_Zone_Error_Rate",
        201 => "Soft_Read_Error_Rate",
        202 => "Percent_Lifetime_Remain",
        206 => "Flying_Height",
        210 => "RAIFF_Error_Rate",
        220 => "Disk_Shift",
        222 => "Loaded_Hours",
        223 => "Load_Retry_Count",
        224 => "Load_Friction",
        226 => "Load-in_Time",
        240 => "Head_Flying_Hours",
        241 => "Total_LBAs_Written",
        242 => "Total_LBAs_Read",
        245 => "Percent_Lifetime_Remaining",
        246 => "Total_LBAs_Written",
        247 => "Host_Program_NAND_Pages_Count",
        248 => "FTL_Program_NAND_Pages_Count",

        // --- SSD vendor-specific fréquents (IDs hex des captures) ---
        100 => "Total_Erase_Count",       // 0x64
        160 => "Uncorrectable_Error_Cnt", // 0xA0
        161 => "Valid_Spare_Block_Cnt",   // 0xA1
        163 => "Initial_Bad_Block_Count", // 0xA3
        164 => "Total_Erase_Count",       // 0xA4
        165 => "Max_Erase_Count",         // 0xA5
        166 => "Min_Erase_Count",         // 0xA6
        167 => "Average_Erase_Count",     // 0xA7
        168 => "SATA_PHY_Error_Count",    // 0xA8 (souvent)
        169 => "Remaining_Lifetime_Perc", // 0xA9
        231 => "SSD_Life_Left",           // 0xE7
        232 => "Available_Reservd_Space", // 0xE8
        233 => "Media_Wearout_Indicator", // 0xE9
        234 => "Average_Erase_Count",     // 0xEA
        235 => "Max_Erase_Count",         // 0xEB

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn useful_name_kept() {
        assert_eq!(
            ata_attr_display_name(5, "Reallocated_Sector_Ct"),
            "Reallocated Sector Ct"
        );
        assert!(is_useful_smartctl_name("Program_Fail_Count_Chip"));
    }

    #[test]
    fn unknown_mapped_ids_from_screenshots() {
        assert_eq!(
            ata_attr_display_name(0x64, "Unknown_Attribute"),
            "Total Erase Count"
        );
        assert_eq!(
            ata_attr_display_name(0xA8, "Unknown_Attribute"),
            "SATA PHY Error Count"
        );
        assert_eq!(
            ata_attr_display_name(0xA9, "Unknown_Attribute"),
            "Remaining Lifetime Perc"
        );
        assert_eq!(
            ata_attr_display_name(0xAB, "Unknown_Attribute"),
            "Program Fail Count"
        );
        assert_eq!(
            ata_attr_display_name(0xAC, "Unknown_Attribute"),
            "Erase Fail Count"
        );
        assert_eq!(
            ata_attr_display_name(0xAE, "Unknown_Attribute"),
            "Unexpected Power Loss Count"
        );
        assert_eq!(
            ata_attr_display_name(0xA0, "Unknown_Attribute"),
            "Uncorrectable Error Cnt"
        );
    }

    #[test]
    fn unknown_unmapped_is_vendor_specific() {
        assert_eq!(
            ata_attr_display_name(0xFA, "Unknown_Attribute"),
            "Vendor Specific (0xFA)"
        );
        assert_eq!(ata_attr_display_name(99, ""), "Vendor Specific (0x63)");
    }

    #[test]
    fn smartctl_name_wins_over_mapping() {
        assert_eq!(
            ata_attr_display_name(0xAF, "Program_Fail_Count_Chip"),
            "Program Fail Count Chip"
        );
    }

    #[test]
    fn effective_name_for_unknown_a9() {
        assert_eq!(
            effective_ata_attr_name(0xA9, "Unknown_Attribute"),
            "Remaining_Lifetime_Perc"
        );
    }
}
