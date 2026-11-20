//! Verifies decoding with responses recorded from a real device (MX Master 4, RBM27.02_0017)

use std::collections::HashMap;

use super::*;
use crate::hidpp::{self, Params};

fn b(s: &str) -> Params {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = [0u8; 16];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        p[i] = u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap();
    }
    p
}

fn s(text: &str) -> Params {
    let mut p = [0u8; 16];
    p[..text.len()].copy_from_slice(text.as_bytes());
    p
}

/// A fake device that returns recorded responses and records writes (fn >= 2) that have no recorded response
struct Fake {
    recorded: HashMap<String, Params>,
    writes: Vec<String>,
}

fn key(id: u16, function: u8, params: &[u8]) -> String {
    format!(
        "{id:04x}:{function}:{}",
        hidpp::hex(params).replace(' ', "")
    )
}

impl Fake {
    fn new() -> Self {
        let entries: &[(u16, u8, &str, Params)] = &[
            (f::ADJUSTABLE_DPI, 1, "00", b("00 00c8 e032 1f40 0000")),
            (f::ADJUSTABLE_DPI, 2, "00", b("00 0960 03e8")),
            (f::SMART_SHIFT_ENHANCED, 0, "", b("01 0a 4b 0e")),
            (f::SMART_SHIFT_ENHANCED, 1, "", b("01 0c 4b")),
            (f::HIRES_WHEEL, 0, "", b("0f 1c 18 18")),
            (f::HIRES_WHEEL, 1, "", b("03")),
            (f::HIRES_WHEEL, 3, "", b("00")),
            (f::THUMB_WHEEL, 0, "", b("0014 0078 00 03 03e8")),
            (f::THUMB_WHEEL, 1, "", b("01 00")),
            (f::HAPTIC, 0, "", b("00 01 00 3c 08 00 7f ff")),
            (f::HAPTIC, 1, "", b("01 3c 00")),
            (f::FORCE_SENSING_BUTTON, 0, "", b("01")),
            (f::FORCE_SENSING_BUTTON, 1, "00", b("0001 165a 1dba 11e2")),
            (f::FORCE_SENSING_BUTTON, 2, "00", b("165a")),
            (f::UNIFIED_BATTERY, 1, "", b("37 08 00 00")),
            (
                f::DEVICE_INFO,
                0,
                "",
                b("04 28157bf1 0002 b042 0000 0000 01 01"),
            ),
            (
                f::DEVICE_INFO,
                1,
                "00",
                b("00 52424d 27 02 0017 01 b042 38 86 09 4c"),
            ),
            (
                f::DEVICE_INFO,
                1,
                "01",
                b("01 4c4400 04 00 0000 00 0000 64 2b a1 a1"),
            ),
            (f::DEVICE_INFO, 1, "02", b("00 52424d 27 02 0017 00 b042")),
            (f::DEVICE_INFO, 1, "03", b("02 484700 00 00 0000 00 0000")),
            (f::DEVICE_INFO, 2, "", s("2616ZALC66H8")),
            (f::DEVICE_NAME, 0, "", b("0b")),
            (f::DEVICE_NAME, 1, "00", s("MX Master 4")),
            (f::DEVICE_NAME, 2, "", b("03")),
            (f::REPROG_CONTROLS_V4, 0, "", b("02")),
            (
                f::REPROG_CONTROLS_V4,
                1,
                "00",
                b("0053 003c 31 00 02 03 05"),
            ),
            (
                f::REPROG_CONTROLS_V4,
                1,
                "01",
                b("0052 003a 31 00 02 03 05"),
            ),
            (f::REPROG_CONTROLS_V4, 2, "0053", b("0053 01 0053 01")),
            (f::REPROG_CONTROLS_V4, 2, "0052", b("0052 00 0052 01")),
        ];
        let recorded = entries
            .iter()
            .map(|(id, func, p, r)| (format!("{id:04x}:{func}:{p}"), *r))
            .collect();
        Self {
            recorded,
            writes: Vec::new(),
        }
    }

    /// Responses recorded from an MX Master 3S (RBM22.00_0003), which has SmartShift (0x2110) instead of 0x2111
    fn mx_master_3s() -> Self {
        let mut d = Self::new();
        let enhanced = format!("{:04x}:", f::SMART_SHIFT_ENHANCED);
        d.recorded.retain(|k, _| !k.starts_with(&enhanced));
        d.recorded
            .insert(format!("{:04x}:0:", f::SMART_SHIFT), b("01 0a 0a"));
        d
    }
}

impl FeatureCall for Fake {
    fn call(&mut self, feature_id: u16, function: u8, params: &[u8]) -> hidpp::Result<Params> {
        let k = key(feature_id, function, params);
        if let Some(r) = self.recorded.get(&k) {
            return Ok(*r);
        }
        let is_getter = self
            .recorded
            .keys()
            .any(|r| r.starts_with(&format!("{feature_id:04x}:{function}:")));
        // SmartShift (0x2110) writes with fn 1; the other features write with fn 2 or later
        let is_writer = function >= 2 || (feature_id == f::SMART_SHIFT && function == 1);
        if is_writer && !is_getter {
            self.writes.push(k);
            return Ok([0; 16]);
        }
        panic!("no recorded response for {k}");
    }

    fn has(&mut self, feature_id: u16) -> bool {
        let prefix = format!("{feature_id:04x}:");
        self.recorded.keys().any(|k| k.starts_with(&prefix))
    }
}

#[test]
fn dpi_reads_current_value_and_range() {
    let r = get_dpi(&mut Fake::new(), 0).unwrap();
    assert_eq!((r.dpi, r.default), (2400, 1000));
    assert_eq!(
        r.range,
        Some(DpiRange {
            min: 200,
            max: 8000,
            step: 50
        })
    );
    assert_eq!(r.supported.len(), (8000 - 200) / 50 + 1);
}

#[test]
fn dpi_rejects_unsupported_value() {
    let mut d = Fake::new();
    assert!(
        set_dpi(&mut d, 1625, 0)
            .unwrap_err()
            .to_string()
            .contains("not supported")
    );
    set_dpi(&mut d, 1600, 0).unwrap();
    assert_eq!(d.writes, ["2201:3:000640"]);
}

#[test]
fn smartshift_always_writes_all_three_values() {
    let mut d = Fake::new();
    assert_eq!(
        get_smartshift(&mut d).unwrap(),
        SmartShift {
            mode: WheelMode::Freespin,
            threshold: Threshold::Value(12),
            torque: Some(75),
            defaults: SmartShiftDefaults {
                threshold: 10,
                torque: Some(75)
            },
            tunable_torque: true,
        }
    );
    set_smartshift(&mut d, None, None, Some(50)).unwrap();
    set_smartshift(&mut d, Some(WheelMode::Ratchet), Some(Threshold::Off), None).unwrap();
    assert_eq!(d.writes, ["2111:2:010c32", "2111:2:02ff4b"]);
}

#[test]
fn wheel_keeps_other_mode_bits() {
    let mut d = Fake::new();
    assert_eq!(
        get_wheel(&mut d).unwrap(),
        Wheel {
            hires: true,
            invert: false,
            divert: true,
            multiplier: 15,
            ratchet: false
        }
    );
    set_wheel(&mut d, None, Some(true), Some(false)).unwrap();
    assert_eq!(d.writes, ["2121:2:06"]);
}

#[test]
fn thumbwheel() {
    let mut d = Fake::new();
    assert_eq!(
        get_thumbwheel(&mut d).unwrap(),
        ThumbWheel {
            divert: true,
            invert: false,
            native_resolution: 20,
            diverted_resolution: 120
        }
    );
    set_thumbwheel(&mut d, Some(true), None).unwrap();
    assert_eq!(d.writes, ["2150:2:0101"]);
}

#[test]
fn haptic_waveforms_and_level() {
    let mut d = Fake::new();
    let h = get_haptic(&mut d).unwrap();
    assert!(h.enabled);
    assert_eq!(h.level, 60);
    assert_eq!(h.waveforms.len(), 16);
    set_haptic(&mut d, None).unwrap();
    play_haptic(&mut d, "happy-alert").unwrap();
    assert_eq!(d.writes, ["19b0:2:003c", "19b0:4:05"]);
}

#[test]
fn force_sensing_button() {
    let mut d = Fake::new();
    assert_eq!(
        get_force_buttons(&mut d).unwrap(),
        [ForceButton {
            button: 0,
            current: 5722,
            default: 5722,
            max: 7610,
            min: 4578,
            changeable: true
        }]
    );
    assert!(
        set_force_button(&mut d, 0, Some(9000))
            .unwrap_err()
            .to_string()
            .contains("4578-7610")
    );
    set_force_button(&mut d, 0, Some(6000)).unwrap();
    assert_eq!(d.writes, ["19c0:3:001770"]);
}

#[test]
fn battery_info_and_name() {
    let mut d = Fake::new();
    assert_eq!(
        get_battery(&mut d).unwrap(),
        Battery {
            percent: 55,
            level: "full",
            status: "discharging".into(),
            external_power: false
        }
    );
    let info = get_device_info(&mut d).unwrap();
    assert_eq!(info.serial.as_deref(), Some("2616ZALC66H8"));
    assert_eq!(info.model_ids, ["b042"]);
    assert_eq!(info.firmware[0].version, "RBM27.02_0017");
    assert_eq!(
        get_name(&mut d).unwrap(),
        Name {
            name: "MX Master 4".into(),
            kind: "mouse".into()
        }
    );
}

#[test]
fn controls_and_remap() {
    let mut d = Fake::new();
    let controls = get_controls(&mut d).unwrap();
    assert_eq!(controls[0].cid, 0x53);
    assert!(controls[0].diverted);
    assert!(controls[0].flags.contains(&"reprogrammable"));
    remap_control(&mut d, 0x53, 0x52).unwrap();
    assert_eq!(d.writes, ["1b04:3:0053000052"]);
    assert!(
        remap_control(&mut d, 0x53, 0x50)
            .unwrap_err()
            .to_string()
            .contains("not found")
    );
}

#[test]
fn decodes_notifications() {
    assert_eq!(
        decode_notification(f::REPROG_CONTROLS_V4, 0, &b("0053 00c3")).as_deref(),
        Some("pressed: back, gesture")
    );
    assert_eq!(
        decode_notification(f::REPROG_CONTROLS_V4, 0, &b("")).as_deref(),
        Some("released")
    );
    assert_eq!(
        decode_notification(f::HIRES_WHEEL, 0, &b("11 fff1")).as_deref(),
        Some("wheel: delta=-15 hi-res periods=1")
    );
}

#[test]
fn decodes_analytics_key_events() {
    // Notifications observed on the device when pressing and releasing buttons
    assert_eq!(
        decode_notification(f::REPROG_CONTROLS_V4, 2, &b("0050 01")).as_deref(),
        Some("key event: left down")
    );
    assert_eq!(
        decode_notification(f::REPROG_CONTROLS_V4, 2, &b("01a0 00")).as_deref(),
        Some("key event: haptic-sense up")
    );
}

#[test]
fn reads_analytics_reporting_flag() {
    let controls = get_controls(&mut Fake::new()).unwrap();
    assert!(controls.iter().all(|c| c.analytics));
}

#[test]
fn smartshift_v1_without_torque() {
    let mut d = Fake::mx_master_3s();
    assert_eq!(
        get_smartshift(&mut d).unwrap(),
        SmartShift {
            mode: WheelMode::Freespin,
            threshold: Threshold::Value(10),
            torque: None,
            defaults: SmartShiftDefaults {
                threshold: 10,
                torque: None
            },
            tunable_torque: false,
        }
    );
    assert!(
        set_smartshift(&mut d, None, None, Some(50))
            .unwrap_err()
            .to_string()
            .contains("torque")
    );
    set_smartshift(
        &mut d,
        Some(WheelMode::Ratchet),
        Some(Threshold::Value(20)),
        None,
    )
    .unwrap();
    assert_eq!(d.writes, ["2110:1:021400"]);
}
