# dual-battery-tray

A lightweight Rust/Linux tray application that monitors the battery levels of a **Keychron K2 HE ISO RGB** keyboard, a **Logitech G PRO X 2 LIGHTSPEED** headset, and a **Keychron M5** mouse.

The application runs as a single background process, displays all battery levels in the tray menu, and uses the standard **Breeze/Breeze-dark battery icon** where available.

A native Linux/XDG desktop notification is shown when any device drops **strictly below 20%**. Each device triggers at most one notification per low-battery period; once it reaches 20% or higher again, notifications are re-armed for the next drop.

## Resource usage

The application polls all devices only once every 10 minutes by default. The headset battery is queried by invoking the `headsetcontrol` binary once per poll; there is no daemon or persistent connection. The Keychron Link HID channel is kept open in one dedicated thread and is reopened only after an I/O error. The mouse battery is polled by a second dedicated thread that keeps the Ultra-Link 8K receiver's vendor HID interface open under the same policy. The tray is updated only when the battery value or status actually changes. Tokio runs as a single-threaded runtime.

The polling interval can be changed with `refresh_seconds`; values below 60 seconds are rejected.

## Notifications

Desktop notifications are sent through [`notify-rust`](https://docs.rs/notify-rust/latest/notify_rust/), using the Linux/XDG desktop notification service.

Default configuration:

```toml
low_battery_threshold = 20
```

The comparison is intentionally `< 20`: exactly 20% does **not** trigger a warning.

The threshold can be changed in `~/.config/dual-battery-tray/config.toml`.

## Supported hardware

```text
Headset: Logitech G PRO X 2 LIGHTSPEED
Headset VID/PID: 046d:0af7 (Logitech LIGHTSPEED USB receiver)
Connection: LIGHTSPEED 2.4 GHz dongle
Battery source: HeadsetControl
Keyboard: Keychron K2 HE ISO RGB
Keyboard VID/PID: 3434:0E21
2.4 GHz receiver: Keychron Link
Receiver VID/PID: 3434:D030
Receiver usage: FF60:61
Firmware: v1.2.1
Mouse: Keychron M5
Mouse VID/PID (wired): 3434:D048
2.4 GHz receiver: Keychron Ultra-Link 8K
Receiver VID/PID: 3434:D028 (same ID for every paired mouse)
Vendor HID interface: 4
```

The K2 HE battery is queried using the Raw HID command `0xAC` provided by the [`vial-updated-keychron`](https://github.com/tymon3310/vial-qmk/tree/vial-updated-keychron) firmware branch. Communication with the Keychron Link receiver uses the wireless transport implemented by that firmware branch.

With the tested setup, the receiver returned `84 43 ...` for a battery query. The first byte is the wireless-transport encoded command response and `0x43` represents **67%** battery.

## Keychron M5 mouse

Keychron mice do not expose their battery through the standard HID battery usage; the M5 uses a vendor-specific protocol on **USB interface 4** of the Ultra-Link 8K receiver (or of the mouse itself when connected by USB cable). This is the same protocol used by the [`keychron-battery-dkms`](https://github.com/csutcliff/keychron-battery-dkms) kernel driver and by Keychron's web launcher — implemented here in userspace via hidapi, no kernel module needed.

```text
Battery query:  output report 0xB3 0x06 (64 bytes, SET_REPORT)
Battery reply:  input report 0xB4; byte 20 = bit 7 charging, bits 6..0 percentage
Model query:    output report 0xB5 0x03 (21 bytes)
Model reply:    up to 3 slots of 5 bytes: VID (le16), PID (le16), byte 4 = 1 when connected
```

The receiver reports the same VID/PID for every mouse, so the tray asks it which mouse is paired and labels the entry accordingly (Keychron M5, Keychron M6 8K, or `Keychron (xxxx:yyyy)`). When the mouse is connected by cable, the same protocol is served directly by the mouse (USB ID `3434:D048`); a wired connection takes precedence in the kernel driver because it usually means the mouse is charging — this tray shows whichever device answers.

**A sleeping mouse does not answer.** The M5 sleeps aggressively when idle, so a poll can fail with a timeout error until the mouse wakes up (move it briefly). This is expected behavior; the next poll recovers. The connection is only re-established on real I/O errors.

## HeadsetControl

The headset battery level is queried with the external [`HeadsetControl`](https://github.com/Sapd/HeadsetControl) tool:

```bash
headsetcontrol -b
```

Install HeadsetControl first (package `headsetcontrol`, a release tarball, or built from source) and verify that the command above reports the G PRO X 2. Its udev rules are required so the tray user can access the device; HeadsetControl ships them (`make install` or `headsetcontrol -u`).

The tray application invokes `headsetcontrol -b -ojson` and parses the JSON battery status (`BATTERY_AVAILABLE`, `BATTERY_CHARGING`, `BATTERY_UNAVAILABLE`, ...). Output from both HeadsetControl 3.x and 4.x is supported.

Example configuration:

```toml
[headsetcontrol]
binary_path = "headsetcontrol"
# Optional: only use the device whose name contains this text.
# device_match = "G PRO X 2"
timeout_seconds = 15
```

If `headsetcontrol` is not in the `PATH` of the systemd user session, set `binary_path` to an absolute path (for example `/usr/local/bin/headsetcontrol`).

## Installation

```bash
mkdir -p ~/.config/dual-battery-tray
cp config.toml.example ~/.config/dual-battery-tray/config.toml
cargo build --release
mkdir -p ~/.local/bin
cp target/release/dual-battery-tray ~/.local/bin/
```

On Debian/Ubuntu, the following development packages may be required depending on your system setup:

```bash
sudo apt install libdbus-1-dev libudev-dev
```

## Test and probe

Run the hardware probe:

```bash
~/.local/bin/dual-battery-tray --probe
```

Then start the tray application normally:

```bash
~/.local/bin/dual-battery-tray
```

To test the low-battery notification, temporarily configure a threshold above one of the current battery levels, for example:

```toml
low_battery_threshold = 70
```

After testing, set it back to `20`.

## Autostart with systemd

```bash
mkdir -p ~/.config/systemd/user
cp systemd/dual-battery-tray.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now dual-battery-tray.service
```

## udev

```bash
sudo cp udev/70-dual-battery-tray-keychron.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

`hidrawN` device numbers are dynamic. The application tries the configured path first and then searches automatically by receiver VID/PID and HID usage.

HeadsetControl brings its own udev rules for the Logitech receiver; install them as part of the HeadsetControl setup (`headsetcontrol -u` prints them).

## Tray icon

The normal tray icon uses the Freedesktop icon name `battery`. On KDE Plasma with Breeze/Breeze-dark enabled, this resolves to the native Breeze battery icon where supported. An embedded white fallback is included for tray hosts that do not resolve the theme icon.

When **any** monitored device falls strictly below `low_battery_threshold` (20% by default), the StatusNotifierItem enters `NeedsAttention` and uses a red battery icon. Once all devices are back at or above the threshold, the normal white icon is restored.

## License

This project is licensed under the **GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later)**. See [`LICENSE`](LICENSE).

## Features

- Shows headset, keyboard, and mouse battery levels (with charging indicator where available) in the tray menu.
- Force-refresh all devices immediately from the tray menu.
- Mouse battery via the Ultra-Link 8K receiver's vendor HID protocol in pure userspace — no kernel module.
