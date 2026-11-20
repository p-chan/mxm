//! `mxm daemon`: writes the config file to devices whenever they connect
//!
//! Devices forget most settings when they are turned off (see PROTOCOL.md), so the settings are
//! written again on these events:
//! - A HID interface appears (a device connected over Bluetooth or USB, or a receiver plugged in).
//!   Interfaces are enumerated every few seconds; enumeration does not wake devices.
//! - A receiver reports that a device in one of its slots connected (HID++ 1.0 notification 0x41).
//! - A device asks to be reconfigured (WirelessDeviceStatus 0x1d4b notification), e.g. after power-on.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use hidapi::{HidApi, HidDevice};

use crate::apply;
use crate::config;
use crate::device;
use crate::features;
use crate::hidpp::{
    DAEMON_SW_ID, DIRECT_INDEX, Hidpp, Interface, REPORT_LONG, REPORT_SHORT, Transport,
    list_interfaces,
};

const SCAN_INTERVAL: Duration = Duration::from_secs(2);
/// A connection produces several events (e.g. 0x41 and 0x1d4b); apply once for all of them
const DEBOUNCE: Duration = Duration::from_secs(5);
/// HID++ 1.0 receiver register that enables notifications; bit 0x000100 = wireless (dis)connections
const REGISTER_NOTIFICATIONS: u8 = 0x00;
/// HID++ 1.0 notification sent by a receiver when a device in a slot connects or disconnects
const DEVICE_CONNECTION: u8 = 0x41;

enum Event {
    /// A notification (software ID 0) read from an interface
    Report { path: CString, report: Vec<u8> },
    /// The interface went away (e.g. a Bluetooth device turned off)
    Closed { path: CString },
}

type Slot = (CString, u8);

struct Daemon<'a> {
    api: &'a mut HidApi,
    config_path: &'a Path,
    trace: bool,
    tx: Sender<Event>,
    watched: HashMap<CString, Interface>,
    /// Interfaces that could not be opened; logged once
    failed: HashMap<CString, String>,
    /// Receivers whose connection notifications could not be enabled yet; retried on every scan
    notifications_pending: HashSet<CString>,
    /// Feature index of WirelessDeviceStatus per device, to recognize its notifications
    status_index: HashMap<Slot, u8>,
    last_apply: HashMap<Slot, Instant>,
}

pub fn run(api: &mut HidApi, config_path: &Path, trace: bool) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    log(&format!(
        "Started; writing {} to devices when they connect",
        config_path.display()
    ));
    let mut daemon = Daemon {
        api,
        config_path,
        trace,
        tx,
        watched: HashMap::new(),
        failed: HashMap::new(),
        notifications_pending: HashSet::new(),
        status_index: HashMap::new(),
        last_apply: HashMap::new(),
    };
    daemon.run(&rx)
}

impl Daemon<'_> {
    fn run(&mut self, rx: &Receiver<Event>) -> Result<()> {
        let mut next_scan = Instant::now();
        loop {
            if Instant::now() >= next_scan {
                self.scan();
                next_scan = Instant::now() + SCAN_INTERVAL;
            }
            let wait = next_scan.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(Event::Report { path, report }) => self.on_report(path, &report),
                Ok(Event::Closed { path }) => {
                    if let Some(iface) = self.watched.remove(&path) {
                        log(&format!("{} disconnected", iface.hid_product));
                    }
                    self.notifications_pending.remove(&path);
                    self.status_index.retain(|(p, _), _| *p != path);
                    self.last_apply.retain(|(p, _), _| *p != path);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    unreachable!("the daemon holds a sender")
                }
            }
        }
    }

    /// Starts watching interfaces that appeared since the last scan
    fn scan(&mut self) {
        if let Err(e) = self.api.refresh_devices() {
            log(&format!("Failed to enumerate HID devices: {e}"));
            return;
        }
        for path in self.notifications_pending.clone() {
            if let Some(iface) = self.watched.get(&path).cloned()
                && self.enable_connection_notifications(&iface, false)
            {
                self.notifications_pending.remove(&path);
            }
        }
        for iface in list_interfaces(self.api) {
            if self.watched.contains_key(&iface.path) {
                continue;
            }
            let dev = match self.api.open_path(&iface.path) {
                Ok(dev) => dev,
                Err(e) => {
                    let msg = e.to_string();
                    if self.failed.get(&iface.path) != Some(&msg) {
                        log(&format!("Cannot open {}: {msg}", iface.hid_product));
                        self.failed.insert(iface.path.clone(), msg);
                    }
                    continue;
                }
            };
            self.failed.remove(&iface.path);
            spawn_reader(dev, iface.path.clone(), self.tx.clone());
            log(&format!(
                "{} connected ({})",
                iface.hid_product, iface.transport
            ));
            self.watched.insert(iface.path.clone(), iface.clone());
            if iface.transport == Transport::Receiver {
                if !self.enable_connection_notifications(&iface, true) {
                    self.notifications_pending.insert(iface.path.clone());
                }
                for index in 1..=6 {
                    self.apply(&iface.path, index, false);
                }
            } else {
                self.apply(&iface.path, DIRECT_INDEX, true);
            }
        }
    }

    fn on_report(&mut self, path: CString, r: &[u8]) {
        let at = |i: usize| r.get(i).copied().unwrap_or(0);
        let index = at(1);
        // [0x10, slot, 0x41, protocol, flags (bit6: link not established), ...]
        let connected = at(0) == REPORT_SHORT && at(2) == DEVICE_CONNECTION && at(4) & 0x40 == 0;
        // [0x11, idx, WirelessDeviceStatus index, fn 0, status, request (1: reconfigure), reason]
        let reconfigure = at(0) == REPORT_LONG
            && at(3) == 0x00
            && self.status_index.get(&(path.clone(), index)) == Some(&at(2))
            && at(5) == 1;
        if connected || reconfigure {
            self.apply(&path, index, true);
        }
    }

    /// Makes the receiver report device connections, keeping the flags other software set.
    /// Returns false on failure, which is logged only when `log_error` is set (retries stay quiet)
    fn enable_connection_notifications(&self, iface: &Interface, log_error: bool) -> bool {
        let result = (|| -> Result<()> {
            let mut d = self.open(&iface.path, DIRECT_INDEX)?;
            let mut flags = d.read_register(REGISTER_NOTIFICATIONS)?;
            if flags[1] & 0x01 == 0 {
                flags[1] |= 0x01;
                d.write_register(REGISTER_NOTIFICATIONS, flags)?;
            }
            Ok(())
        })();
        if let Err(e) = &result
            && log_error
        {
            log(&format!(
                "Cannot enable connection notifications on {}: {e}",
                iface.hid_product
            ));
        }
        result.is_ok()
    }

    fn open(&self, path: &CString, index: u8) -> Result<Hidpp> {
        let mut d = Hidpp::new(self.api.open_path(path)?, index);
        d.sw_id = DAEMON_SW_ID;
        d.trace = self.trace;
        Ok(d)
    }

    /// Writes the config to one device. `retry` waits for a device that has just connected
    fn apply(&mut self, path: &CString, index: u8, retry: bool) {
        let slot = (path.clone(), index);
        if self
            .last_apply
            .get(&slot)
            .is_some_and(|t| t.elapsed() < DEBOUNCE)
        {
            return;
        }
        let Some(iface) = self.watched.get(path).cloned() else {
            return;
        };
        let mut d = match self.open(path, index) {
            Ok(d) => d,
            Err(e) => {
                log(&format!("Cannot open {}: {e}", iface.hid_product));
                return;
            }
        };
        // Empty receiver slots fail at once; a device that just connected may take a moment to answer
        let attempts = if retry { 3 } else { 1 };
        let responding = (0..attempts).any(|i| {
            if i > 0 {
                thread::sleep(Duration::from_secs(1));
            }
            matches!(d.ping(Duration::from_secs(2)), Ok(Some((major, _))) if major >= 2)
        });
        if !responding {
            if retry {
                log(&format!("{} did not respond", iface.hid_product));
            }
            return;
        }
        if let Ok(Some(i)) = d.feature_index(features::WIRELESS_DEVICE_STATUS) {
            self.status_index.insert(slot.clone(), i);
        }

        let config = match config::load(self.config_path) {
            Ok(c) => c,
            Err(e) => {
                log(&format!("{}: {e:#}", iface.hid_product));
                return;
            }
        };
        // Right after connecting, the device may stop answering for a moment (e.g. while Logi
        // Options+ talks to it), and Bluetooth gives no later event to retry on, so retry here
        for attempt in 1..=attempts {
            let last = attempt == attempts;
            match apply_once(&mut d, &config) {
                Ok((label, None)) => {
                    log(&format!("{label}: no settings in the config file"));
                    return;
                }
                Ok((label, Some(outcomes))) => {
                    let failed = outcomes
                        .iter()
                        .any(|o| matches!(o.status, apply::Status::Failed(_)));
                    if !failed || last {
                        log(&format!("{label}: {}", apply::summarize(&outcomes)));
                    }
                    // Only a successful apply suppresses the other events of the same connection
                    if !failed {
                        self.last_apply.insert(slot, Instant::now());
                        return;
                    }
                }
                Err(e) if last => {
                    log(&format!("{}: {e}", iface.hid_product));
                    return;
                }
                Err(_) => {}
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
}

/// Reads the name and serial number, and writes the matching settings. None if no settings match
fn apply_once(
    d: &mut Hidpp,
    config: &config::Config,
) -> Result<(String, Option<Vec<apply::Outcome>>)> {
    let name = device::get_name(d)?.name;
    let serial = device::get_serial(d)?;
    let label = match &serial {
        Some(s) => format!("{name} ({s})"),
        None => name.clone(),
    };
    let outcomes = config
        .resolve(&name, serial.as_deref())
        .map(|s| apply::apply(d, &s));
    Ok((label, outcomes))
}

/// Forwards notifications from one interface until it goes away
fn spawn_reader(dev: HidDevice, path: CString, tx: Sender<Event>) {
    thread::spawn(move || {
        let mut buf = [0u8; 64];
        loop {
            match dev.read_timeout(&mut buf, 1000) {
                Ok(0) => {}
                Ok(n) => {
                    let r = &buf[..n];
                    let is_hidpp = (r[0] == REPORT_SHORT || r[0] == REPORT_LONG) && r.len() >= 4;
                    // Software ID 0: sent by the device on its own, not a response to a request
                    if is_hidpp && r[3] & 0x0f == 0 {
                        let event = Event::Report {
                            path: path.clone(),
                            report: r.to_vec(),
                        };
                        if tx.send(event).is_err() {
                            return;
                        }
                    }
                }
                Err(_) => {
                    let _ = tx.send(Event::Closed { path });
                    return;
                }
            }
        }
    });
}

fn log(msg: &str) {
    eprintln!("{} {msg}", crate::now_hms());
}
