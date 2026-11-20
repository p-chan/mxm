//! Reading and writing each feature

use anyhow::{Result, anyhow, bail};
use serde::Serialize;

use crate::controls::control_flags;
use crate::features as f;
use crate::hidpp::{FeatureCall, HidppError, Params};

pub fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}

pub fn s16_at(b: &[u8], o: usize) -> i16 {
    i16::from_be_bytes([b[o], b[o + 1]])
}

fn ascii(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].iter().map(|&c| c as char).collect()
}

// ---- FeatureSet (0x0001) ----

#[derive(Debug, Serialize)]
pub struct FeatureEntry {
    pub index: u8,
    pub id: u16,
    #[serde(rename = "type")]
    pub kind: u8,
    pub version: u8,
}

pub fn list_features(d: &mut impl FeatureCall) -> Result<Vec<FeatureEntry>> {
    let count = d.call(f::FEATURE_SET, 0, &[])?[0];
    let mut out = vec![FeatureEntry {
        index: 0,
        id: 0,
        kind: 0,
        version: 0,
    }];
    for i in 1..=count {
        let r = d.call(f::FEATURE_SET, 1, &[i])?;
        out.push(FeatureEntry {
            index: i,
            id: u16_at(&r, 0),
            kind: r[2],
            version: r[3],
        });
    }
    Ok(out)
}

// ---- DeviceName (0x0005) ----

const DEVICE_KINDS: &[&str] = &[
    "keyboard",
    "remote-control",
    "numpad",
    "mouse",
    "trackpad",
    "trackball",
    "presenter",
    "receiver",
    "headset",
    "webcam",
    "steering-wheel",
    "joystick",
    "gamepad",
    "dock",
    "speaker",
    "microphone",
    "illumination-light",
    "programmable-controller",
    "car-sim-pedals",
    "adapter",
];

#[derive(Debug, Serialize, PartialEq)]
pub struct Name {
    pub name: String,
    pub kind: String,
}

pub fn get_name(d: &mut impl FeatureCall) -> Result<Name> {
    let len = d.call(f::DEVICE_NAME, 0, &[])?[0] as usize;
    let mut name = String::new();
    while name.len() < len {
        let chunk = ascii(&d.call(f::DEVICE_NAME, 1, &[name.len() as u8])?);
        if chunk.is_empty() {
            break;
        }
        name.push_str(&chunk);
    }
    name.truncate(len);
    let kind = d.call(f::DEVICE_NAME, 2, &[])?[0] as usize;
    let kind = DEVICE_KINDS
        .get(kind)
        .map_or_else(|| format!("unknown({kind})"), |s| (*s).to_string());
    Ok(Name { name, kind })
}

// ---- DeviceInfo (0x0003) ----

const ENTITY_TYPES: &[&str] = &[
    "firmware",
    "bootloader",
    "hardware",
    "touchpad",
    "optical-sensor",
    "softdevice",
    "rf-companion",
    "factory-app",
    "rgb-effect",
    "motor-drive",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Firmware {
    #[serde(rename = "type")]
    pub kind: String,
    pub version: String,
    pub active: bool,
    pub pid: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub unit_id: String,
    pub transports: Vec<&'static str>,
    pub model_ids: Vec<String>,
    pub serial: Option<String>,
    pub firmware: Vec<Firmware>,
}

pub fn get_device_info(d: &mut impl FeatureCall) -> Result<DeviceInfo> {
    let r = d.call(f::DEVICE_INFO, 0, &[])?;
    let transport = u16_at(&r, 5);
    let transports = [
        (0x01, "bluetooth"),
        (0x02, "ble"),
        (0x04, "unifying"),
        (0x08, "usb"),
    ]
    .into_iter()
    .filter(|(b, _)| transport & b != 0)
    .map(|(_, n)| n)
    .collect();
    let model_ids = [7, 9, 11]
        .into_iter()
        .map(|o| u16_at(&r, o))
        .filter(|&v| v != 0)
        .map(|v| format!("{v:04x}"))
        .collect();
    let mut firmware = Vec::new();
    for i in 0..r[0] {
        let fw = d.call(f::DEVICE_INFO, 1, &[i])?;
        let prefix = ascii(&fw[1..4]).trim().to_string();
        firmware.push(Firmware {
            kind: ENTITY_TYPES
                .get(fw[0] as usize)
                .map_or_else(|| format!("type{}", fw[0]), |s| (*s).to_string()),
            version: format!(
                "{prefix}{:02x}.{:02x}_{:02x}{:02x}",
                fw[4], fw[5], fw[6], fw[7]
            ),
            active: fw[8] & 0x01 == 1,
            pid: format!("{:04x}", u16_at(&fw, 9)),
        });
    }
    let serial = get_serial(d)?;
    Ok(DeviceInfo {
        unit_id: crate::hidpp::hex(&r[1..5]).replace(' ', ""),
        transports,
        model_ids,
        serial,
        firmware,
    })
}

/// The serial number, or None if the device has none. Needs fewer requests than `get_device_info`
pub fn get_serial(d: &mut impl FeatureCall) -> Result<Option<String>> {
    let r = d.call(f::DEVICE_INFO, 0, &[])?;
    if r[14] & 0x01 == 0 {
        return Ok(None);
    }
    Ok(Some(ascii(&d.call(f::DEVICE_INFO, 2, &[])?[..12])))
}

// ---- UnifiedBattery (0x1004) ----

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub percent: u8,
    pub level: &'static str,
    pub status: String,
    pub external_power: bool,
}

pub fn get_battery(d: &mut impl FeatureCall) -> Result<Battery> {
    Ok(parse_battery(&d.call(f::UNIFIED_BATTERY, 1, &[])?))
}

pub fn parse_battery(r: &[u8]) -> Battery {
    let lv = r[1];
    let level = if lv & 0x08 != 0 {
        "full"
    } else if lv & 0x04 != 0 {
        "good"
    } else if lv & 0x02 != 0 {
        "low"
    } else if lv & 0x01 != 0 {
        "critical"
    } else {
        "unknown"
    };
    let status = match r[2] {
        0 => "discharging".into(),
        1 => "charging".into(),
        2 => "charging-slow".into(),
        3 => "full".into(),
        4 => "error".into(),
        n => format!("unknown({n})"),
    };
    Battery {
        percent: r[0],
        level,
        status,
        external_power: r[3] != 0,
    }
}

// ---- AdjustableDpi (0x2201) ----

#[derive(Debug, Serialize, PartialEq)]
pub struct DpiRange {
    pub min: u16,
    pub max: u16,
    pub step: u16,
}

#[derive(Debug, Serialize)]
pub struct Dpi {
    pub dpi: u16,
    pub default: u16,
    pub supported: Vec<u16>,
    pub range: Option<DpiRange>,
}

pub fn get_dpi(d: &mut impl FeatureCall, sensor: u8) -> Result<Dpi> {
    let r = d.call(f::ADJUSTABLE_DPI, 2, &[sensor])?;
    let list = d.call(f::ADJUSTABLE_DPI, 1, &[sensor])?;
    let mut values: Vec<u16> = Vec::new();
    let mut range = None;
    let mut o = 1;
    while o + 1 < list.len() {
        let v = u16_at(&list, o);
        if v == 0 {
            break;
        }
        if v >> 13 == 0b111 {
            // 0xE000 | step means "from the previous value to the next value, in steps of `step`"
            let step = v & 0x1fff;
            let min = values.last().copied().unwrap_or(0);
            let max = if o + 3 < list.len() {
                u16_at(&list, o + 2)
            } else {
                min
            };
            range = Some(DpiRange { min, max, step });
            let mut x = min + step;
            while step > 0 && x < max {
                values.push(x);
                x += step;
            }
        } else {
            values.push(v);
        }
        o += 2;
    }
    Ok(Dpi {
        dpi: u16_at(&r, 1),
        default: u16_at(&r, 3),
        supported: values,
        range,
    })
}

pub fn set_dpi(d: &mut impl FeatureCall, dpi: u16, sensor: u8) -> Result<()> {
    let supported = get_dpi(d, sensor)?.supported;
    if !supported.contains(&dpi) {
        let nearest = supported
            .iter()
            .min_by_key(|&&v| v.abs_diff(dpi))
            .copied()
            .unwrap_or(dpi);
        bail!(
            "DPI {dpi} is not supported (nearest: {nearest}, range: {}-{})",
            supported.first().unwrap_or(&0),
            supported.last().unwrap_or(&0)
        );
    }
    let [hi, lo] = dpi.to_be_bytes();
    d.call(f::ADJUSTABLE_DPI, 3, &[sensor, hi, lo])?;
    Ok(())
}

// ---- SmartShift (0x2110) / SmartShiftEnhanced (0x2111) ----
// 0x2111
// getRatchetControlMode: [wheelMode (1=freespin, 2=ratchet), autoDisengage (1..254, 255 = SmartShift off), torque (1..100)]
// setRatchetControlMode: same layout; 0 means "unchanged"

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum WheelMode {
    Freespin,
    Ratchet,
}

/// SmartShift threshold. Off disables SmartShift (the wheel stays in wheelMode)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Threshold {
    Value(u8),
    Off,
}

impl Serialize for Threshold {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Value(v) => s.serialize_u8(*v),
            Self::Off => s.serialize_str("off"),
        }
    }
}

impl std::str::FromStr for Threshold {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        if s == "off" {
            return Ok(Self::Off);
        }
        match s.parse::<u8>() {
            Ok(v @ 1..=254) => Ok(Self::Value(v)),
            _ => Err("threshold must be 1-254 or off".into()),
        }
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SmartShiftDefaults {
    pub threshold: u8,
    /// None on devices without torque (SmartShift 0x2110)
    pub torque: Option<u8>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SmartShift {
    pub mode: WheelMode,
    pub threshold: Threshold,
    /// None on devices without torque (SmartShift 0x2110)
    pub torque: Option<u8>,
    pub defaults: SmartShiftDefaults,
    pub tunable_torque: bool,
}

fn parse_mode(v: u8) -> WheelMode {
    if v == 1 {
        WheelMode::Freespin
    } else {
        WheelMode::Ratchet
    }
}

fn parse_threshold(v: u8) -> Threshold {
    if v == 0xff {
        Threshold::Off
    } else {
        Threshold::Value(v)
    }
}

/// Uses SmartShiftEnhanced (0x2111, e.g. MX Master 4) when available, otherwise SmartShift (0x2110, e.g. MX Master 3S)
pub fn get_smartshift(d: &mut impl FeatureCall) -> Result<SmartShift> {
    if d.has(f::SMART_SHIFT_ENHANCED) {
        let caps = d.call(f::SMART_SHIFT_ENHANCED, 0, &[])?;
        let r = d.call(f::SMART_SHIFT_ENHANCED, 1, &[])?;
        let tunable = caps[0] & 0x01 == 1;
        return Ok(SmartShift {
            mode: parse_mode(r[0]),
            threshold: parse_threshold(r[1]),
            torque: Some(r[2]),
            defaults: SmartShiftDefaults {
                threshold: caps[1],
                torque: Some(caps[2]),
            },
            tunable_torque: tunable,
        });
    }
    // SmartShift (0x2110) getRatchetControlMode: [wheelMode, autoDisengage, autoDisengageDefault]
    let r = d.call(f::SMART_SHIFT, 0, &[])?;
    Ok(SmartShift {
        mode: parse_mode(r[0]),
        threshold: parse_threshold(r[1]),
        torque: None,
        defaults: SmartShiftDefaults {
            threshold: r[2],
            torque: None,
        },
        tunable_torque: false,
    })
}

pub fn set_smartshift(
    d: &mut impl FeatureCall,
    mode: Option<WheelMode>,
    threshold: Option<Threshold>,
    torque: Option<u8>,
) -> Result<()> {
    let enhanced = d.has(f::SMART_SHIFT_ENHANCED);
    let cur = if enhanced {
        d.call(f::SMART_SHIFT_ENHANCED, 1, &[])?
    } else {
        d.call(f::SMART_SHIFT, 0, &[])?
    };
    let mode = match mode {
        None => cur[0],
        Some(WheelMode::Freespin) => 1,
        Some(WheelMode::Ratchet) => 2,
    };
    let threshold = match threshold {
        None => cur[1],
        Some(Threshold::Off) => 0xff,
        Some(Threshold::Value(v)) => v,
    };
    if !enhanced {
        if torque.is_some() {
            bail!("This device does not support changing the ratchet torque");
        }
        // SmartShift (0x2110) setRatchetControlMode: [wheelMode, autoDisengage, autoDisengageDefault]; 0 means "unchanged"
        d.call(f::SMART_SHIFT, 1, &[mode, threshold, 0])?;
        return Ok(());
    }
    let torque = torque.unwrap_or(cur[2]);
    if !(1..=100).contains(&torque) {
        bail!("torque must be 1-100");
    }
    // Solaar reports that the MX Master 4 ignores torque-only writes, so always write all three values
    d.call(f::SMART_SHIFT_ENHANCED, 2, &[mode, threshold, torque])?;
    Ok(())
}

// ---- HiResWheel (0x2121) ----
// mode: bit0 = divert to HID++, bit1 = high resolution, bit2 = inverted

#[derive(Debug, Serialize, PartialEq)]
pub struct Wheel {
    pub hires: bool,
    pub invert: bool,
    pub divert: bool,
    pub multiplier: u8,
    pub ratchet: bool,
}

pub fn get_wheel(d: &mut impl FeatureCall) -> Result<Wheel> {
    let caps = d.call(f::HIRES_WHEEL, 0, &[])?;
    let mode = d.call(f::HIRES_WHEEL, 1, &[])?[0];
    let ratchet = d.call(f::HIRES_WHEEL, 3, &[])?[0];
    Ok(Wheel {
        hires: mode & 0x02 != 0,
        invert: mode & 0x04 != 0,
        divert: mode & 0x01 != 0,
        multiplier: caps[0],
        ratchet: ratchet & 0x01 != 0,
    })
}

fn apply_bit(v: u8, bit: u8, on: Option<bool>) -> u8 {
    match on {
        Some(true) => v | bit,
        Some(false) => v & !bit,
        None => v,
    }
}

pub fn set_wheel(
    d: &mut impl FeatureCall,
    hires: Option<bool>,
    invert: Option<bool>,
    divert: Option<bool>,
) -> Result<()> {
    let mut mode = d.call(f::HIRES_WHEEL, 1, &[])?[0];
    mode = apply_bit(mode, 0x01, divert);
    mode = apply_bit(mode, 0x02, hires);
    mode = apply_bit(mode, 0x04, invert);
    d.call(f::HIRES_WHEEL, 2, &[mode])?;
    Ok(())
}

// ---- ThumbWheel (0x2150) ----
// getStatus: [reportingMode (0=HID, 1=HID++), invert (bit0)]

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThumbWheel {
    pub invert: bool,
    pub divert: bool,
    pub native_resolution: u16,
    pub diverted_resolution: u16,
}

pub fn get_thumbwheel(d: &mut impl FeatureCall) -> Result<ThumbWheel> {
    let info = d.call(f::THUMB_WHEEL, 0, &[])?;
    let r = d.call(f::THUMB_WHEEL, 1, &[])?;
    Ok(ThumbWheel {
        divert: r[0] == 1,
        invert: r[1] & 0x01 != 0,
        native_resolution: u16_at(&info, 0),
        diverted_resolution: u16_at(&info, 2),
    })
}

pub fn set_thumbwheel(
    d: &mut impl FeatureCall,
    invert: Option<bool>,
    divert: Option<bool>,
) -> Result<()> {
    let r = d.call(f::THUMB_WHEEL, 1, &[])?;
    let divert = divert.map_or(r[0], u8::from);
    let invert = invert.map_or(r[1] & 0x01, u8::from);
    d.call(f::THUMB_WHEEL, 2, &[divert, invert])?;
    Ok(())
}

// ---- Haptic (0x19b0) ----

pub const HAPTIC_WAVEFORMS: &[(&str, u8)] = &[
    ("sharp-state-change", 0x00),
    ("damp-state-change", 0x01),
    ("sharp-collision", 0x02),
    ("damp-collision", 0x03),
    ("subtle-collision", 0x04),
    ("happy-alert", 0x05),
    ("angry-alert", 0x06),
    ("completed", 0x07),
    ("square", 0x08),
    ("wave", 0x09),
    ("firework", 0x0a),
    ("mad", 0x0b),
    ("knock", 0x0c),
    ("jingle", 0x0d),
    ("ringing", 0x0e),
    ("whisper-collision", 0x1b),
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Haptic {
    pub enabled: bool,
    pub level: u8,
    pub four_levels_only: bool,
    pub waveforms: Vec<&'static str>,
}

pub fn get_haptic(d: &mut impl FeatureCall) -> Result<Haptic> {
    let caps = d.call(f::HAPTIC, 0, &[])?;
    let r = d.call(f::HAPTIC, 1, &[])?;
    let mask = u32::from_be_bytes([caps[4], caps[5], caps[6], caps[7]]);
    let waveforms = HAPTIC_WAVEFORMS
        .iter()
        .filter(|(_, id)| (mask >> id) & 1 == 1)
        .map(|(n, _)| *n)
        .collect();
    Ok(Haptic {
        enabled: r[0] & 0x01 != 0,
        level: r[1],
        four_levels_only: r[2] & 0x01 != 0,
        waveforms,
    })
}

/// Disables haptics when `level` is None
pub fn set_haptic(d: &mut impl FeatureCall, level: Option<u8>) -> Result<()> {
    match level {
        None => {
            let cur = d.call(f::HAPTIC, 1, &[])?[1];
            d.call(f::HAPTIC, 2, &[0, if cur == 0 { 50 } else { cur }])?;
        }
        Some(l @ 1..=100) => {
            d.call(f::HAPTIC, 2, &[1, l])?;
        }
        Some(_) => bail!("haptic level must be 1-100 or off"),
    }
    Ok(())
}

pub fn play_haptic(d: &mut impl FeatureCall, waveform: &str) -> Result<()> {
    let id = HAPTIC_WAVEFORMS
        .iter()
        .find(|(n, _)| *n == waveform)
        .map(|(_, id)| *id)
        .or_else(|| waveform.parse().ok())
        .ok_or_else(|| anyhow!("Unknown waveform: {waveform}"))?;
    d.call(f::HAPTIC, 4, &[id])?;
    Ok(())
}

// ---- ForceSensingButton (0x19c0) ----

#[derive(Debug, Serialize, PartialEq)]
pub struct ForceButton {
    pub button: u8,
    pub current: u16,
    pub default: u16,
    pub min: u16,
    pub max: u16,
    pub changeable: bool,
}

pub fn get_force_buttons(d: &mut impl FeatureCall) -> Result<Vec<ForceButton>> {
    let count = d.call(f::FORCE_SENSING_BUTTON, 0, &[])?[0];
    let mut out = Vec::new();
    for i in 0..count {
        let cfg = d.call(f::FORCE_SENSING_BUTTON, 1, &[i])?;
        let cur = d.call(f::FORCE_SENSING_BUTTON, 2, &[i])?;
        out.push(ForceButton {
            button: i,
            current: u16_at(&cur, 0),
            default: u16_at(&cfg, 2),
            max: u16_at(&cfg, 4),
            min: u16_at(&cfg, 6),
            changeable: u16_at(&cfg, 0) & 0x01 == 1,
        });
    }
    Ok(out)
}

/// Restores the default when `value` is None
pub fn set_force_button(d: &mut impl FeatureCall, button: u8, value: Option<u16>) -> Result<()> {
    let buttons = get_force_buttons(d)?;
    let b = buttons
        .iter()
        .find(|b| b.button == button)
        .ok_or_else(|| anyhow!("Force sensing button {button} not found"))?;
    if !b.changeable {
        bail!("Force sensing button {button} is not changeable");
    }
    let v = value.unwrap_or(b.default);
    if v < b.min || v > b.max {
        bail!("force must be {}-{} (default {})", b.min, b.max, b.default);
    }
    let [hi, lo] = v.to_be_bytes();
    d.call(f::FORCE_SENSING_BUTTON, 3, &[button, hi, lo])?;
    Ok(())
}

// ---- ReprogControlsV4 (0x1b04) ----

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub cid: u16,
    pub task_id: u16,
    pub flags: Vec<&'static str>,
    pub position: u8,
    pub group: u8,
    pub group_mask: u8,
    pub diverted: bool,
    pub persistently_diverted: bool,
    #[serde(rename = "rawXY")]
    pub raw_xy: bool,
    /// Whether analyticsKeyEvents (fn 2 notifications) are reported: bit0 of byte 5 of getCidReporting (inferred from observing a real device)
    pub analytics: bool,
    pub remapped_to: u16,
}

pub fn get_controls(d: &mut impl FeatureCall) -> Result<Vec<Control>> {
    let n = d.call(f::REPROG_CONTROLS_V4, 0, &[])?[0];
    let mut out = Vec::new();
    for i in 0..n {
        let r = d.call(f::REPROG_CONTROLS_V4, 1, &[i])?;
        let cid = u16_at(&r, 0);
        let rep = d.call(f::REPROG_CONTROLS_V4, 2, &cid.to_be_bytes())?;
        let remap = u16_at(&rep, 3);
        out.push(Control {
            cid,
            task_id: u16_at(&r, 2),
            flags: control_flags(r[4], r[8]),
            position: r[5],
            group: r[6],
            group_mask: r[7],
            diverted: rep[2] & 0x01 != 0,
            persistently_diverted: rep[2] & 0x04 != 0,
            raw_xy: rep[2] & 0x10 != 0,
            analytics: rep[5] & 0x01 != 0,
            remapped_to: if remap == 0 { cid } else { remap },
        });
    }
    Ok(out)
}

fn can_map(src: &Control, dst: &Control) -> bool {
    dst.group != 0 && src.group_mask & (1 << (dst.group - 1)) != 0
}

/// Makes `source` act as `target`; `target == source` restores the default
pub fn remap_control(d: &mut impl FeatureCall, source: u16, target: u16) -> Result<()> {
    let controls = get_controls(d)?;
    let find = |cid: u16| {
        controls
            .iter()
            .find(|c| c.cid == cid)
            .ok_or_else(|| anyhow!("Control 0x{cid:04x} not found on this device"))
    };
    let src = find(source)?;
    let dst = find(target)?;
    if source != target {
        if !src.flags.contains(&"reprogrammable") {
            bail!("Control 0x{source:04x} is not reprogrammable");
        }
        if !can_map(src, dst) {
            let allowed: Vec<_> = controls
                .iter()
                .filter(|c| can_map(src, c))
                .map(|c| format!("0x{:04x}", c.cid))
                .collect();
            bail!(
                "Control 0x{source:04x} cannot be remapped to 0x{target:04x} (allowed: {})",
                allowed.join(", ")
            );
        }
    }
    let [sh, sl] = source.to_be_bytes();
    let [th, tl] = target.to_be_bytes();
    d.call(f::REPROG_CONTROLS_V4, 3, &[sh, sl, 0x00, th, tl])?;
    Ok(())
}

/// Diverts a button to HID++ notifications (stops its events from reaching the OS)
pub fn divert_control(
    d: &mut impl FeatureCall,
    cid: u16,
    divert: Option<bool>,
    persist: Option<bool>,
) -> Result<()> {
    let mut flags = 0u8;
    if let Some(v) = divert {
        flags |= 0x02 | u8::from(v);
    }
    if let Some(v) = persist {
        flags |= 0x08 | (u8::from(v) << 2);
    }
    let [h, l] = cid.to_be_bytes();
    d.call(f::REPROG_CONTROLS_V4, 3, &[h, l, flags, 0, 0])?;
    Ok(())
}

// ---- ChangeHost (0x1814) / HostsInfo (0x1815) ----

#[derive(Debug, Serialize)]
pub struct Hosts {
    pub count: u8,
    pub current: u8,
    pub names: Vec<Option<String>>,
}

pub fn get_hosts(d: &mut impl FeatureCall) -> Result<Hosts> {
    let r = d.call(f::CHANGE_HOST, 0, &[])?;
    let names = (0..r[0])
        .map(|h| get_host_name(d, h))
        .collect::<Result<_>>()?;
    Ok(Hosts {
        count: r[0],
        current: r[1],
        names,
    })
}

fn get_host_name(d: &mut impl FeatureCall, host: u8) -> Result<Option<String>> {
    if !d.has(f::HOSTS_INFO) {
        return Ok(None);
    }
    // getHostInfo: [host, status, busType, numPages, nameLen, nameMaxLen, ...]
    let info = match d.call(f::HOSTS_INFO, 1, &[host]) {
        Ok(i) => i,
        Err(HidppError::Protocol { .. }) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if info[1] == 0 {
        return Ok(None);
    }
    let len = info[4] as usize;
    let mut name = String::new();
    while name.len() < len {
        // getHostFriendlyName: [host, offset, name bytes...]
        let r = match d.call(f::HOSTS_INFO, 3, &[host, name.len() as u8]) {
            Ok(r) => r,
            Err(HidppError::Protocol { .. }) => break,
            Err(e) => return Err(e.into()),
        };
        let chunk: String = ascii(&r[2..]).chars().take(len - name.len()).collect();
        if chunk.is_empty() {
            break;
        }
        name.push_str(&chunk);
    }
    Ok((!name.is_empty()).then_some(name))
}

/// `host` is 0-based
pub fn set_host(d: &mut impl FeatureCall, host: u8) -> Result<()> {
    let count = d.call(f::CHANGE_HOST, 0, &[])?[0];
    if host >= count {
        bail!("host must be 1-{count}");
    }
    match d.call(f::CHANGE_HOST, 1, &[host]) {
        // The connection to this host drops after switching, so a missing response is not an error
        Ok(_) | Err(HidppError::Timeout(_)) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Formats a notification for humans
pub fn decode_notification(feature_id: u16, function: u8, p: &Params) -> Option<String> {
    use crate::controls::control_name;
    match (feature_id, function) {
        (f::REPROG_CONTROLS_V4, 0) => {
            let pressed: Vec<_> = [0, 2, 4, 6]
                .into_iter()
                .map(|o| u16_at(p, o))
                .filter(|&c| c != 0)
                .map(control_name)
                .collect();
            Some(if pressed.is_empty() {
                "released".into()
            } else {
                format!("pressed: {}", pressed.join(", "))
            })
        }
        (f::REPROG_CONTROLS_V4, 1) => {
            Some(format!("raw xy: dx={} dy={}", s16_at(p, 0), s16_at(p, 2)))
        }
        // analyticsKeyEvents: a list of [cid (u16), event (1 = down, 0 = up)], sent even for buttons that are not diverted
        (f::REPROG_CONTROLS_V4, 2) => {
            let events: Vec<_> = (0..5)
                .map(|i| (u16_at(p, i * 3), p[i * 3 + 2]))
                .take_while(|&(cid, _)| cid != 0)
                .map(|(cid, ev)| {
                    let action = if ev == 1 { "down" } else { "up" };
                    format!("{} {action}", control_name(cid))
                })
                .collect();
            Some(format!("key event: {}", events.join(", ")))
        }
        (f::HIRES_WHEEL, 0) => Some(format!(
            "wheel: delta={} {} periods={}",
            s16_at(p, 1),
            if p[0] & 0x10 != 0 {
                "hi-res"
            } else {
                "low-res"
            },
            p[0] & 0x0f
        )),
        (f::HIRES_WHEEL, 1) => Some(format!(
            "ratchet: {}",
            if p[0] & 0x01 != 0 {
                "ratchet"
            } else {
                "freespin"
            }
        )),
        (f::THUMB_WHEEL, 0) => {
            let status = ["inactive", "start", "active", "stop"]
                .get(p[4] as usize)
                .map_or_else(|| p[4].to_string(), |s| (*s).to_string());
            Some(format!(
                "thumbwheel: rotation={} status={status}",
                s16_at(p, 0)
            ))
        }
        (f::UNIFIED_BATTERY, 0) => {
            let b = parse_battery(p);
            Some(format!(
                "battery: {}% ({}, {})",
                b.percent, b.level, b.status
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests;
