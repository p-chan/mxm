//! `mxm daemon`: writes the config file to devices whenever they connect
//!
//! Devices forget most settings when they are turned off (see PROTOCOL.md), so the settings are
//! written again on these events:
//! - A HID interface appears (a device connected over Bluetooth or USB, or a receiver plugged in).
//!   Interfaces are enumerated every few seconds; enumeration does not wake devices.
//! - A receiver reports that a device in one of its slots connected (HID++ 1.0 notification 0x41).
//! - A device asks to be reconfigured (WirelessDeviceStatus 0x1d4b notification), e.g. after power-on.
//! - The display turns on again, or the Mac wakes from sleep. After macOS starts going to sleep, an
//!   interface opened before may stop delivering notifications, even when the sleep is cancelled
//!   (observed with a Bluetooth MX Master 4: no notifications after `kIOMessageSystemWillSleep`
//!   until the interface was opened again), so all interfaces are opened again.
//!
//! Like Logi Options+, the daemon also diverts the wheels and posts the scroll events itself (see
//! scroll.rs). It undoes the diversion when it is stopped, since a diverted wheel does not scroll
//! without it.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

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
use crate::scroll::{self, Input, Scroller, ThumbScroll, WheelState};

const SCAN_INTERVAL: Duration = Duration::from_secs(2);
/// Wall-clock time the monotonic clock missed, over which the Mac is taken to have slept
const WAKE_GAP: Duration = Duration::from_secs(5);
/// A connection produces several events (e.g. 0x41 and 0x1d4b); apply once for all of them
const DEBOUNCE: Duration = Duration::from_secs(5);
/// HID++ 1.0 receiver register that enables notifications; bit 0x000100 = wireless (dis)connections
const REGISTER_NOTIFICATIONS: u8 = 0x00;
/// HID++ 1.0 notification sent by a receiver when a device in a slot connects or disconnects
const DEVICE_CONNECTION: u8 = 0x41;

enum Event {
    /// A notification (software ID 0) read from an interface
    Report {
        path: CString,
        reader: u64,
        report: Vec<u8>,
        /// When the report was read
        at: Instant,
    },
    /// The interface went away (e.g. a Bluetooth device turned off)
    Closed { path: CString, reader: u64 },
    /// SIGINT or SIGTERM (e.g. `mxm service uninstall`)
    Quit,
}

/// A device whose wheels the daemon diverted for software scrolling
struct Scrolling {
    /// Feature index of HiResWheel and the state of the wheel
    wheel: Option<(u8, WheelState)>,
    /// Feature index of ThumbWheel and the settings
    thumb: Option<(u8, ThumbScroll)>,
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
    scroller: Option<Scroller>,
    scrolling: HashMap<Slot, Scrolling>,
    /// Devices waiting for the Accessibility permission to start software scrolling
    scroll_pending: HashSet<Slot>,
    /// The current reader of each watched interface; events of replaced readers are ignored
    readers: HashMap<CString, Reader>,
    next_reader: u64,
}

struct Reader {
    id: u64,
    stop: Arc<AtomicBool>,
}

pub fn run(api: &mut HidApi, config_path: &Path, trace: bool) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let quit = tx.clone();
    ctrlc::set_handler(move || {
        let _ = quit.send(Event::Quit);
    })?;
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
        scroller: None,
        scrolling: HashMap::new(),
        scroll_pending: HashSet::new(),
        readers: HashMap::new(),
        next_reader: 0,
    };
    daemon.run(&rx)
}

impl Daemon<'_> {
    fn run(&mut self, rx: &Receiver<Event>) -> Result<()> {
        let mut next_scan = Instant::now();
        // `Instant` stops while the Mac sleeps and `SystemTime` does not, so a gap between them is a wake
        let mut clocks = (Instant::now(), SystemTime::now());
        let mut display_asleep = scroll::display_asleep();
        loop {
            // Polled at least every SCAN_INTERVAL
            let asleep = scroll::display_asleep();
            let display_woke = display_asleep && !asleep;
            display_asleep = asleep;
            let slept = SystemTime::now()
                .duration_since(clocks.1)
                .unwrap_or_default()
                .saturating_sub(clocks.0.elapsed());
            clocks = (Instant::now(), SystemTime::now());
            if slept > WAKE_GAP || display_woke {
                log(if display_woke {
                    "The display turned on; opening the devices again"
                } else {
                    "Woke from sleep; opening the devices again"
                });
                self.reopen_all();
                next_scan = Instant::now();
            }
            if Instant::now() >= next_scan {
                self.scan();
                next_scan = Instant::now() + SCAN_INTERVAL;
            }
            let wait = next_scan.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(Event::Report {
                    path,
                    reader,
                    report,
                    at,
                }) => {
                    if self.is_current(&path, reader) {
                        self.on_report(path, &report, at);
                    }
                }
                Ok(Event::Closed { path, reader }) => {
                    if !self.is_current(&path, reader) {
                        continue;
                    }
                    self.readers.remove(&path);
                    if let Some(iface) = self.watched.remove(&path) {
                        log(&format!("{} disconnected", iface.hid_product));
                    }
                    self.notifications_pending.remove(&path);
                    self.status_index.retain(|(p, _), _| *p != path);
                    self.last_apply.retain(|(p, _), _| *p != path);
                    self.scrolling.retain(|(p, _), _| *p != path);
                    self.scroll_pending.retain(|(p, _)| *p != path);
                }
                Ok(Event::Quit) => {
                    self.stop_scrolling();
                    log("Stopped");
                    return Ok(());
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
        if !self.scrolling.is_empty() && !scroll::has_permission(false) {
            // Posting no longer works; a diverted wheel would not scroll at all
            log("Accessibility permission lost; giving the wheels back to the OS");
            let slots: Vec<Slot> = self.scrolling.keys().cloned().collect();
            self.stop_scrolling();
            self.scroll_pending.extend(slots);
        }
        if !self.scroll_pending.is_empty() && scroll::has_permission(false) {
            log("Accessibility permission granted");
            // A slot leaves the list once applying reaches the scrolling setup, so a failed
            // attempt is retried on the next scan
            for (path, index) in self.scroll_pending.clone() {
                self.last_apply.remove(&(path.clone(), index));
                self.apply(&path, index, false);
            }
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
            let stop = Arc::new(AtomicBool::new(false));
            self.next_reader += 1;
            let id = self.next_reader;
            spawn_reader(dev, iface.path.clone(), id, stop.clone(), self.tx.clone());
            self.readers.insert(iface.path.clone(), Reader { id, stop });
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

    fn is_current(&self, path: &CString, reader: u64) -> bool {
        self.readers.get(path).is_some_and(|r| r.id == reader)
    }

    /// Opens every interface again and writes the config
    fn reopen_all(&mut self) {
        for (_, r) in self.readers.drain() {
            r.stop.store(true, Ordering::Relaxed);
        }
        self.watched.clear();
        self.notifications_pending.clear();
        self.status_index.clear();
        self.last_apply.clear();
        self.scroll_pending.clear();
        // `scrolling` is kept: the slots stay the same, and applying again updates it
    }

    fn on_report(&mut self, path: CString, r: &[u8], received: Instant) {
        let at = |i: usize| r.get(i).copied().unwrap_or(0);
        let index = at(1);
        if let Some(s) = self.scrolling.get_mut(&(path.clone(), index))
            && let Some(scroller) = &self.scroller
            && at(0) == REPORT_LONG
        {
            let function = at(3) >> 4;
            if let Some((feature, state)) = &mut s.wheel
                && at(2) == *feature
            {
                match function {
                    // HiResWheel fn 0: [flags (bit4: high resolution, bits 0-3: periods), delta (s16)]
                    0 => {
                        let delta = i16::from_be_bytes([at(5), at(6)]);
                        let periods = at(4) & 0x0f;
                        if self.trace {
                            log(&format!("wheel: {delta:+} x{}", periods.max(1)));
                        }
                        scroller.send(Input::Wheel {
                            delta,
                            periods,
                            at: received,
                            state: *state,
                        });
                    }
                    // HiResWheel fn 1: [ratchet (bit0)]
                    1 => state.freewheel = at(4) & 0x01 == 0,
                    _ => {}
                }
                return;
            }
            // ThumbWheel fn 0: [rotation (s16), timestamp (u16), status]
            if let Some((feature, scroll)) = s.thumb
                && at(2) == feature
                && function == 0
            {
                let rotation = i16::from_be_bytes([at(4), at(5)]);
                let status = at(8);
                if self.trace {
                    log(&format!("thumbwheel: {rotation:+} status {status}"));
                }
                scroller.send(Input::Thumb {
                    rotation,
                    status,
                    at: received,
                    scroll,
                });
                return;
            }
        }
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
                    self.scroll_pending.remove(&slot);
                    return;
                }
                Ok((label, Some((settings, outcomes)))) => {
                    let failed = outcomes
                        .iter()
                        .any(|o| matches!(o.status, apply::Status::Failed(_)));
                    if !failed || last {
                        log(&format!("{label}: {}", apply::summarize(&outcomes)));
                    }
                    // A failed setting does not hold back software scrolling
                    if !failed || last {
                        self.update_scrolling(&mut d, &slot, &settings, &label);
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
#[allow(clippy::type_complexity)]
fn apply_once(
    d: &mut Hidpp,
    config: &config::Config,
) -> Result<(String, Option<(config::Settings, Vec<apply::Outcome>)>)> {
    let name = device::get_name(d)?.name;
    let serial = device::get_serial(d)?;
    let label = match &serial {
        Some(s) => format!("{name} ({s})"),
        None => name.clone(),
    };
    let outcomes = config.resolve(&name, serial.as_deref()).map(|s| {
        let outcomes = apply::apply(d, &s);
        (s, outcomes)
    });
    Ok((label, outcomes))
}

impl Daemon<'_> {
    /// Diverts the wheels that the settings scroll in software, and undoes earlier diversions that
    /// the settings no longer ask for
    fn update_scrolling(
        &mut self,
        d: &mut Hidpp,
        slot: &Slot,
        settings: &config::Settings,
        label: &str,
    ) {
        let wheel = settings.wheel_scroll();
        let thumb = settings.thumb_scroll();
        self.scrolling.remove(slot);
        self.scroll_pending.remove(slot);
        let result = (|| -> Result<Vec<&str>> {
            if !scroll::SUPPORTED {
                return Ok(vec![]);
            }
            if !scroll::has_permission(true) {
                self.scroll_pending.insert(slot.clone());
                anyhow::bail!(
                    "software scrolling needs the Accessibility permission: allow {} in System Settings > Privacy & Security > Accessibility",
                    std::env::current_exe()?.display()
                );
            }
            let mut s = Scrolling {
                wheel: None,
                thumb: None,
            };
            let mut items = vec![];
            if let Some(index) = d.feature_index(features::HIRES_WHEEL)? {
                // Smooth scrolling is the high-resolution mode, as in Options+
                device::set_wheel(d, Some(wheel.smooth), None, Some(true))?;
                let w = device::get_wheel(d)?;
                let multiplier = if w.hires { w.multiplier.max(1) } else { 1 };
                s.wheel = Some((
                    index,
                    WheelState {
                        scroll: wheel,
                        multiplier,
                        freewheel: !w.ratchet,
                    },
                ));
                items.push("wheel");
            }
            if let Some(index) = d.feature_index(features::THUMB_WHEEL)? {
                device::set_thumbwheel(d, None, Some(true))?;
                s.thumb = Some((index, thumb));
                items.push("thumbwheel");
            }
            self.scroller.get_or_insert_with(Scroller::new);
            self.scrolling.insert(slot.clone(), s);
            Ok(items)
        })();
        match result {
            Ok(items) if items.is_empty() => {}
            Ok(items) => log(&format!(
                "{label}: software scrolling: {}",
                items.join(", ")
            )),
            Err(e) => {
                log(&format!("{label}: {e}"));
                // Without software scrolling a diverted wheel would not scroll at all
                let mut has = |id| d.feature_index(id).ok().flatten().is_some();
                let (wheel, thumb) = (has(features::HIRES_WHEEL), has(features::THUMB_WHEEL));
                for e in undivert(d, wheel, thumb) {
                    log(&format!(
                        "{label}: cannot undo the diversion of the wheels: {e}"
                    ));
                }
            }
        }
    }

    /// Gives the wheels back to the OS
    fn stop_scrolling(&mut self) {
        for ((path, index), s) in std::mem::take(&mut self.scrolling) {
            let errors = match self.open(&path, index) {
                Ok(mut d) => undivert(&mut d, s.wheel.is_some(), s.thumb.is_some()),
                Err(e) => vec![e],
            };
            for e in errors {
                log(&format!("Cannot undo the diversion of the wheels: {e}"));
            }
        }
    }
}

/// Undoes the diversion of the wheels, trying each one; returns the failures
fn undivert(d: &mut Hidpp, wheel: bool, thumb: bool) -> Vec<anyhow::Error> {
    let mut errors = vec![];
    if wheel && let Err(e) = device::set_wheel(d, None, None, Some(false)) {
        errors.push(e);
    }
    if thumb && let Err(e) = device::set_thumbwheel(d, None, Some(false)) {
        errors.push(e);
    }
    errors
}

/// Forwards notifications from one interface
/// until it goes away, or until `stop` is set
fn spawn_reader(dev: HidDevice, path: CString, id: u64, stop: Arc<AtomicBool>, tx: Sender<Event>) {
    thread::spawn(move || {
        let mut buf = [0u8; 64];
        while !stop.load(Ordering::Relaxed) {
            match dev.read_timeout(&mut buf, 1000) {
                Ok(0) => {}
                Ok(n) => {
                    let r = &buf[..n];
                    let is_hidpp = (r[0] == REPORT_SHORT || r[0] == REPORT_LONG) && r.len() >= 4;
                    // Software ID 0: sent by the device on its own, not a response to a request
                    if is_hidpp && r[3] & 0x0f == 0 {
                        let event = Event::Report {
                            path: path.clone(),
                            reader: id,
                            report: r.to_vec(),
                            at: Instant::now(),
                        };
                        if tx.send(event).is_err() {
                            return;
                        }
                    }
                }
                Err(_) => {
                    let _ = tx.send(Event::Closed { path, reader: id });
                    return;
                }
            }
        }
    });
}

fn log(msg: &str) {
    eprintln!("{} {msg}", crate::now_hms());
}
