//! mxm - Configuration tool for Logitech MX Master mice (HID++ 2.0)

mod apply;
mod config;
mod controls;
mod daemon;
mod device;
mod discover;
mod features;
mod hidpp;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand};
use hidapi::HidApi;
use serde::Serialize;
use serde_json::json;

use controls::{control_name, parse_control};
use device::{Threshold, WheelMode};
use features::{feature_name, feature_type_flags, fmt_id};
use hidpp::{FeatureCall, Hidpp, hex};

#[derive(Parser)]
#[command(
    name = "mxm",
    version,
    about = "Configuration tool for Logitech MX Master mice (HID++ 2.0)"
)]
struct Cli {
    /// Target device (number from 'list' or part of its name)
    #[arg(short, long, global = true)]
    device: Option<String>,
    /// Output JSON
    #[arg(long, global = true)]
    json: bool,
    /// Print raw HID++ traffic to stderr
    #[arg(long, global = true)]
    trace: bool,
    #[command(subcommand)]
    command: Command,
}

/// Parses on / off
fn on_off(s: &str) -> std::result::Result<bool, String> {
    match s.to_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => Ok(true),
        "off" | "false" | "0" | "no" => Ok(false),
        _ => Err("must be on or off".into()),
    }
}

#[derive(Subcommand)]
enum Command {
    /// List connected HID++ 2.0 devices
    List,
    /// Show name, firmware, serial, battery and host
    Info,
    /// Show the device's HID++ feature table
    Features,
    /// Show battery status
    Battery,
    /// Show or set pointer DPI
    Dpi { value: Option<u16> },
    /// Show or set SmartShift (wheel ratchet)
    Smartshift {
        #[arg(long, value_enum)]
        mode: Option<WheelMode>,
        /// 1-254, or off to disable SmartShift
        #[arg(long)]
        threshold: Option<Threshold>,
        /// Ratchet torque (1-100)
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=100))]
        torque: Option<u8>,
    },
    /// Show or set the main scroll wheel
    Wheel {
        #[arg(long, value_parser = on_off)]
        hires: Option<bool>,
        #[arg(long, value_parser = on_off)]
        invert: Option<bool>,
        #[arg(long, value_parser = on_off)]
        divert: Option<bool>,
    },
    /// Show or set the thumb wheel
    Thumbwheel {
        #[arg(long, value_parser = on_off)]
        invert: Option<bool>,
        #[arg(long, value_parser = on_off)]
        divert: Option<bool>,
    },
    /// Show or set haptic feedback level (1-100 or off), or play a waveform with 'play <waveform>'
    Haptic { args: Vec<String> },
    /// Show or set the haptic sense panel press force (value or 'default')
    Force { value: Option<String> },
    /// List, remap, reset or divert buttons
    Buttons {
        #[command(subcommand)]
        action: Option<ButtonAction>,
    },
    /// Show Easy-Switch hosts or switch to another host
    Host { host: Option<u8> },
    /// Print HID++ notifications (temporarily diverts given controls)
    Monitor {
        /// Comma-separated buttons to divert while monitoring (e.g. back,gesture)
        #[arg(long, value_delimiter = ',')]
        divert: Vec<String>,
        /// Divert the main wheel while monitoring
        #[arg(long)]
        wheel: bool,
        /// Divert the thumb wheel while monitoring
        #[arg(long)]
        thumbwheel: bool,
    },
    /// Send a raw HID++ request (feature ID like 0x2201, or index like #20)
    Raw {
        feature: String,
        function: u8,
        bytes: Vec<String>,
    },
    /// Dump all readable settings as JSON
    Dump,
    /// Write the settings of the config file to connected devices (all of them unless --device is given)
    Apply {
        /// Config file (default: ~/.config/mxm/config.toml)
        #[arg(long)]
        config: Option<std::path::PathBuf>,
    },
    /// Stay running and write the config file to devices whenever they connect
    Daemon {
        /// Config file (default: ~/.config/mxm/config.toml)
        #[arg(long)]
        config: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum ButtonAction {
    /// Make <button> act as <target> (e.g. back middle)
    Remap { button: String, target: String },
    /// Restore the default action of <button>
    Reset { button: String },
    /// Send <button> to HID++ notifications instead of the OS
    Divert {
        button: String,
        #[arg(value_parser = on_off, action = clap::ArgAction::Set)]
        on: bool,
        /// Keep the diversion across reconnections
        #[arg(long)]
        persist: bool,
    },
}

fn parse_int(s: &str) -> Result<u32> {
    let r = match s.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16),
        None => s.parse(),
    };
    r.map_err(|_| anyhow!("not an integer: {s}"))
}

fn fmt_bool(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

struct App {
    api: HidApi,
    json: bool,
    trace: bool,
    device: Option<String>,
}

impl App {
    fn print<T: Serialize>(&self, data: &T, human: impl FnOnce() -> String) -> Result<()> {
        if self.json {
            emit(&serde_json::to_string_pretty(data)?)?;
        } else {
            emit(&human())?;
        }
        Ok(())
    }

    fn open(&self) -> Result<Hidpp> {
        let (mut d, _) = discover::open_target(&self.api, self.device.as_deref())?;
        d.trace = self.trace;
        Ok(d)
    }

    fn run(&mut self, command: Command) -> Result<()> {
        match command {
            Command::Apply { config } => {
                let path = config.map_or_else(config::default_path, Ok)?;
                return self.apply(&path);
            }
            Command::Daemon { config } => {
                let path = config.map_or_else(config::default_path, Ok)?;
                return daemon::run(&mut self.api, &path, self.trace);
            }
            _ => {}
        }
        if let Command::List = command {
            let all = discover::discover(&self.api);
            return self.print(&all, || {
                if all.is_empty() {
                    "No HID++ 2.0 device found".into()
                } else {
                    discover::format_list(&all)
                }
            });
        }
        let mut d = self.open()?;
        let d = &mut d;
        match command {
            Command::List | Command::Apply { .. } | Command::Daemon { .. } => unreachable!(),
            Command::Info => {
                let name = device::get_name(d)?;
                let info = device::get_device_info(d)?;
                let battery = if d.has(features::UNIFIED_BATTERY) {
                    Some(device::get_battery(d)?)
                } else {
                    None
                };
                let hosts = if d.has(features::CHANGE_HOST) {
                    Some(device::get_hosts(d)?)
                } else {
                    None
                };
                let data =
                    json!({ "name": name, "info": info, "battery": battery, "hosts": hosts });
                self.print(&data, || {
                    let mut lines = vec![
                        format!("Name:      {} ({})", name.name, name.kind),
                        format!("Model:     {}", info.model_ids.join(", ")),
                        format!("Serial:    {}", info.serial.as_deref().unwrap_or("-")),
                        format!("Unit ID:   {}", info.unit_id),
                        format!("Transport: {}", info.transports.join(", ")),
                    ];
                    for f in &info.firmware {
                        lines.push(format!(
                            "Firmware:  {:<10} {}{}",
                            f.kind,
                            f.version,
                            if f.active { " (active)" } else { "" }
                        ));
                    }
                    if let Some(b) = &battery {
                        lines.push(format!(
                            "Battery:   {}% ({}, {})",
                            b.percent, b.level, b.status
                        ));
                    }
                    if let Some(h) = &hosts {
                        lines.push(format!("Host:      {} / {}", h.current + 1, h.count));
                    }
                    lines.join("\n")
                })
            }
            Command::Features => {
                let list = device::list_features(d)?;
                let data: Vec<_> = list
                    .iter()
                    .map(|f| json!({ "index": f.index, "id": fmt_id(f.id), "name": feature_name(f.id), "type": f.kind, "version": f.version, "flags": feature_type_flags(f.kind) }))
                    .collect();
                self.print(&data, || {
                    list.iter()
                        .map(|f| {
                            let flags = feature_type_flags(f.kind);
                            let flags = if flags.is_empty() {
                                String::new()
                            } else {
                                format!("  [{}]", flags.join(", "))
                            };
                            format!(
                                "{:>3}  {}  v{}  {}{flags}",
                                f.index,
                                fmt_id(f.id),
                                f.version,
                                feature_name(f.id)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            }
            Command::Battery => {
                let b = device::get_battery(d)?;
                self.print(&b, || {
                    format!(
                        "{}% ({}, {}{})",
                        b.percent,
                        b.level,
                        b.status,
                        if b.external_power {
                            ", external power"
                        } else {
                            ""
                        }
                    )
                })
            }
            Command::Dpi { value } => {
                if let Some(v) = value {
                    device::set_dpi(d, v, 0)?;
                }
                let r = device::get_dpi(d, 0)?;
                self.print(&r, || {
                    let range = match &r.range {
                        Some(x) => format!("{}-{} step {}", x.min, x.max, x.step),
                        None => r
                            .supported
                            .iter()
                            .map(u16::to_string)
                            .collect::<Vec<_>>()
                            .join(", "),
                    };
                    format!("DPI: {} (default {}, range {range})", r.dpi, r.default)
                })
            }
            Command::Smartshift {
                mode,
                threshold,
                torque,
            } => {
                if mode.is_some() || threshold.is_some() || torque.is_some() {
                    device::set_smartshift(d, mode, threshold, torque)?;
                }
                let s = device::get_smartshift(d)?;
                self.print(&s, || {
                    let threshold = match s.threshold {
                        Threshold::Off => "off (SmartShift disabled)".to_string(),
                        Threshold::Value(v) => v.to_string(),
                    };
                    let mode = match s.mode {
                        WheelMode::Freespin => "freespin",
                        WheelMode::Ratchet => "ratchet",
                    };
                    let torque = match (s.torque, s.defaults.torque) {
                        (Some(t), Some(def)) => format!("{t} (default {def})"),
                        (Some(t), None) => t.to_string(),
                        _ => "not supported by this device".into(),
                    };
                    let torque = if s.torque.is_some() && !s.tunable_torque {
                        format!("{torque} (not tunable)")
                    } else {
                        torque
                    };
                    [
                        format!("Mode:      {mode}"),
                        format!("Threshold: {threshold} (default {})", s.defaults.threshold),
                        format!("Torque:    {torque}"),
                    ]
                    .join("\n")
                })
            }
            Command::Wheel {
                hires,
                invert,
                divert,
            } => {
                if hires.is_some() || invert.is_some() || divert.is_some() {
                    device::set_wheel(d, hires, invert, divert)?;
                }
                let w = device::get_wheel(d)?;
                self.print(&w, || {
                    [
                        format!(
                            "Hi-res:     {} (multiplier {})",
                            fmt_bool(w.hires),
                            w.multiplier
                        ),
                        format!("Invert:     {}", fmt_bool(w.invert)),
                        format!("Divert:     {}", fmt_bool(w.divert)),
                        format!(
                            "Ratchet:    {}",
                            if w.ratchet { "ratchet" } else { "freespin" }
                        ),
                    ]
                    .join("\n")
                })
            }
            Command::Thumbwheel { invert, divert } => {
                if invert.is_some() || divert.is_some() {
                    device::set_thumbwheel(d, invert, divert)?;
                }
                let t = device::get_thumbwheel(d)?;
                self.print(&t, || {
                    [
                        format!("Invert: {}", fmt_bool(t.invert)),
                        format!("Divert: {}", fmt_bool(t.divert)),
                        format!(
                            "Resolution: {} (diverted {})",
                            t.native_resolution, t.diverted_resolution
                        ),
                    ]
                    .join("\n")
                })
            }
            Command::Haptic { args } => {
                match args.as_slice() {
                    [] => {}
                    [play, waveform] if play == "play" => return device::play_haptic(d, waveform),
                    [play] if play == "play" => bail!("Usage: mxm haptic play <waveform>"),
                    [off] if off == "off" => device::set_haptic(d, None)?,
                    [level] => {
                        let level = level
                            .parse::<u8>()
                            .map_err(|_| anyhow!("haptic level must be 1-100 or off"))?;
                        device::set_haptic(d, Some(level))?;
                    }
                    _ => bail!("Usage: mxm haptic [<1-100>|off] | mxm haptic play <waveform>"),
                }
                let h = device::get_haptic(d)?;
                self.print(&h, || {
                    let state = if h.enabled {
                        format!("on, level {}", h.level)
                    } else {
                        "off".into()
                    };
                    format!("Haptic:    {state}\nWaveforms: {}", h.waveforms.join(", "))
                })
            }
            Command::Force { value } => {
                if let Some(v) = value {
                    let v = if v == "default" {
                        None
                    } else {
                        Some(u16::try_from(parse_int(&v)?)?)
                    };
                    device::set_force_button(d, 0, v)?;
                }
                let list = device::get_force_buttons(d)?;
                self.print(&list, || {
                    list.iter()
                        .map(|b| {
                            format!(
                                "Button {}: {} (default {}, range {}-{}{})",
                                b.button,
                                b.current,
                                b.default,
                                b.min,
                                b.max,
                                if b.changeable { "" } else { ", fixed" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            }
            Command::Buttons { action } => {
                match action {
                    None => {}
                    Some(ButtonAction::Remap { button, target }) => {
                        device::remap_control(d, parse_control(&button)?, parse_control(&target)?)?;
                    }
                    Some(ButtonAction::Reset { button }) => {
                        let cid = parse_control(&button)?;
                        device::remap_control(d, cid, cid)?;
                    }
                    Some(ButtonAction::Divert {
                        button,
                        on,
                        persist,
                    }) => {
                        device::divert_control(
                            d,
                            parse_control(&button)?,
                            Some(on),
                            persist.then_some(on),
                        )?;
                    }
                }
                let list = device::get_controls(d)?;
                let data: Vec<_> = list
                    .iter()
                    .map(|c| {
                        let mut v = serde_json::to_value(c).unwrap_or_default();
                        v["name"] = json!(control_name(c.cid));
                        v["remappedToName"] = json!(control_name(c.remapped_to));
                        v
                    })
                    .collect();
                self.print(&data, || {
                    list.iter()
                        .map(|c| {
                            let mut state = Vec::new();
                            if c.remapped_to != c.cid {
                                state.push(format!("-> {}", control_name(c.remapped_to)));
                            }
                            if c.diverted {
                                state.push("diverted".into());
                            }
                            if c.persistently_diverted {
                                state.push("persist".into());
                            }
                            if c.analytics {
                                state.push("analytics".into());
                            }
                            let state = if state.is_empty() {
                                String::new()
                            } else {
                                format!("[{}]  ", state.join(", "))
                            };
                            format!(
                                "0x{:04x}  {:<16} group {}  {state}{}",
                                c.cid,
                                control_name(c.cid),
                                c.group,
                                c.flags.join(" ")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            }
            Command::Host { host } => {
                if let Some(n) = host {
                    let index = n
                        .checked_sub(1)
                        .ok_or_else(|| anyhow!("host must be 1 or greater"))?;
                    device::set_host(d, index)?;
                    return self.print(&json!({ "switchedTo": n }), || {
                        format!("Switched to host {n}")
                    });
                }
                let h = device::get_hosts(d)?;
                self.print(&h, || {
                    h.names
                        .iter()
                        .enumerate()
                        .map(|(i, name)| {
                            let mark = if i == h.current as usize { "*" } else { " " };
                            format!(
                                "{mark} {}. {}",
                                i + 1,
                                name.as_deref().unwrap_or("(unnamed)")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            }
            Command::Raw {
                feature,
                function,
                bytes,
            } => {
                let index = match feature.strip_prefix('#') {
                    Some(i) => u8::try_from(parse_int(i)?)?,
                    None => {
                        let id = u16::try_from(parse_int(&feature)?)?;
                        d.feature_index(id)?
                            .ok_or_else(|| anyhow!("Feature {feature} is not supported"))?
                    }
                };
                let joined: String = bytes
                    .concat()
                    .chars()
                    .filter(char::is_ascii_hexdigit)
                    .collect();
                let params = (0..joined.len() / 2)
                    .map(|i| u8::from_str_radix(&joined[i * 2..i * 2 + 2], 16))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let r = d.request(index, function, &params, Duration::from_secs(2))?;
                self.print(
                    &json!({ "featureIndex": index, "response": hex(&r) }),
                    || hex(&r),
                )
            }
            Command::Dump => {
                fn safe<T: Serialize>(r: Result<T>) -> serde_json::Value {
                    match r {
                        Ok(v) => serde_json::to_value(v).unwrap_or_default(),
                        Err(e) => json!({ "error": e.to_string() }),
                    }
                }
                let features = device::list_features(d).map(|l| {
                    l.iter().map(|f| json!({ "index": f.index, "id": fmt_id(f.id), "name": feature_name(f.id), "type": f.kind, "version": f.version })).collect::<Vec<_>>()
                });
                let buttons = device::get_controls(d).map(|l| {
                    l.iter()
                        .map(|c| {
                            let mut v = serde_json::to_value(c).unwrap_or_default();
                            v["name"] = json!(control_name(c.cid));
                            v
                        })
                        .collect::<Vec<_>>()
                });
                let out = json!({
                    "name": safe(device::get_name(d)),
                    "info": safe(device::get_device_info(d)),
                    "features": safe(features),
                    "battery": safe(device::get_battery(d)),
                    "dpi": safe(device::get_dpi(d, 0)),
                    "smartshift": safe(device::get_smartshift(d)),
                    "wheel": safe(device::get_wheel(d)),
                    "thumbwheel": safe(device::get_thumbwheel(d)),
                    "haptic": safe(device::get_haptic(d)),
                    "force": safe(device::get_force_buttons(d)),
                    "buttons": safe(buttons),
                    "hosts": safe(device::get_hosts(d)),
                });
                emit(&serde_json::to_string_pretty(&out)?)?;
                Ok(())
            }
            Command::Monitor {
                divert,
                wheel,
                thumbwheel,
            } => self.monitor(d, &divert, wheel, thumbwheel),
        }
    }

    fn apply(&self, path: &std::path::Path) -> Result<()> {
        let config = config::load(path)?;
        let targets = match &self.device {
            Some(_) => vec![discover::open_target(&self.api, self.device.as_deref())?.1],
            None => discover::discover(&self.api),
        };
        if targets.is_empty() {
            bail!("No HID++ 2.0 device found");
        }
        let mut results = Vec::new();
        for t in &targets {
            let mut d = Hidpp::new(self.api.open_path(&t.path)?, t.device_index);
            d.trace = self.trace;
            let serial = device::get_device_info(&mut d).ok().and_then(|i| i.serial);
            let outcomes = config
                .resolve(&t.name, serial.as_deref())
                .map(|s| apply::apply(&mut d, &s));
            results.push((t, serial, outcomes));
        }
        let data: Vec<_> = results
            .iter()
            .map(|(t, serial, outcomes)| json!({ "name": t.name, "serial": serial, "results": outcomes }))
            .collect();
        self.print(&data, || {
            results
                .iter()
                .map(|(t, serial, outcomes)| {
                    let label = serial
                        .as_ref()
                        .map_or_else(|| t.name.clone(), |s| format!("{} ({s})", t.name));
                    let summary = outcomes.as_ref().map_or_else(
                        || "no settings in the config file".into(),
                        |o| apply::summarize(o),
                    );
                    format!("{label}: {summary}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        })?;
        let failed = results.iter().any(|(_, _, o)| {
            o.as_ref().is_some_and(|o| {
                o.iter()
                    .any(|o| matches!(o.status, apply::Status::Failed(_)))
            })
        });
        if failed {
            bail!("Some settings could not be applied");
        }
        Ok(())
    }

    fn monitor(
        &self,
        d: &mut Hidpp,
        divert: &[String],
        wheel: bool,
        thumbwheel: bool,
    ) -> Result<()> {
        let by_index: std::collections::HashMap<u8, u16> = device::list_features(d)?
            .into_iter()
            .map(|f| (f.index, f.id))
            .collect();

        // Record the current state before diverting so it can be restored on exit
        let mut restore_buttons = Vec::new();
        for name in divert {
            let cid = parse_control(name)?;
            let before = device::get_controls(d)?
                .into_iter()
                .find(|c| c.cid == cid)
                .ok_or_else(|| anyhow!("Button {name} not found"))?;
            device::divert_control(d, cid, Some(true), None)?;
            restore_buttons.push((cid, before.diverted));
        }
        let restore_wheel = if wheel {
            let before = device::get_wheel(d)?.divert;
            device::set_wheel(d, None, None, Some(true))?;
            Some(before)
        } else {
            None
        };
        let restore_thumb = if thumbwheel {
            let before = device::get_thumbwheel(d)?.divert;
            device::set_thumbwheel(d, None, Some(true))?;
            Some(before)
        } else {
            None
        };

        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();
        ctrlc::set_handler(move || flag.store(false, Ordering::SeqCst))?;
        eprintln!("Monitoring HID++ notifications. Press Ctrl+C to stop.");

        let result = (|| -> Result<()> {
            while running.load(Ordering::SeqCst) {
                let Some(n) = d.read_notification(Duration::from_millis(100))? else {
                    continue;
                };
                let id = by_index.get(&n.feature_index).copied();
                let label = id.map_or_else(
                    || format!("index 0x{:02x}", n.feature_index),
                    |id| format!("{}[{}]", feature_name(id), n.function),
                );
                let decoded =
                    id.and_then(|id| device::decode_notification(id, n.function, &n.params));
                let time = now_hms();
                let line = json!({ "time": time, "feature": id.map(fmt_id), "fn": n.function, "raw": hex(&n.raw), "decoded": decoded });
                self.print(&line, || {
                    format!(
                        "{time}  {label:<24} {}",
                        decoded.clone().unwrap_or_else(|| hex(&n.params))
                    )
                })?;
            }
            Ok(())
        })();

        for (cid, was) in restore_buttons.into_iter().rev() {
            device::divert_control(d, cid, Some(was), None)?;
        }
        if let Some(was) = restore_wheel {
            device::set_wheel(d, None, None, Some(was))?;
        }
        if let Some(was) = restore_thumb {
            device::set_thumbwheel(d, None, Some(was))?;
        }
        result
    }
}

/// Current UTC time (HH:MM:SS.mmm)
fn now_hms() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        now.subsec_millis()
    )
}

/// Writes a line to stdout. If the reader has gone away (e.g. `| head`), returns an error so the caller can clean up
fn emit(s: &str) -> Result<()> {
    use std::io::Write;
    writeln!(std::io::stdout().lock(), "{s}")?;
    Ok(())
}

fn is_broken_pipe(e: &anyhow::Error) -> bool {
    e.downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::BrokenPipe)
}

fn main() {
    let cli = Cli::parse();
    let result = HidApi::new().map_err(anyhow::Error::from).and_then(|api| {
        // macOS opens devices exclusively by default, which would lock out Logi Options+, other
        // `mxm` commands and `mxm daemon` from each other
        #[cfg(target_os = "macos")]
        api.set_open_exclusive(false);
        App {
            api,
            json: cli.json,
            trace: cli.trace,
            device: cli.device,
        }
        .run(cli.command)
    });
    if let Err(e) = result {
        if is_broken_pipe(&e) {
            return;
        }
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
