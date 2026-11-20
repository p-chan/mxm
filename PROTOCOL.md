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
- Logi Options+ talks to the device at the same time, so this tool uses software ID `0xB` to avoid picking up its responses.
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
