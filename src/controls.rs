//! Control IDs (CIDs) of ReprogControlsV4 and their names
//! Covers the CIDs found on the MX Master 4

use anyhow::{Result, bail};

const CONTROL_NAMES: &[(u16, &str)] = &[
    (0x0050, "left"),
    (0x0051, "right"),
    (0x0052, "middle"),
    (0x0053, "back"),
    (0x0056, "forward"),
    (0x00c3, "gesture"),
    (0x00c4, "smartshift"),
    (0x00d7, "virtual-gesture"),
    (0x01a0, "haptic-sense"),
];

const ALIASES: &[(&str, u16)] = &[
    ("left-click", 0x0050),
    ("right-click", 0x0051),
    ("middle-click", 0x0052),
    ("thumb", 0x00c3),
    ("mode-shift", 0x00c4),
    ("actions-ring", 0x01a0),
];

pub fn control_name(cid: u16) -> String {
    CONTROL_NAMES
        .iter()
        .find(|(c, _)| *c == cid)
        .map_or_else(|| format!("cid-0x{cid:04x}"), |(_, n)| (*n).to_string())
}

fn parse_int(s: &str) -> Option<u32> {
    match s.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

/// Resolves a CID from a name or a number
pub fn parse_control(input: &str) -> Result<u16> {
    let lower = input.to_lowercase();
    if let Some((cid, _)) = CONTROL_NAMES.iter().find(|(_, n)| *n == lower) {
        return Ok(*cid);
    }
    if let Some((_, cid)) = ALIASES.iter().find(|(a, _)| *a == lower) {
        return Ok(*cid);
    }
    let num = lower.strip_prefix("cid-").unwrap_or(&lower);
    match parse_int(num) {
        Some(n) if n > 0 && n <= 0xffff => Ok(n as u16),
        _ => {
            let names: Vec<_> = CONTROL_NAMES.iter().map(|(_, n)| *n).collect();
            bail!(
                "Unknown control: {input} (use a name such as {} or a CID like 0x53)",
                names.join(", ")
            )
        }
    }
}

/// flags (byte 4) and flags2 (byte 8) of getCidInfo
pub fn control_flags(flags: u8, flags2: u8) -> Vec<&'static str> {
    let f1 = [
        (0x01, "mouse"),
        (0x02, "fkey"),
        (0x04, "hotkey"),
        (0x08, "fn-toggle"),
        (0x10, "reprogrammable"),
        (0x20, "divertable"),
        (0x40, "persistently-divertable"),
        (0x80, "virtual"),
    ];
    let f2 = [
        (0x01, "raw-xy"),
        (0x02, "force-raw-xy"),
        (0x04, "analytics"),
        (0x08, "raw-wheel"),
    ];
    f1.into_iter()
        .filter(|(b, _)| flags & b != 0)
        .chain(f2.into_iter().filter(|(b, _)| flags2 & b != 0))
        .map(|(_, n)| n)
        .collect()
}
