//! Writes the settings of the config file to a device

use anyhow::Result;
use serde::Serialize;

use crate::config::{Force, Haptic, Settings};
use crate::controls::parse_control;
use crate::device;
use crate::hidpp::{FeatureCall, HidppError};

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase", tag = "status", content = "message")]
pub enum Status {
    Applied,
    /// The device does not have the feature (e.g. haptics on the MX Master 3S)
    Unsupported,
    Failed(String),
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Outcome {
    pub item: String,
    #[serde(flatten)]
    pub status: Status,
}

/// Writes every setting in `s`, continuing after failures. Settings that are not written are left untouched
pub fn apply(d: &mut impl FeatureCall, s: &Settings) -> Vec<Outcome> {
    let mut out = Vec::new();
    let mut run = |item: String, f: &mut dyn FnMut() -> Result<()>| {
        let status = match f() {
            Ok(()) => Status::Applied,
            Err(e) if matches!(e.downcast_ref(), Some(HidppError::Unsupported(_))) => {
                Status::Unsupported
            }
            Err(e) => Status::Failed(e.to_string()),
        };
        out.push(Outcome { item, status });
    };

    if let Some(dpi) = s.dpi {
        run("dpi".into(), &mut || device::set_dpi(d, dpi, 0));
    }
    if let Some(ss) = &s.smartshift {
        run("smartshift".into(), &mut || {
            device::set_smartshift(d, ss.mode, ss.threshold, ss.torque)
        });
    }
    if let Some(w) = &s.wheel {
        run("wheel".into(), &mut || {
            device::set_wheel(d, w.hires, w.invert, None)
        });
    }
    if let Some(t) = &s.thumbwheel {
        run("thumbwheel".into(), &mut || {
            device::set_thumbwheel(d, t.invert, None)
        });
    }
    if let Some(h) = s.haptic {
        run("haptic".into(), &mut || {
            device::set_haptic(
                d,
                match h {
                    Haptic::Level(l) => Some(l),
                    Haptic::Off => None,
                },
            )
        });
    }
    if let Some(f) = s.force {
        run("force".into(), &mut || {
            device::set_force_button(
                d,
                0,
                match f {
                    Force::Value(v) => Some(v),
                    Force::Default => None,
                },
            )
        });
    }
    for (button, target) in &s.buttons {
        run(format!("buttons.{button}"), &mut || {
            let source = parse_control(button)?;
            let target = if target == "default" {
                source
            } else {
                parse_control(target)?
            };
            device::remap_control(d, source, target)
        });
    }
    out
}

pub fn summarize(outcomes: &[Outcome]) -> String {
    let pick = |want: fn(&Status) -> bool| {
        outcomes
            .iter()
            .filter(|o| want(&o.status))
            .map(|o| match &o.status {
                Status::Failed(e) => format!("{} ({e})", o.item),
                _ => o.item.clone(),
            })
            .collect::<Vec<_>>()
    };
    let mut parts = Vec::new();
    for (label, items) in [
        ("applied", pick(|s| *s == Status::Applied)),
        ("not supported", pick(|s| *s == Status::Unsupported)),
        ("failed", pick(|s| matches!(s, Status::Failed(_)))),
    ] {
        if !items.is_empty() {
            parts.push(format!("{label}: {}", items.join(", ")));
        }
    }
    if parts.is_empty() {
        "nothing to apply".into()
    } else {
        parts.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;
    use crate::device::tests::Fake;

    fn settings(toml: &str) -> Settings {
        parse(&format!("[device.\"MX Master 4\"]\n{toml}"))
            .unwrap()
            .resolve("MX Master 4", None)
            .unwrap()
    }

    #[test]
    fn writes_only_the_given_settings() {
        let mut d = Fake::new();
        let out = apply(
            &mut d,
            &settings("dpi = 1600\nwheel = { invert = true }\nbuttons = { back = \"middle\" }"),
        );
        assert!(out.iter().all(|o| o.status == Status::Applied), "{out:?}");
        // DPI 1600, wheel mode 0x03 with the invert bit added, back -> middle
        assert_eq!(
            d.writes,
            ["2201:3:000640", "2121:2:07", "1b04:3:0053000052"]
        );
    }

    #[test]
    fn empty_settings_write_nothing() {
        let mut d = Fake::new();
        assert!(apply(&mut d, &Settings::default()).is_empty());
        assert!(d.writes.is_empty());
    }

    #[test]
    fn reports_unsupported_and_failed_items_and_continues() {
        let mut d = Fake::mx_master_3s_without_haptics();
        let out = apply(
            &mut d,
            &settings("dpi = 1625\nhaptic = 60\nthumbwheel = { invert = true }"),
        );
        assert!(matches!(out[0].status, Status::Failed(_)), "{out:?}");
        assert_eq!(out[1].item, "thumbwheel");
        assert_eq!(out[1].status, Status::Applied);
        assert_eq!(out[2].item, "haptic");
        assert_eq!(out[2].status, Status::Unsupported);
        assert_eq!(d.writes, ["2150:2:0101"]);
    }
}
