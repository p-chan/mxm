# mxm

English | [日本語](README.ja.md)

A command-line tool that reads and writes the settings of Logitech MX Master mice directly over the HID++ 2.0 protocol. It has been tested on the MX Master 4 and MX Master 3S. Change DPI, SmartShift, the scroll wheels, haptic feedback, button mappings and more without Logi Options+.

This is an unofficial project and is not affiliated with Logitech.

The results of the protocol analysis are documented in [PROTOCOL.md](PROTOCOL.md).

## Requirements

- macOS (Linux and Windows should work wherever [hidapi](https://crates.io/crates/hidapi) does, but are untested)
- Rust (build time only)
- An MX Master mouse connected via Bluetooth or a Logi Bolt receiver (tested on the MX Master 4 and MX Master 3S; features a model lacks, such as haptic feedback on the MX Master 3S, report an error)

## Installation

```sh
cargo install --path .   # installs to ~/.cargo/bin/mxm
```

You can also run it without installing via `cargo run -- <command>`.

## Development

```sh
cargo test     # decoding tests using responses recorded from a real device
cargo clippy --all-targets
cargo fmt
```

## Usage

```sh
mxm list                                # connected HID++ 2.0 devices
mxm info                                # name, firmware, serial, battery, host
mxm features                            # feature table
mxm battery

mxm dpi                                 # current DPI
mxm dpi 1600                            # change DPI (200-8000, in steps of 50)

mxm smartshift                          # SmartShift status
mxm smartshift --mode ratchet --threshold 12 --torque 75
mxm smartshift --threshold off          # disable SmartShift (always ratchet)

mxm wheel --invert on                   # invert the main wheel
mxm wheel --hires off                   # disable high-resolution scrolling
mxm thumbwheel --invert on              # invert the thumb wheel

mxm haptic                              # haptic feedback status and available waveforms
mxm haptic 60                           # change intensity (1-100)
mxm haptic off
mxm haptic play happy-alert             # play a waveform

mxm force                               # press force of the haptic sense panel
mxm force default

mxm buttons                             # list buttons
mxm buttons remap back middle           # make the back button a middle click
mxm buttons reset back

mxm host                                # list Easy-Switch hosts
mxm host 2                              # switch to host 2 (disconnects from this machine)
```

### Commands for protocol analysis

```sh
mxm --trace dpi                         # print sent and received HID++ reports to stderr
mxm raw 0x2201 2 00                     # call any feature / function (here: getSensorDpi)
mxm raw '#20' 1                         # specify the feature index directly
mxm monitor                             # print notifications from the device
mxm monitor --divert back,gesture --wheel --thumbwheel
                                        # temporarily divert buttons or wheels to HID++ to observe them (restored on exit)
mxm dump                                # dump every readable setting as JSON
```

Add `--json` to get JSON output from any command.

## Config file and resident mode

The mouse forgets most settings when it is turned off (see [Notes](#notes)). `mxm daemon` stays running and writes the settings of a config file to each mouse whenever it connects.

Write the settings in `~/.config/mxm/config.toml` (or `$XDG_CONFIG_HOME/mxm/config.toml`). Settings that are not written are left untouched.

```toml
# Per model, by the name shown by `mxm list` (case-insensitive)
[device."MX Master 4"]
dpi = 1600
smartshift = { mode = "ratchet", threshold = 12, torque = 75 }  # threshold = "off" disables SmartShift
wheel = { hires = true, invert = false }
thumbwheel = { invert = true }
haptic = 60                 # 1-100, or "off"
force = "default"           # press force of the haptic sense panel: a value, or "default"

[device."MX Master 4".buttons]
back = "middle"             # remap a button; "default" restores its default action

# Per mouse, by the serial number shown by `mxm info`; overrides the model settings
[serial."XXXXXXXXXXXX"]
dpi = 2000
```

```sh
mxm apply                   # write the config to every connected mouse once
mxm daemon                  # keep running and write the config on every connection
```

`mxm daemon` writes the config when it starts, when a mouse connects over Bluetooth or through a Logi Bolt receiver, and when a mouse asks to be reconfigured after it is turned on. It reads the config file again each time, so edits take effect on the next connection. Settings a model lacks, such as haptic feedback on the MX Master 3S, are reported as "not supported" and skipped. Quit Logi Options+ while using it, as Options+ also writes its own settings on every connection.

### Starting at login (macOS)

Save the following as `~/Library/LaunchAgents/io.github.p-chan.mxm.plist`, replacing `YOU` with your user name.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>io.github.p-chan.mxm</string>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/YOU/.cargo/bin/mxm</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardErrorPath</key>
  <string>/tmp/mxm.log</string>
</dict>
</plist>
```

```sh
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/io.github.p-chan.mxm.plist   # start (and at every login)
tail -f /tmp/mxm.log                                                                 # log
launchctl bootout gui/$(id -u)/io.github.p-chan.mxm                                  # stop
```

When started by launchd, `mxm` needs the Input Monitoring permission to open mice connected over Bluetooth (the log shows `not permitted` without it). Add `~/.cargo/bin/mxm` in System Settings → Privacy & Security → Input Monitoring. Mice behind a Logi Bolt receiver work without it.

## Notes

- While Logi Options+ is running, it may overwrite settings and divert buttons and wheels to HID++. Quit Options+ if you want to keep the settings you change with this tool.
- After the mouse is turned off and on, the haptic level, SmartShift and the haptic sense panel force are kept, but DPI, high-resolution scrolling, the thumb wheel direction, button remapping and diversion return to their defaults (see [PROTOCOL.md](PROTOCOL.md#settings-after-a-power-cycle)). `mxm daemon` writes them again on every connection (see [Config file and resident mode](#config-file-and-resident-mode)).
- The left and right buttons cannot be remapped (the device does not allow it), but other buttons can be mapped to left or right click.

## Acknowledgements

Logitech does not publish the HID++ 2.0 specification. The feature names, message layouts and device behavior used by this tool were cross-checked against [Solaar](https://github.com/pwr-Solaar/Solaar), a manager for Logitech devices (mainly for Linux, with limited macOS support), and verified on a real MX Master 4 and MX Master 3S where possible (see [PROTOCOL.md](PROTOCOL.md) for what is unverified). No code from Solaar is included in this project.

## License

[MIT](LICENSE)
