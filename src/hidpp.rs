//! HID++ 2.0 transport
//!
//! Message format (long report, 20 bytes):
//!   [0] report ID   0x10 = short (7 bytes), 0x11 = long (20 bytes)
//!   [1] device index  0xFF = direct (Bluetooth / USB), 1..6 = receiver slot
//!   [2] feature index  looked up from the feature ID with Root.getFeature
//!   [3] function ID (upper 4 bits) | software ID (lower 4 bits)
//!   [4..] parameters
//!
//! Error responses:
//!   HID++ 2.0: [0x11, idx, 0xFF, featureIndex, fn|sw, errorCode, ...]
//!   HID++ 1.0: [0x10, idx, 0x8F, subId, address, errorCode, ...]

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};

pub const LOGITECH_VID: u16 = 0x046d;
pub const REPORT_SHORT: u8 = 0x10;
pub const REPORT_LONG: u8 = 0x11;
pub const DIRECT_INDEX: u8 = 0xff;
const LONG_LENGTH: usize = 20;
/// A distinct software ID so responses are not confused with those of other clients such as Logi Options+
pub const SW_ID: u8 = 0x0b;
/// Software ID of `mxm daemon`, so that it and the CLI can talk to a device at the same time
pub const DAEMON_SW_ID: u8 = 0x0c;

/// Parameter part of a response
pub type Params = [u8; 16];

#[derive(Debug)]
pub enum HidppError {
    /// An error returned by the device
    Protocol { message: String },
    /// No response
    Timeout(String),
    /// The feature is not supported
    Unsupported(u16),
    /// HID I/O failure
    Io(String),
}

impl fmt::Display for HidppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol { message, .. } | Self::Timeout(message) | Self::Io(message) => {
                f.write_str(message)
            }
            Self::Unsupported(id) => {
                write!(
                    f,
                    "Feature 0x{id:04x} ({}) is not supported by this device",
                    crate::features::feature_name(*id)
                )
            }
        }
    }
}

impl std::error::Error for HidppError {}

impl From<hidapi::HidError> for HidppError {
    fn from(e: hidapi::HidError) -> Self {
        Self::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, HidppError>;

fn hidpp20_error(code: u8) -> &'static str {
    match code {
        0x00 => "NoError",
        0x01 => "Unknown",
        0x02 => "InvalidArgument",
        0x03 => "OutOfRange",
        0x04 => "HardwareError",
        0x05 => "LogitechInternal",
        0x06 => "InvalidFeatureIndex",
        0x07 => "InvalidFunctionId",
        0x08 => "Busy",
        0x09 => "Unsupported",
        _ => "?",
    }
}

fn hidpp10_error(code: u8) -> &'static str {
    match code {
        0x01 => "InvalidSubId",
        0x02 => "InvalidAddress",
        0x03 => "InvalidValue",
        0x04 => "ConnectFail",
        0x05 => "TooManyDevices",
        0x06 => "AlreadyExists",
        0x07 => "Busy",
        0x08 => "UnknownDevice",
        0x09 => "ResourceError",
        0x0a => "RequestUnavailable",
        0x0b => "InvalidParamValue",
        0x0c => "WrongPinCode",
        _ => "?",
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Something that can be called by feature ID. Tests replace it with a fake that returns recorded responses
pub trait FeatureCall {
    fn call(&mut self, feature_id: u16, function: u8, params: &[u8]) -> Result<Params>;
    fn has(&mut self, feature_id: u16) -> bool;
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub feature_index: u8,
    pub function: u8,
    pub params: Params,
    pub raw: Vec<u8>,
}

/// Communication with one HID++ device (one slot when behind a receiver)
pub struct Hidpp {
    dev: HidDevice,
    device_index: u8,
    feature_cache: HashMap<u16, u8>,
    pending: VecDeque<Vec<u8>>,
    pub trace: bool,
    pub sw_id: u8,
}

impl Hidpp {
    pub fn new(dev: HidDevice, device_index: u8) -> Self {
        Self {
            dev,
            device_index,
            feature_cache: HashMap::new(),
            pending: VecDeque::new(),
            trace: false,
            sw_id: SW_ID,
        }
    }

    /// Switches to another slot of the same HID device (for scanning receivers)
    pub fn set_device_index(&mut self, index: u8) {
        self.device_index = index;
        self.feature_cache.clear();
        self.pending.clear();
    }

    fn write(&self, msg: &[u8]) -> Result<()> {
        if self.trace {
            eprintln!("-> {}", hex(msg));
        }
        self.dev.write(msg)?;
        Ok(())
    }

    fn read(&self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let mut buf = [0u8; 64];
        let n = self
            .dev
            .read_timeout(&mut buf, timeout.as_millis().max(1) as i32)?;
        if n == 0 {
            return Ok(None);
        }
        let r = &buf[..n];
        // Ignore regular HID reports such as mouse movement
        if (r[0] != REPORT_SHORT && r[0] != REPORT_LONG) || r.len() < 4 || r[1] != self.device_index
        {
            return Ok(None);
        }
        if self.trace {
            eprintln!("<- {}", hex(r));
        }
        Ok(Some(r.to_vec()))
    }

    /// Sends a request to a feature index and function ID, and returns the parameters of the response
    pub fn request(
        &mut self,
        feature_index: u8,
        function: u8,
        params: &[u8],
        timeout: Duration,
    ) -> Result<Params> {
        let mut msg = [0u8; LONG_LENGTH];
        msg[0] = REPORT_LONG;
        msg[1] = self.device_index;
        msg[2] = feature_index;
        msg[3] = ((function & 0x0f) << 4) | (self.sw_id & 0x0f);
        let len = params.len().min(LONG_LENGTH - 4);
        msg[4..4 + len].copy_from_slice(&params[..len]);
        self.write(&msg)?;

        let deadline = Instant::now() + timeout;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Some(r) = self.read(left.min(Duration::from_millis(200)))? else {
                continue;
            };
            let at = |i: usize| r.get(i).copied().unwrap_or(0);
            if at(2) == 0xff && at(3) == feature_index && at(4) == msg[3] {
                let code = at(5);
                return Err(HidppError::Protocol {
                    message: format!(
                        "HID++ 2.0 error {} (0x{code:02x}) (feature index 0x{feature_index:02x}, fn {function})",
                        hidpp20_error(code)
                    ),
                });
            }
            if at(0) == REPORT_SHORT && at(2) == 0x8f && at(3) == feature_index {
                let code = at(5);
                return Err(HidppError::Protocol {
                    message: format!("HID++ 1.0 error {} (0x{code:02x})", hidpp10_error(code)),
                });
            }
            if at(2) == feature_index && at(3) == msg[3] {
                return Ok(params_of(&r));
            }
            // Keep unrelated reports (such as notifications) for `monitor`
            if at(3) & 0x0f == 0 {
                self.pending.push_back(r);
            }
        }
        Err(HidppError::Timeout(format!(
            "No response from device (index 0x{:02x}, feature index 0x{feature_index:02x}, fn {function})",
            self.device_index
        )))
    }

    /// Returns the protocol version (major, minor), or None if the device does not respond
    pub fn ping(&mut self, timeout: Duration) -> Result<Option<(u8, u8)>> {
        match self.request(0x00, 1, &[0, 0, 0x5a], timeout) {
            Ok(r) => Ok(Some((r[0], r[1]))),
            Err(HidppError::Protocol { .. } | HidppError::Timeout(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Reads a 3-byte HID++ 1.0 register (e.g. of a receiver)
    pub fn read_register(&mut self, register: u8) -> Result<[u8; 3]> {
        let r = self.register_request(0x81, register, [0; 3])?;
        Ok([r[4], r[5], r[6]])
    }

    /// Writes a 3-byte HID++ 1.0 register (e.g. of a receiver)
    pub fn write_register(&mut self, register: u8, value: [u8; 3]) -> Result<()> {
        self.register_request(0x80, register, value)?;
        Ok(())
    }

    /// HID++ 1.0 short register request: [0x10, idx, 0x80 (set) / 0x81 (get), register, value...]
    fn register_request(&mut self, sub_id: u8, register: u8, value: [u8; 3]) -> Result<Vec<u8>> {
        let msg = [
            REPORT_SHORT,
            self.device_index,
            sub_id,
            register,
            value[0],
            value[1],
            value[2],
        ];
        self.write(&msg)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Some(r) = self.read(left.min(Duration::from_millis(200)))? else {
                continue;
            };
            let at = |i: usize| r.get(i).copied().unwrap_or(0);
            if at(2) == 0x8f && at(3) == sub_id && at(4) == register {
                let code = at(5);
                return Err(HidppError::Protocol {
                    message: format!(
                        "HID++ 1.0 error {} (0x{code:02x}) (register 0x{register:02x})",
                        hidpp10_error(code)
                    ),
                });
            }
            if at(2) == sub_id && at(3) == register && r.len() >= 7 {
                return Ok(r);
            }
        }
        Err(HidppError::Timeout(format!(
            "No response to register 0x{register:02x} (index 0x{:02x})",
            self.device_index
        )))
    }

    /// Looks up the feature index of a feature ID, or None if unsupported
    pub fn feature_index(&mut self, feature_id: u16) -> Result<Option<u8>> {
        if feature_id == 0 {
            return Ok(Some(0));
        }
        let index = match self.feature_cache.get(&feature_id) {
            Some(&i) => i,
            None => {
                let r = self.request(0x00, 0, &feature_id.to_be_bytes(), Duration::from_secs(2))?;
                self.feature_cache.insert(feature_id, r[0]);
                r[0]
            }
        };
        Ok((index != 0).then_some(index))
    }

    /// Waits for a notification from the device (a report with software ID 0)
    pub fn read_notification(&mut self, timeout: Duration) -> Result<Option<Notification>> {
        let r = match self.pending.pop_front() {
            Some(r) => r,
            None => match self.read(timeout)? {
                Some(r) => r,
                None => return Ok(None),
            },
        };
        Ok(Some(Notification {
            feature_index: r[2],
            function: r[3] >> 4,
            params: params_of(&r),
            raw: r,
        }))
    }
}

impl FeatureCall for Hidpp {
    fn call(&mut self, feature_id: u16, function: u8, params: &[u8]) -> Result<Params> {
        let index = self
            .feature_index(feature_id)?
            .ok_or(HidppError::Unsupported(feature_id))?;
        self.request(index, function, params, Duration::from_secs(2))
    }

    fn has(&mut self, feature_id: u16) -> bool {
        matches!(self.feature_index(feature_id), Ok(Some(_)))
    }
}

fn params_of(r: &[u8]) -> Params {
    let mut p = [0u8; 16];
    let src = r.get(4..).unwrap_or(&[]);
    let n = src.len().min(16);
    p[..n].copy_from_slice(&src[..n]);
    p
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Bluetooth,
    Receiver,
    Usb,
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bluetooth => "bluetooth",
            Self::Receiver => "receiver",
            Self::Usb => "usb",
        })
    }
}

#[derive(Debug, Clone)]
pub struct Interface {
    pub path: std::ffi::CString,
    pub product_id: u16,
    pub transport: Transport,
    pub hid_product: String,
}

/// Lists Logitech interfaces that speak HID++
pub fn list_interfaces(api: &HidApi) -> Vec<Interface> {
    let mut out: Vec<Interface> = Vec::new();
    for d in api.device_list() {
        if d.vendor_id() != LOGITECH_VID {
            continue;
        }
        // 0xFF43: HID++ collection of a device connected directly over Bluetooth LE
        // 0xFF00 usage 0x02: HID++ long report collection of a receiver or USB device
        let is_ble = d.usage_page() == 0xff43;
        let is_vendor = d.usage_page() == 0xff00 && d.usage() == 0x02;
        if !is_ble && !is_vendor {
            continue;
        }
        if out.iter().any(|i| i.path.as_c_str() == d.path()) {
            continue;
        }
        let product = d.product_string().unwrap_or_default().to_string();
        let is_receiver = is_vendor && product.to_lowercase().contains("receiver");
        out.push(Interface {
            path: d.path().to_owned(),
            product_id: d.product_id(),
            transport: if is_ble {
                Transport::Bluetooth
            } else if is_receiver {
                Transport::Receiver
            } else {
                Transport::Usb
            },
            hid_product: product,
        });
    }
    out
}
