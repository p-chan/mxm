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

## Notes

- While Logi Options+ is running, it may overwrite settings and divert buttons and wheels to HID++. Quit Options+ if you want to keep the settings you change with this tool.
- After the mouse is turned off and on, the haptic level, SmartShift and the haptic sense panel force are kept, but DPI, high-resolution scrolling, the thumb wheel direction, button remapping and diversion return to their defaults (see [PROTOCOL.md](PROTOCOL.md#settings-after-a-power-cycle)). A resident mode that reapplies settings on every connection is planned.
- The left and right buttons cannot be remapped (the device does not allow it), but other buttons can be mapped to left or right click.

## Acknowledgements

Logitech does not publish the HID++ 2.0 specification. The feature names, message layouts and device behavior used by this tool were cross-checked against [Solaar](https://github.com/pwr-Solaar/Solaar), a manager for Logitech devices (mainly for Linux, with limited macOS support), and verified on a real MX Master 4 and MX Master 3S where possible (see [PROTOCOL.md](PROTOCOL.md) for what is unverified). No code from Solaar is included in this project.
