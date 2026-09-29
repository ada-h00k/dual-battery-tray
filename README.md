# dual-battery-tray

A lightweight Rust/Linux tray application that monitors the battery levels of a **Keychron K2 HE ISO RGB** keyboard and a **Corsair HS80 MAX WIRELESS** headset.

The application runs as a single background process, displays both battery levels in the tray menu, and uses the standard **Breeze/Breeze-dark battery icon** where available.

A native Linux/XDG desktop notification is shown when either device drops **strictly below 20%**. Each device triggers at most one notification per low-battery period; once it reaches 20% or higher again, notifications are re-armed for the next drop.

## Resource usage

The application polls both devices only once every 10 minutes by default. The OpenLinkHub connection reuses a single HTTP client. The Keychron Link HID channel is kept open in one dedicated thread and is reopened only after an I/O error. The tray is updated only when the battery value or status actually changes. Tokio runs as a single-threaded runtime.

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
Keyboard: Keychron K2 HE ISO RGB
Keyboard VID/PID: 3434:0E21
2.4 GHz receiver: Keychron Link
Receiver VID/PID: 3434:D030
Receiver usage: FF60:61
Firmware: v1.2.1
```

The K2 HE battery is queried using the Raw HID command `0xAC` provided by the [`vial-updated-keychron`](https://github.com/tymon3310/vial-qmk/tree/vial-updated-keychron) firmware branch. Communication with the Keychron Link receiver uses the wireless transport implemented by that firmware branch.

With the tested setup, the receiver returned `84 43 ...` for a battery query. The first byte is the wireless-transport encoded command response and `0x43` represents **67%** battery.

## OpenLinkHub

The headset battery level is read from OpenLinkHub's `/api/systray` endpoint.

Example configuration:

```toml
[openlinkhub]
base_url = "http://127.0.0.1:27003"
device_match = "HS80"
```

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

## Tray icon

The normal tray icon uses the Freedesktop icon name `battery`. On KDE Plasma with Breeze/Breeze-dark enabled, this resolves to the native Breeze battery icon where supported. An embedded white fallback is included for tray hosts that do not resolve the theme icon.

When **either** monitored device falls strictly below `low_battery_threshold` (20% by default), the StatusNotifierItem enters `NeedsAttention` and uses a red battery icon. Once both devices are back at or above the threshold, the normal white icon is restored.

## License

This project is licensed under the **GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later)**. See [`LICENSE`](LICENSE).

## Features

- Force-refresh both devices immediately from the tray menu.
