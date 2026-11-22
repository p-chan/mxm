//! Config file (`~/.config/mxm/config.toml`) read by `apply` and `daemon`
//!
//! Settings are written per model name (`[device."MX Master 4"]`) and can be overridden per
//! serial number (`[serial."..."]`). Settings that are not written are left untouched.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde::Deserialize;

use crate::controls::parse_control;
use crate::device::{Threshold, WheelMode};
use crate::scroll::{ThumbScroll, WheelScroll};

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub device: BTreeMap<String, Settings>,
    #[serde(default)]
    pub serial: BTreeMap<String, Settings>,
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub dpi: Option<u16>,
    pub smartshift: Option<SmartShiftSettings>,
    pub wheel: Option<WheelSettings>,
    pub thumbwheel: Option<ThumbWheelSettings>,
    pub haptic: Option<Haptic>,
    pub force: Option<Force>,
    /// Button name -> target button name, or "default"
    #[serde(default)]
    pub buttons: BTreeMap<String, String>,
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SmartShiftSettings {
    pub mode: Option<WheelMode>,
    pub threshold: Option<Threshold>,
    pub torque: Option<u8>,
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WheelSettings {
    pub hires: Option<bool>,
    pub invert: Option<bool>,
    /// Scroll speed of `mxm daemon`, 0-100 (default 50)
    pub speed: Option<Speed>,
    /// High-resolution scrolling in `mxm daemon` (default true); overrides `hires` there
    pub smooth: Option<bool>,
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThumbWheelSettings {
    pub invert: Option<bool>,
    /// Scroll speed of `mxm daemon`, 0-100 (default 50)
    pub speed: Option<Speed>,
}

/// Scroll speed 0-100 (the "Scrolling speed" percentage of Logi Options+)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "i64")]
pub struct Speed(pub u8);

impl TryFrom<i64> for Speed {
    type Error = String;
    fn try_from(v: i64) -> std::result::Result<Self, String> {
        match v {
            0..=100 => Ok(Self(v as u8)),
            _ => Err("speed must be 0-100".into()),
        }
    }
}

/// Speed when none is written, as in Logi Options+
const DEFAULT_SPEED: u8 = 50;

impl Settings {
    /// Software scrolling of the main wheel in `mxm daemon` (always on, as with Logi Options+)
    pub fn wheel_scroll(&self) -> WheelScroll {
        let w = self.wheel.clone().unwrap_or_default();
        WheelScroll {
            speed: w.speed.map_or(DEFAULT_SPEED, |s| s.0),
            smooth: w.smooth.unwrap_or(true),
        }
    }

    /// Software scrolling of the thumb wheel in `mxm daemon` (always on, as with Logi Options+)
    pub fn thumb_scroll(&self) -> ThumbScroll {
        ThumbScroll {
            speed: self
                .thumbwheel
                .as_ref()
                .and_then(|t| t.speed)
                .map_or(DEFAULT_SPEED, |s| s.0),
        }
    }
}

/// A number, or a keyword such as "off" or "default"
#[derive(Deserialize)]
#[serde(untagged)]
enum NumberOrWord {
    Number(i64),
    Word(String),
}

/// Haptic level 1-100, or "off"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "NumberOrWord")]
pub enum Haptic {
    Level(u8),
    Off,
}

impl TryFrom<NumberOrWord> for Haptic {
    type Error = String;
    fn try_from(v: NumberOrWord) -> std::result::Result<Self, String> {
        match v {
            NumberOrWord::Number(n @ 1..=100) => Ok(Self::Level(n as u8)),
            NumberOrWord::Word(w) if w == "off" => Ok(Self::Off),
            _ => Err("haptic must be 1-100 or \"off\"".into()),
        }
    }
}

/// Press force of the haptic sense panel, or "default"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "NumberOrWord")]
pub enum Force {
    Value(u16),
    Default,
}

impl TryFrom<NumberOrWord> for Force {
    type Error = String;
    fn try_from(v: NumberOrWord) -> std::result::Result<Self, String> {
        match v {
            NumberOrWord::Number(n) => u16::try_from(n)
                .map(Self::Value)
                .map_err(|_| "force must be a number or \"default\"".into()),
            NumberOrWord::Word(w) if w == "default" => Ok(Self::Default),
            _ => Err("force must be a number or \"default\"".into()),
        }
    }
}

impl<'de> Deserialize<'de> for Threshold {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = match NumberOrWord::deserialize(d)? {
            NumberOrWord::Number(n) => n.to_string(),
            NumberOrWord::Word(w) => w,
        };
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// `$XDG_CONFIG_HOME/mxm/config.toml`, or `~/.config/mxm/config.toml`
pub fn default_path() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => std::env::home_dir()
            .ok_or_else(|| anyhow!("Cannot find the home directory"))?
            .join(".config"),
    };
    Ok(base.join("mxm").join("config.toml"))
}

pub fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("Cannot read the config file {}: {e}", path.display()))?;
    parse(&text).map_err(|e| anyhow!("Invalid config file {}: {e}", path.display()))
}

pub fn parse(text: &str) -> Result<Config> {
    Ok(toml::from_str(text)?)
}

impl Config {
    /// Settings for a device: the model section, overridden by the serial section. None if neither matches
    pub fn resolve(&self, name: &str, serial: Option<&str>) -> Option<Settings> {
        let by_name = self
            .device
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v);
        let by_serial = serial.and_then(|s| {
            self.serial
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(s))
                .map(|(_, v)| v)
        });
        match (by_name, by_serial) {
            (None, None) => None,
            (Some(base), None) => Some(base.clone()),
            (None, Some(over)) => Some(over.clone()),
            (Some(base), Some(over)) => Some(base.merged(over)),
        }
    }
}

impl Settings {
    /// `over` wins field by field; tables are merged by their keys
    fn merged(&self, over: &Settings) -> Settings {
        // Match buttons by control, so that `back` and `Back` (or `thumb` and `gesture`) are the same button
        let same = |a: &str, b: &str| match (parse_control(a), parse_control(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => a == b,
        };
        let mut buttons = self.buttons.clone();
        buttons.retain(|k, _| !over.buttons.keys().any(|o| same(k, o)));
        buttons.extend(over.buttons.clone());
        Settings {
            dpi: over.dpi.or(self.dpi),
            smartshift: merge_opt(&self.smartshift, &over.smartshift, |a, b| {
                SmartShiftSettings {
                    mode: b.mode.or(a.mode),
                    threshold: b.threshold.or(a.threshold),
                    torque: b.torque.or(a.torque),
                }
            }),
            wheel: merge_opt(&self.wheel, &over.wheel, |a, b| WheelSettings {
                hires: b.hires.or(a.hires),
                invert: b.invert.or(a.invert),
                speed: b.speed.or(a.speed),
                smooth: b.smooth.or(a.smooth),
            }),
            thumbwheel: merge_opt(&self.thumbwheel, &over.thumbwheel, |a, b| {
                ThumbWheelSettings {
                    invert: b.invert.or(a.invert),
                    speed: b.speed.or(a.speed),
                }
            }),
            haptic: over.haptic.or(self.haptic),
            force: over.force.or(self.force),
            buttons,
        }
    }
}

fn merge_opt<T: Clone>(a: &Option<T>, b: &Option<T>, f: impl Fn(&T, &T) -> T) -> Option<T> {
    match (a, b) {
        (Some(a), Some(b)) => Some(f(a, b)),
        (a, b) => b.clone().or_else(|| a.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[device."MX Master 4"]
dpi = 1600
smartshift = { mode = "ratchet", threshold = 12 }
wheel = { hires = true }
haptic = 60
force = "default"

[device."MX Master 4".buttons]
back = "middle"
forward = "default"

[serial."2616ZALC66H8"]
dpi = 2000
smartshift = { threshold = "off" }
buttons = { forward = "back" }
"#;

    #[test]
    fn model_section_matches_case_insensitively() {
        let c = parse(SAMPLE).unwrap();
        let s = c.resolve("mx master 4", None).unwrap();
        assert_eq!(s.dpi, Some(1600));
        assert_eq!(s.haptic, Some(Haptic::Level(60)));
        assert_eq!(s.force, Some(Force::Default));
        assert_eq!(s.buttons["back"], "middle");
        assert!(c.resolve("MX Master 3S", None).is_none());
    }

    #[test]
    fn serial_section_overrides_field_by_field() {
        let c = parse(SAMPLE).unwrap();
        let s = c.resolve("MX Master 4", Some("2616ZALC66H8")).unwrap();
        assert_eq!(s.dpi, Some(2000));
        assert_eq!(
            s.smartshift,
            Some(SmartShiftSettings {
                mode: Some(WheelMode::Ratchet),
                threshold: Some(Threshold::Off),
                torque: None,
            })
        );
        assert_eq!(s.wheel.unwrap().hires, Some(true));
        assert_eq!(s.buttons["back"], "middle");
        assert_eq!(s.buttons["forward"], "back");
    }

    #[test]
    fn serial_buttons_override_other_spellings_of_the_same_button() {
        let c = parse(
            "[device.\"MX Master 4\".buttons]\nBack = \"middle\"\nthumb = \"middle\"\n\
             [serial.\"S\".buttons]\nback = \"forward\"\ngesture = \"default\"",
        )
        .unwrap();
        let s = c.resolve("MX Master 4", Some("S")).unwrap();
        assert_eq!(s.buttons.len(), 2);
        assert_eq!(s.buttons["back"], "forward");
        assert_eq!(s.buttons["gesture"], "default");
    }

    #[test]
    fn scrolling_defaults_follow_options_plus() {
        let settings = |toml: &str| {
            parse(&format!("[device.\"M\"]\n{toml}"))
                .unwrap()
                .resolve("M", None)
                .unwrap()
        };
        let s = settings("dpi = 1600");
        assert_eq!(
            s.wheel_scroll(),
            WheelScroll {
                speed: 50,
                smooth: true
            }
        );
        assert_eq!(s.thumb_scroll(), ThumbScroll { speed: 50 });
        let s = settings("wheel = { speed = 85, smooth = false }\nthumbwheel = { speed = 0 }");
        assert_eq!(
            s.wheel_scroll(),
            WheelScroll {
                speed: 85,
                smooth: false
            }
        );
        assert_eq!(s.thumb_scroll(), ThumbScroll { speed: 0 });
    }

    #[test]
    fn serial_section_alone_is_enough() {
        let c = parse(SAMPLE).unwrap();
        let s = c.resolve("Some Other Mouse", Some("2616zalc66h8")).unwrap();
        assert_eq!(s.dpi, Some(2000));
    }

    #[test]
    fn rejects_unknown_keys_and_bad_values() {
        assert!(parse("[device.\"MX Master 4\"]\ndpii = 1600").is_err());
        assert!(parse("[device.\"MX Master 4\"]\nhaptic = 0").is_err());
        assert!(parse("[device.\"MX Master 4\"]\nhaptic = \"on\"").is_err());
        assert!(parse("[device.\"MX Master 4\"]\nsmartshift = { threshold = 255 }").is_err());
        assert!(parse("[device.\"MX Master 4\"]\nsmartshift = { mode = \"fast\" }").is_err());
        assert!(parse("[device.\"MX Master 4\"]\nwheel = { speed = -1 }").is_err());
        assert!(parse("[device.\"MX Master 4\"]\nthumbwheel = { speed = 101 }").is_err());
    }

    #[test]
    fn readme_examples_parse() {
        for readme in [
            include_str!("../README.md"),
            include_str!("../README.ja.md"),
        ] {
            // Git on Windows may check the files out with CRLF line endings
            let readme = readme.replace("\r\n", "\n");
            let block = readme
                .split("```toml\n")
                .nth(1)
                .and_then(|rest| rest.split("```").next())
                .expect("README has a TOML example");
            let c = parse(block).unwrap();
            assert!(c.resolve("MX Master 4", None).is_some());
        }
    }
}
