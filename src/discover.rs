//! Device discovery and selection

use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use hidapi::HidApi;
use serde::Serialize;

use crate::device::{get_name, u16_at};
use crate::features;
use crate::hidpp::FeatureCall;
use crate::hidpp::{DIRECT_INDEX, Hidpp, Transport, list_interfaces};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundDevice {
    #[serde(skip)]
    pub path: std::ffi::CString,
    pub device_index: u8,
    pub product_id: u16,
    pub transport: Transport,
    pub name: String,
    pub protocol: String,
}

/// Lists connected HID++ 2.0 devices, scanning slots 1..6 of each receiver
pub fn discover(api: &HidApi) -> Vec<FoundDevice> {
    let mut found = Vec::new();
    for iface in list_interfaces(api) {
        let Ok(dev) = api.open_path(&iface.path) else {
            continue;
        };
        let indexes: Vec<u8> = if iface.transport == Transport::Receiver {
            (1..=6).collect()
        } else {
            vec![DIRECT_INDEX]
        };
        // A device behind a receiver can take over a second to wake from power saving.
        // Empty slots return an error immediately, so a longer timeout does not slow down the scan.
        let timeout = Duration::from_millis(if iface.transport == Transport::Receiver {
            2000
        } else {
            1000
        });
        let mut d = Hidpp::new(dev, DIRECT_INDEX);
        for index in indexes {
            d.set_device_index(index);
            let Ok(Some((major, minor))) = d.ping(timeout) else {
                continue;
            };
            if major < 2 {
                continue;
            }
            // Fall back to the HID product name for devices without DeviceName
            let name = get_name(&mut d).map_or_else(|_| iface.hid_product.clone(), |n| n.name);
            // Behind a receiver, the HID product ID is the receiver's; use the device's own model ID instead
            let product_id = model_id(&mut d).unwrap_or(iface.product_id);
            found.push(FoundDevice {
                path: iface.path.clone(),
                device_index: index,
                product_id,
                transport: iface.transport,
                name,
                protocol: format!("{major}.{minor}"),
            });
        }
    }
    // HID enumeration order can change between runs; sort so that `--device <n>` stays stable
    found.sort_by(|a, b| {
        (a.name.as_str(), a.transport as u8, a.device_index).cmp(&(
            b.name.as_str(),
            b.transport as u8,
            b.device_index,
        ))
    });
    found
}

/// The first model ID reported by DeviceInfo (0x0003)
fn model_id(d: &mut Hidpp) -> Option<u16> {
    let r = d.call(features::DEVICE_INFO, 0, &[]).ok()?;
    let id = u16_at(&r, 7);
    (id != 0).then_some(id)
}

/// Opens the target device. `selector` is a number from `list` or part of a name; without it, an MX Master mouse is preferred
pub fn open_target(api: &HidApi, selector: Option<&str>) -> Result<(Hidpp, FoundDevice)> {
    let all = discover(api);
    if all.is_empty() {
        bail!(
            "No HID++ 2.0 device found. Wake the mouse by moving it, and check that it is connected via Bluetooth or a Logi Bolt receiver."
        );
    }
    let target = match selector {
        None => {
            // Pick the only MX Master mouse, or the only device; never guess between several
            let mx: Vec<_> = all
                .iter()
                .filter(|d| d.name.to_lowercase().contains("mx master"))
                .collect();
            match (mx.as_slice(), all.as_slice()) {
                ([one], _) => *one,
                ([], [one]) => one,
                _ => bail!(
                    "Multiple devices found; choose one with --device:\n{}",
                    format_list(&all)
                ),
            }
        }
        Some(s) => {
            let hit = match s.parse::<usize>() {
                Ok(n) => n.checked_sub(1).and_then(|i| all.get(i)),
                Err(_) => all
                    .iter()
                    .find(|d| d.name.to_lowercase().contains(&s.to_lowercase())),
            };
            hit.ok_or_else(|| anyhow!("Device \"{s}\" not found:\n{}", format_list(&all)))?
        }
    };
    let dev = api.open_path(&target.path)?;
    Ok((Hidpp::new(dev, target.device_index), target.clone()))
}

pub fn format_list(all: &[FoundDevice]) -> String {
    all.iter()
        .enumerate()
        .map(|(i, d)| {
            let slot = if d.device_index == DIRECT_INDEX {
                String::new()
            } else {
                format!(" slot {}", d.device_index)
            };
            format!(
                "  {}. {} ({}{slot}, HID++ {}, pid {:04x})",
                i + 1,
                d.name,
                d.transport,
                d.protocol,
                d.product_id
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
