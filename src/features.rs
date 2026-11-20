//! HID++ 2.0 feature IDs and names
//! Names follow Solaar; features it does not name are shown as "Unknown"

pub const FEATURE_SET: u16 = 0x0001;
pub const DEVICE_INFO: u16 = 0x0003;
pub const DEVICE_NAME: u16 = 0x0005;
pub const UNIFIED_BATTERY: u16 = 0x1004;
pub const CHANGE_HOST: u16 = 0x1814;
pub const HOSTS_INFO: u16 = 0x1815;
pub const HAPTIC: u16 = 0x19b0;
pub const FORCE_SENSING_BUTTON: u16 = 0x19c0;
pub const REPROG_CONTROLS_V4: u16 = 0x1b04;
pub const SMART_SHIFT: u16 = 0x2110;
pub const SMART_SHIFT_ENHANCED: u16 = 0x2111;
pub const HIRES_WHEEL: u16 = 0x2121;
pub const THUMB_WHEEL: u16 = 0x2150;
pub const ADJUSTABLE_DPI: u16 = 0x2201;

pub fn feature_name(id: u16) -> &'static str {
    match id {
        0x0000 => "Root",
        0x0001 => "FeatureSet",
        0x0003 => "DeviceInfo",
        0x0005 => "DeviceName",
        0x0007 => "DeviceFriendlyName",
        0x0011 => "PropertyAccess",
        0x0020 => "ConfigChange",
        0x0021 => "CryptoId",
        0x00c3 => "DfuControl",
        0x1004 => "UnifiedBattery",
        0x1802 => "DeviceReset",
        0x1805 => "OobState",
        0x1806 => "ConfigurableDeviceProperties",
        0x1814 => "ChangeHost",
        0x1815 => "HostsInfo",
        0x1816 => "BleProPrePairing",
        0x19b0 => "Haptic",
        0x19c0 => "ForceSensingButton",
        0x1b04 => "ReprogControlsV4",
        0x1d4b => "WirelessDeviceStatus",
        0x1e00 => "EnableHiddenFeatures",
        0x2110 => "SmartShift",
        0x2111 => "SmartShiftEnhanced",
        0x2121 => "HiResWheel",
        0x2150 => "ThumbWheel",
        0x2201 => "AdjustableDpi",
        0x2250 => "XYStats",
        0x2251 => "WheelStats",
        _ => "Unknown",
    }
}

/// Flags of the feature type (third byte of the FeatureSet.getFeatureID response)
pub fn feature_type_flags(t: u8) -> Vec<&'static str> {
    [
        (0x80, "obsolete"),
        (0x40, "hidden"),
        (0x20, "engineering"),
        (0x10, "manufacturing-deactivatable"),
        (0x08, "compliance-deactivatable"),
    ]
    .into_iter()
    .filter(|(bit, _)| t & bit != 0)
    .map(|(_, name)| name)
    .collect()
}

pub fn fmt_id(id: u16) -> String {
    format!("0x{id:04x}")
}
