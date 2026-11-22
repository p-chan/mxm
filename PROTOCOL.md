# MX Master HID++ 2.0 Protocol

English | [日本語](PROTOCOL.ja.md)

These notes come from sending and receiving messages on a real MX Master 4 (firmware RBM27.02_0017, connected via Bluetooth LE, PID `0xB042`). Differences on the MX Master 3S are listed in [MX Master 3S](#mx-master-3s). The meaning of each feature was cross-checked against the implementation in [Solaar](https://github.com/pwr-Solaar/Solaar). Statements that were not verified on the device are marked as such.

## Transport

| Connection | HID collection | Device index |
| --- | --- | --- |
| Bluetooth LE | usage page `0xFF43`, usage `0x0202` | `0xFF` |
| Logi Bolt receiver (PID `0xC548`) | usage page `0xFF00`, usage `0x0002` | pairing slot `1`-`6` |

Reading the receiver's HID++ 1.0 register `0xB5` (sub-addresses `0x5N` for pairing information and `0x6N` for the name of slot N) shows which devices are paired. On the observed receiver, the MX Master 4 was paired in slot 2 with the same unit ID as over Bluetooth, and after switching the mouse to host 3 the tool talked to it as device index `0x02`. After about 45 seconds of inactivity, the mouse took up to about 1.4 seconds to answer the first request through the receiver, presumably while waking from power saving, so the tool waits up to 2 seconds per slot.

On macOS, hidapi can read from and write to both the Bluetooth LE mouse and the receiver without detaching the OS driver. With the Bluetooth LE mouse, regular input reports such as mouse movement are also read from the same device, so HID++ reports are selected by their report ID.

hidapi opens devices exclusively on macOS by default. While one process holds a device, others fail to open it with `exclusive access and device already open`, so the tool opens devices in non-exclusive mode.

## Message format

The tool uses long reports (20 bytes).

| Byte | Content |
| --- | --- |
| 0 | Report ID (`0x10` = short, 7 bytes; `0x11` = long, 20 bytes) |
| 1 | Device index |
| 2 | Feature index |
| 3 | Upper 4 bits: function ID; lower 4 bits: software ID |
| 4-19 | Parameters |

- A response carries the same feature index and function / software ID as its request. Reports with software ID `0` are notifications that the device sends on its own.
- Logi Options+ talks to the device at the same time, so this tool uses software ID `0xB` to avoid picking up its responses. `mxm daemon` uses `0xC`, so that it and other `mxm` commands can run at the same time.
- HID++ 2.0 errors come back as `[0x11, idx, 0xFF, featureIndex, fn|sw, errorCode]`.
- To look up a feature index from a feature ID, call function 0 `getFeature(featureId)` of Root (index 0).

Example: reading the DPI

```text
-> 11 ff 14 2b 00 00 00 ...    feature index 0x14 (AdjustableDpi), fn 2 getSensorDpi(sensor 0), sw 0xB
<- 11 ff 14 2b 00 09 60 03 e8  sensor 0, dpi 0x0960 = 2400, default 0x03E8 = 1000
```

## Feature table

Output of `mxm features`. Names follow Solaar; features it does not name are listed as Unknown. Feature indexes differ between models and firmware versions, so the tool always looks them up by feature ID.

| Index | ID | Name | Notes |
| ---: | --- | --- | --- |
| 1 | `0x0001` | FeatureSet | |
| 2 | `0x0003` | DeviceInfo | |
| 3 | `0x0005` | DeviceName | |
| 4 | `0x1D4B` | WirelessDeviceStatus | |
| 5 | `0x0020` | ConfigChange | |
| 6 | `0x0021` | CryptoId | |
| 7 | `0x0007` | DeviceFriendlyName | |
| 8 | `0x0011` | PropertyAccess | |
| 9 | `0x1004` | UnifiedBattery | |
| 10 | `0x1701` | Unknown | |
| 11 | `0x19B0` | Haptic | |
| 12 | `0x19C0` | ForceSensingButton | |
| 13 | `0x1B04` | ReprogControlsV4 | |
| 14 | `0x1814` | ChangeHost | |
| 15 | `0x1815` | HostsInfo | |
| 16 | `0x2250` | XYStats | |
| 17 | `0x2111` | SmartShiftEnhanced | |
| 18 | `0x2121` | HiResWheel | |
| 19 | `0x2150` | ThumbWheel | |
| 20 | `0x2201` | AdjustableDpi | |
| 21 | `0x2251` | WheelStats | |
| 22 | `0x00D1` | Unknown | |
| 23-45 | `0x18xx`, `0x1Exx`, `0x9xxx` and others | Diagnostics / manufacturing | Flagged hidden (most are also flagged engineering) |

## Settings after a power cycle

Observed by changing settings with Logi Options+ stopped, turning the mouse off and on with its power switch, and reading the settings back over Bluetooth LE.

| Setting | After the power cycle |
| --- | --- |
| Haptic level (`0x19B0`) | Kept |
| SmartShift mode, threshold and torque (`0x2111`) | Kept |
| Haptic sense panel force (`0x19C0`) | Kept |
| DPI (`0x2201`) | Reset to the default (1000) |
| High-resolution scrolling (`0x2121`) | Reset to off |
| Thumb wheel direction (`0x2150`) | Reset to normal |
| Button remapping and diversion (`0x1B04`) | Reset |
| analyticsKeyEvents reporting (`0x1B04`) | Reset to disabled |

Logi Options+ does not undo its diversions when it exits: after it was stopped, the main wheel, the thumb wheel and the back, forward, gesture and haptic sense panel buttons stayed diverted, so they did not reach the OS until the diversion was turned off.

## Connection events

Observed by turning an MX Master 3S (Logi Bolt receiver) and an MX Master 4 (Bluetooth LE) off and on with their power switches. `mxm daemon` uses these events to write the settings again.

- **Logi Bolt receiver**: the receiver sends the HID++ 1.0 notification `[0x10, slot, 0x41, protocol, flags, wpid (little-endian)]`. Turning the MX Master 3S off produced `10 02 41 10 42 34 b0` (bit6 of `flags` set: link not established), and turning it on produced `10 02 41 10 02 34 b0`. The receiver sends this notification only while bit `0x000100` (wireless) of its HID++ 1.0 register `0x00` is set. Both observed receivers already had it set (`0x000100` and `0x000900`, presumably by Logi Options+); `mxm daemon` sets the bit when it is missing and keeps the other bits.
- **WirelessDeviceStatus `0x1D4B`**: right after the connection notification, the MX Master 3S sent notification fn 0 `01 01 01`: `status, request (1: software reconfiguration needed), reason (1: powered on)`.
- **Bluetooth LE**: turning the MX Master 4 off closes its HID device (reads fail with "device disconnected"). Turning it on makes it appear again as a new HID device, which answered requests about 0.5 seconds later. Whether it also sends the `0x1D4B` notification was not observed.
- **Input Monitoring**: a process started by launchd could open the receiver, but opening the Bluetooth LE mouse failed with `(iokit/common) not permitted` until the Input Monitoring permission was granted to the executable.

Whether a mouse that wakes from power saving (without being turned off) loses settings or sends `0x1D4B` has not been checked.

## Features

Parameters are counted from byte 4 of the report, starting at 0.

### AdjustableDpi `0x2201`

| fn | Name | Request | Response |
| --- | --- | --- | --- |
| 1 | getSensorDpiList | `sensor` | `sensor, dpi[]` (u16 values, terminated by `0`) |
| 2 | getSensorDpi | `sensor` | `sensor, dpi (u16), defaultDpi (u16)` |
| 3 | setSensorDpi | `sensor, dpi (u16)` | |

A value in the DPI list whose upper 3 bits are `111` means "from the previous value to the next value, in steps of the lower 13 bits". The MX Master 4 returns `00c8 e032 1f40`, i.e. 200-8000 in steps of 50.

### SmartShiftEnhanced `0x2111`

The MX Master 3S has the older SmartShift `0x2110` instead; see [MX Master 3S](#mx-master-3s).

| fn | Name | Request / response |
| --- | --- | --- |
| 0 | getCapabilities | `flags (bit0: torque is tunable), default threshold, default torque, ...` |
| 1 | getRatchetControlMode | `wheelMode (1: freespin, 2: ratchet), autoDisengage, torque` |
| 2 | setRatchetControlMode | Same as above. `0` means "unchanged" |

- `autoDisengage` is the SmartShift threshold, `1`-`254`. `255` disables SmartShift (the wheel stays in `wheelMode`).
- `torque` is the ratchet resistance, `1`-`100` (default 75). According to Solaar, the MX Master 4 ignores a torque change unless `wheelMode` is written together with it. On the observed device, writing `00 00 32` (torque only) did change the value read back to 50, but whether the physical resistance changed has not been verified. This tool always writes all three values.

### HiResWheel `0x2121`

| fn | Name | Request / response |
| --- | --- | --- |
| 0 | getWheelCapability | `multiplier (15), flags` |
| 1 | getWheelMode | `mode` |
| 2 | setWheelMode | `mode` |
| 3 | getRatchetSwitchState | `state (bit0: ratchet)` |

In `mode`, bit0 = divert to HID++, bit1 = high resolution, bit2 = inverted direction. While the wheel is diverted, notification fn 0 delivers `flags (bit4: high resolution, bits 0-3: periods), delta (s16)` (observed on the device, e.g. `11 fffe` = high resolution, 1 period, delta -2). Notification fn 1 delivers the new ratchet state (`state`, bit0: ratchet) as soon as the SmartShift button is pressed (observed on the device while the wheel was diverted; whether it is also sent without diversion has not been checked).

### ThumbWheel `0x2150`

| fn | Name | Request / response |
| --- | --- | --- |
| 0 | getInfo | `nativeRes (u16), divertedRes (u16), defaultDirection, capabilities, timeElapsed (u16)` |
| 1 | getStatus | `reportingMode (0: HID, 1: HID++), invert (bit0)` |
| 2 | setStatus | Same as above |

While the thumb wheel is diverted, notification fn 0 delivers `rotation (s16), timestamp (u16), status` (observed on the device). `status` moves through `1` start → `2` active → `3` stop → `0` inactive over one gesture.

### Haptic `0x19B0`

| fn | Name | Request / response |
| --- | --- | --- |
| 0 | getCapabilities | Bytes 4-7 are a bit mask of the playable waveforms (`0x08007FFF`) |
| 1 | getConfig | `enabled (bit0), level (0-100), flags (bit0: four levels only)` |
| 2 | setConfig | `enabled, level` |
| 4 | playWaveform | `waveform` |

Waveform IDs are `0x00`-`0x0E` and `0x1B` (`mxm haptic` lists them).

### ForceSensingButton `0x19C0`

The press force threshold of the haptic sense panel (the thumb press button).

| fn | Name | Request / response |
| --- | --- | --- |
| 0 | getCount | `count (1)` |
| 1 | getButtonInfo | Request `button`; response `flags (u16, bit0: changeable), default (u16), max (u16), min (u16)` |
| 2 | getButtonConfig | Request `button`; response `current (u16)` |
| 3 | setButtonConfig | `button, value (u16)` |

On the MX Master 4 the range is 4578-7610 and the default is 5722.

### ReprogControlsV4 `0x1B04`

| fn | Name | Request / response |
| --- | --- | --- |
| 0 | getCount | `count` |
| 1 | getCidInfo | Request `index`; response `cid (u16), taskId (u16), flags, position, group, groupMask, flags2` |
| 2 | getCidReporting | Request `cid (u16)`; response `cid (u16), flags, remap (u16), flags2` |
| 3 | setCidReporting | `cid (u16), flags, remap (u16)` |

In the `flags` of `setCidReporting`, bit0 is divert / bit1 makes divert valid, bit2 is persist / bit3 makes persist valid, and bit4 is rawXY / bit5 makes rawXY valid. A `remap` of `0` leaves the mapping unchanged, and writing the control's own CID restores the default. A control can only be mapped to CIDs in the groups selected by the bits of its `groupMask`.

Buttons on the MX Master 4:

| CID | Button | Group | groupMask |
| --- | --- | ---: | ---: |
| `0x0050` | Left click | 1 | `0x00` |
| `0x0051` | Right click | 1 | `0x00` |
| `0x0052` | Middle click (wheel press) | 2 | `0x03` |
| `0x0053` | Back | 2 | `0x03` |
| `0x0056` | Forward | 2 | `0x03` |
| `0x00C3` | Gesture button (side button cluster, in front of back / forward) | 2 | `0x03` |
| `0x00C4` | SmartShift button (behind the wheel) | 2 | `0x03` |
| `0x01A0` | Haptic sense panel (thumb rest) | 2 | `0x03` |
| `0x00D7` | Virtual gesture button | 3 | `0x00` |

Buttons in group 2 have a groupMask of `0x03`, so they can be mapped to any control in group 1 (left / right click) or group 2. The left and right buttons themselves have a groupMask of `0x00` and cannot be remapped.

Notifications (observed on the device):

- fn 0: sent when a diverted button is pressed or released, with the CIDs currently held down (up to 4 u16 values). All zeros means every button was released.
- fn 2 (analyticsKeyEvents): `cid (u16), event (1: down, 0: up)` for every button, including the left and right buttons that are not diverted. Solaar names this notification but does not decode it. On the observed device, bit0 of `flags2` in `getCidReporting` was `1` for every button, which appears to be what enables this notification (an inference: Logi Options+ was running and presumably enabled it).
### UnifiedBattery `0x1004`

The response of fn 1 getStatus is `stateOfCharge (%), level (bit0 critical, bit1 low, bit2 good, bit3 full), chargingStatus (0 discharging, 1 charging, 2 charging slowly, 3 complete, 4 error), externalPower`. A notification in the same format arrives as fn 0 (observed on the device: plugging in a USB-C cable produced `charging` about 6 seconds later).

### ChangeHost `0x1814`

fn 0 getHostInfo returns `hostCount (3), currentHost (0-based)`, and fn 1 setCurrentHost takes `host`. On the observed device, setCurrentHost returned a response before the connection to the current host dropped. Switching from host 3 (Logi Bolt receiver) to host 2 (Bluetooth LE) on the same Mac reconnected over Bluetooth within about 2 seconds.

In HostsInfo `0x1815` fn 1 getHostInfo, the third byte appears to be the bus type: hosts 1 and 2 (Bluetooth LE) returned `0x04` and host 3 (Logi Bolt receiver) returned `0x05`. This mapping is inferred from a single device.

### DeviceInfo `0x0003` / DeviceName `0x0005`

- DeviceInfo fn 0: `entityCount, unitId (4), transport (u16), modelId (u16 × 3), extendedModelId, capabilities`
- DeviceInfo fn 1 getFwInfo(entity): `type, prefix (3 characters), number (BCD), revision (BCD), build (BCD u16), active, pid (u16)`
- DeviceInfo fn 2: serial number (12 ASCII characters)
- DeviceName fn 0: length of the name; fn 1 (offset): part of the name; fn 2: device type (`3` = mouse)

## Software scrolling of Logi Options+ (macOS)

`mxm daemon` scrolls the way Logi Options+ 2.x does on macOS. The steps below reproduce recordings of Options+ with an MX Master 4 over Bluetooth: the HID++ input of each wheel and the scroll events Options+ posted, replayed event by event. For the main wheel 95-98% of the replayed events are within 1 pixel of Options+ and the totals within ±1.2% at speeds 0, 50, 85 and 100%. For the thumb wheel the total is within 5% at 100%.

Options+ talks HID++ to a Bluetooth mouse over the Logitech GATT service `00010000-0000-1000-8000-011F2000046D` (characteristic `00010001-…`, notify), not over the HID interface, so diverted-wheel notifications it caused do not reach other HID clients. Options+ stores the scroll speed in its settings as `(percent - 50) / 50`.

### Main wheel

Options+ always diverts the main wheel, in high resolution (15 units per notch) with smooth scrolling on and in low resolution (1 unit per notch) without. Each notification of `n` periods is processed `n` times with the same `delta`, and each pass posts one event at once:

1. Direction hysteresis (high resolution only): counters for each direction swallow the first units after the wheel turns the other way. They hold at most 1 unit in ratchet mode (4 in free spin), and after the wheel rests for whole seconds they move back to 1 (4) by one per second. Turning one way fills the counter of the other way; turning the other way first spends it.
2. `inches = units / multiplier × 7/54` (one notch is 7/54 inch).
3. `dt` = time since the previous notification / `n`, kept within 0.008-0.1 s. A pause of 0.1 s or more resets the smoothed speed.
4. `speed = inches / dt`; `smoothed = 0.95 × smoothed + 0.05 × |speed|`.
5. Gain: piecewise linear in `smoothed` through (0, 1), (0.3, 1), (0.6, 1 + 11s), (60, 1 + 11s), (120, 1 + 5s), where `s = 0.06 + 1.14 × percent / 100`. The plateau is 1.44 px/unit at 0%, 6.85 at 50%, 10.65 at 85% and 12.28 at 100% (measured).
6. `pixels = inches × gain × 100`.

### Thumb wheel

Options+ always diverts the thumb wheel too, and scrolls it with a 16.6 ms timer that repeats the last movement:

- `m = 2^(4 × percent / 100 - 1)` (0.5 at 0%, 8 at 100%); `rotation` is the last reported rotation.
- A gesture starts with the first rotation and ends on status `3` (stop) or `0`, after one more event. A level `L` starts at 1; each new report raises it by `(1 - L) × 0.3`. After 7 ticks without a report it shrinks by ×0.15 per tick and the gesture ends below 0.01.
- Each tick: `v = m × rotation × L / (360 × 0.0166)` inches/s; `smoothed` follows `v` at once when `|v|` is larger, else `0.99 × smoothed + 0.01 × v`; gain piecewise linear in `|smoothed|` through (0, 4), (0.3, 8), (20, 30), (100, 4); `pixels = gain × v × 100 × 0.0166`. At 100% one unit gives 20.4 px per tick (20-21 measured).
- Options+ also posts a "swipe between pages" gesture from the thumb wheel and drops scroll events while that gesture dominates, which reduces scrolling at low speeds (about 15% at 0%). mxm does not post the gesture.

### Posted events

Both wheels post continuous scroll events (`CGEventCreateScrollWheelEvent2` in pixels) opposite to the reported movement; the direction setting of Options+ is the `invert` bit of the device.

- Pixels (fields 96/97): the fraction of a pixel carries over to the next event, and the integer part (toward zero) is posted. No event is posted while it is 0. A thumb wheel gesture starts with no fraction.
- Lines (fields 11/12) and fixed-point lines (93/94): the posted pixels and the wheel movement in inches add up per axis (reset when the direction changes). When an event moves 0.02 inch or more, or the movement adds up to more than 0.06 inch, the lines are the added pixels / 10 (truncated, at least ±1; the fixed-point value is rounded × 65536) and both sums start over; otherwise 0. In ratchet mode the main wheel movement counts in whole notches.
- Scroll phase (99), momentum phase (123) and field 137 (`NSEvent.isDirectionInvertedFromDevice`) are 0.

## MX Master 3S

Observed on an MX Master 3S (firmware RBM22.00_0003, connected via a Logi Bolt receiver, model ID `0xB034`). Everything else in this document applies unless listed here.

- It has no SmartShiftEnhanced `0x2111`, Haptic `0x19B0` or ForceSensingButton `0x19C0`.
- It has the older SmartShift `0x2110`, which has no torque:

  | fn | Name | Request / response |
  | --- | --- | --- |
  | 0 | getRatchetControlMode | `wheelMode (1: freespin, 2: ratchet), autoDisengage, autoDisengageDefault` |
  | 1 | setRatchetControlMode | `wheelMode, autoDisengage, autoDisengageDefault`. `0` means "unchanged" (writing `00 00 00` left the state unchanged) |

- Its buttons are the same as on the MX Master 4 except that there is no haptic sense panel (`0x01A0`).
- The native resolution of the thumb wheel is 18 (20 on the MX Master 4).
