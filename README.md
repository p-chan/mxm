# mxm

English | [日本語](README.ja.md)

mxm is a CLI and daemon for configuring Logitech MX Master mice directly over the HID++ 2.0 protocol. Change DPI, SmartShift, the scroll wheels, haptic feedback, button mappings and more without Logi Options+. Write your settings in a config file and apply them once, or have the daemon reapply them whenever a mouse connects. On macOS, `mxm service` starts the daemon at login, and the daemon scrolls the wheels with the same acceleration as Logi Options+. It has been tested on the MX Master 4 and MX Master 3S.

This is an unofficial project and is not affiliated with Logitech.

The results of the protocol analysis are documented in [PROTOCOL.md](PROTOCOL.md).

## Requirements

- macOS on Apple Silicon or Intel (Linux and Windows should work wherever [hidapi](https://crates.io/crates/hidapi) does, but are untested and have no prebuilt binaries)
- An MX Master mouse connected via Bluetooth or a Logi Bolt receiver (tested on the MX Master 4 and MX Master 3S; features a model lacks, such as haptic feedback on the MX Master 3S, report an error)

## Installation

Prebuilt binaries for macOS are signed with a Developer ID and notarized by Apple. Install them with one of the following.

### Homebrew

```sh
brew tap p-chan/tap
brew trust --tap p-chan/tap   # once; Homebrew loads casks only from taps you trust
brew install --cask mxm
```

Update with `brew upgrade --cask mxm`. If you [run the daemon at login](#starting-at-login-macos), the upgrade updates it too.

### mise

```sh
mise use -g github:p-chan/mxm
```

To run the daemon at login and update it with mise, see [Starting at login](#starting-at-login-macos).

### Install script

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/p-chan/mxm/releases/latest/download/mxm-installer.sh | sh   # installs to ~/.local/bin/mxm
```

Run the same command again to update.

### From source

With [Rust](https://www.rust-lang.org/tools/install) installed:

```sh
git clone https://github.com/p-chan/mxm.git
cd mxm
cargo install --path .   # installs to ~/.cargo/bin/mxm
```

A binary you build is not signed with the Developer ID, so macOS forgets the Input Monitoring and Accessibility permissions of the daemon each time you rebuild it. You can also run it without installing via `cargo run -- <command>`.

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
wheel = { hires = true, invert = false, speed = 85, smooth = true }  # speed and smooth: see below
thumbwheel = { invert = true, speed = 50 }
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

### Software scrolling (macOS)

Like Logi Options+, `mxm daemon` diverts the wheels to HID++ and posts the scroll events itself, with the same acceleration as Options+ (see [PROTOCOL.md](PROTOCOL.md#software-scrolling-of-logi-options-macos)). The settings mean the same as in Options+:

- `wheel.speed`: "Scrolling speed", `0`-`100` (default `50`). It sets how much fast spins are accelerated; slow scrolling stays the same.
- `wheel.smooth`: "Smooth scrolling" (default `true`). `false` reports the wheel per notch (low resolution). In `mxm daemon` it takes the place of `hires`.
- `thumbwheel.speed`: thumb wheel "Speed", `0`-`100` (default `50`).
- The scroll direction is set by `invert` only, as with "Scroll direction" in Options+; the natural scrolling setting of macOS does not change it.

`mxm apply` does not scroll, since nothing would scroll after it exits. Differences from Options+: at low thumb wheel speeds Options+ drops a little scrolling for its "swipe between pages" gesture, and in free spin it mutes the wheel right after a middle click; mxm does neither.

- Posting scroll events needs the Accessibility permission. Allow the executable that the log names in System Settings → Privacy & Security → Accessibility; the daemon starts software scrolling within a few seconds after that. Until then the wheels stay with the OS.
- When the daemon stops (e.g. `mxm service uninstall`), it gives the wheels back to the OS. If it is killed or crashes instead, the wheels stay diverted and do not scroll until the mouse is turned off and on, or until `mxm wheel --divert off` and `mxm thumbwheel --divert off`. `mxm service install` restarts a daemon that exits.
- Quit Logi Options+, which diverts the wheels too; with both running, every scroll is posted twice.

### Starting at login (macOS)

```sh
mxm service install       # register mxm daemon as a LaunchAgent and start it (and at every login)
mxm service update        # update the daemon to this mxm after updating mxm
mxm service status        # whether it is registered and running
mxm service uninstall     # stop and unregister it
tail -f ~/Library/Logs/mxm.log
```

`mxm service install` copies the running `mxm` to `~/Library/Application Support/mxm/mxm` and writes `~/Library/LaunchAgents/io.github.p-chan.mxm.plist` that runs the copy. After updating `mxm`, run `mxm service update` to replace the copy and restart the daemon (`mxm service status` tells when the copy is missing or differs from the `mxm` you run). `mxm service update` keeps the plist as it is, and does nothing if the service is not installed. The config path is fixed when you run it (the default `~/.config/mxm/config.toml`, or `$XDG_CONFIG_HOME/mxm/config.toml` if it is set, unless `--config <path>` is given); run it again after changing it. To manage the plist yourself (e.g. in dotfiles), print it with `mxm service install --print` instead; it runs the copy at the path above, which `--print` does not create.

When started by launchd, `mxm` needs the Input Monitoring permission to open mice connected over Bluetooth (the log shows `not permitted` without it). `mxm service install` shows the executable to add in System Settings → Privacy & Security → Input Monitoring. Mice behind a Logi Bolt receiver work without it. [Software scrolling](#software-scrolling-macos) also needs the Accessibility permission for the same executable. macOS keeps the permission per executable path, and release binaries are signed with a Developer ID, so it carries over when `mxm service update` replaces the copy with an updated `mxm`, however you installed it (e.g. mise installs each version into a different directory).

With [mise](https://mise.jdx.dev/), a `postinstall` command can install the daemon, or update it, whenever mise installs a version:

```toml
[tools]
"github:p-chan/mxm" = { version = "latest", postinstall = "mxm service install" }
```

`mxm service install` writes the plist again each time, so add `--config <path>` to the command if you use one. To only update a daemon you installed yourself, use `mxm service update` instead.

With the Homebrew cask, `brew upgrade --cask mxm` runs `mxm service update` for you.

Other platforms are not supported by `mxm service` yet; run `mxm daemon` with your service manager (e.g. a systemd user service on Linux).

## Notes

- While Logi Options+ is running, it may overwrite settings and divert buttons and wheels to HID++. Quit Options+ if you want to keep the settings you change with this tool.
- After the mouse is turned off and on, the haptic level, SmartShift and the haptic sense panel force are kept, but DPI, high-resolution scrolling, the thumb wheel direction, button remapping and diversion return to their defaults (see [PROTOCOL.md](PROTOCOL.md#settings-after-a-power-cycle)). `mxm daemon` writes them again on every connection (see [Config file and resident mode](#config-file-and-resident-mode)).
- The left and right buttons cannot be remapped (the device does not allow it), but other buttons can be mapped to left or right click.

## Acknowledgements

Logitech does not publish the HID++ 2.0 specification. The feature names, message layouts and device behavior used by this tool were cross-checked against [Solaar](https://github.com/pwr-Solaar/Solaar), a manager for Logitech devices (mainly for Linux, with limited macOS support), and verified on a real MX Master 4 and MX Master 3S where possible (see [PROTOCOL.md](PROTOCOL.md) for what is unverified). No code from Solaar is included in this project.

## License

[MIT](LICENSE)
