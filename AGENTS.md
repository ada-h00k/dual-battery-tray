# AGENTS.md

## Project overview

`dual-battery-tray` is a small Linux tray application written in Rust. It monitors:

- a Keychron K2 HE ISO RGB keyboard via raw HID / Keychron Link
- a Corsair HS80 MAX WIRELESS headset via OpenLinkHub's `/api/systray` endpoint

The process publishes both battery levels through a StatusNotifierItem/tray UI, supports a manual refresh action, and emits Linux/XDG desktop notifications for low battery.

The project is deliberately lightweight: polling defaults to every 10 minutes, Tokio uses a single-threaded runtime, the OpenLinkHub HTTP client is reused, and the Keychron HID connection is kept open until an I/O error.

## Scope and working assumptions

These instructions apply to the whole repository unless a deeper `AGENTS.md` is added later.

Treat the current source and configuration files as the source of truth. Do not invent device behavior or protocol details that are not supported by the existing implementation or documented firmware behavior.

## Build and development commands

Use the stable Rust toolchain compatible with the repository's declared MSRV (`rust-version = "1.80"`). The crate uses Rust 2021 edition.

Before submitting a change, normally run:

```bash
cargo fmt --all -- --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

For a release-build sanity check:

```bash
cargo build --release
```

When changing only documentation or packaging files, Rust checks may still be useful, but do not make unnecessary source changes just to satisfy them.

Do not assume hardware is available in CI or on a development machine. Prefer pure/unit-testable logic for parsing, configuration, state transitions, and formatting. Use the real hardware probe only when the environment has the required devices and permissions.

## Repository-specific runtime commands

To inspect detected Keychron devices and issue protocol diagnostics:

```bash
cargo run -- --probe
```

After installing the release binary, the documented equivalent is:

```bash
~/.local/bin/dual-battery-tray --probe
```

Run the tray normally with:

```bash
cargo run -- --config ~/.config/dual-battery-tray/config.toml
```

When testing low-battery notifications, temporarily raise `low_battery_threshold` above a device's current battery percentage and restore it afterward.

## Architecture and module responsibilities

Keep responsibilities separated:

- `src/config.rs`: TOML deserialization, defaults, and bounded configuration accessors.
- `src/state.rs`: small, serializable device-state/snapshot data structures shared by readers and the tray.
- `src/keychron.rs`: raw HID device discovery, persistent HID worker, Keychron command transport, response parsing, and probe diagnostics.
- `src/openlinkhub.rs`: OpenLinkHub HTTP access and recursive extraction/filtering of battery devices from `/api/systray` JSON.
- `src/notifications.rs`: low-battery notification state machine and desktop notifications.
- `src/icon.rs`: bundled tray icon pixmap fallbacks.
- `src/main.rs`: tray/status-menu behavior and user-facing presentation/integration.

When adding a feature, place the logic in the narrowest appropriate module instead of growing the tray layer into another hardware/protocol implementation.

## Keychron protocol invariants

The Keychron K2 HE battery query uses raw-HID command `0xAC`.

For Keychron Link wireless transport, the raw-HID payload is XOR-encoded with `0x28`. Consequently the battery command appears as `0x84` on the wireless side, and the battery percentage byte must be decoded with the same key.

The parser must continue to:

- tolerate an explicit leading HID report-id byte `0x00` when followed by a plausible battery command
- reject truncated responses
- reject responses that do not represent the expected `0xAC` command (direct or encoded wireless form)
- reject battery percentages greater than `100`

Do not silently relax validation around malformed HID responses. Prefer explicit errors with enough context for `--probe`/logs to diagnose the device.

The worker is intentionally persistent: keep the HID handle open between polls and only drop/re-enumerate it after an I/O failure. Preserve this behavior unless there is a concrete correctness reason to change it.

Device discovery should keep the configured `hidraw_path` as a first attempt, then fall back to VID/PID/usage based discovery because `hidrawN` numbers are dynamic.

## OpenLinkHub invariants

The headset reader calls:

```text
GET {base_url}/api/systray
```

The HTTP client is created once and reused.

Device matching is optional. When `device_match` is configured, match case-insensitively against the device name. Parsed battery values must remain in the valid `0..=100` range.

Do not assume a single fixed JSON shape unnecessarily: the current reader recursively walks arrays/objects and accepts common key spellings for device name, level, and charging state.

## Configuration rules

Current defaults include:

```toml
refresh_seconds = 600
low_battery_threshold = 20

[openlinkhub]
base_url = "http://127.0.0.1:27003"

[keyboard]
enabled = true
keyboard_vid = 0x3434
keyboard_pid = 0x0E21
receiver_vid = 0x3434
receiver_pids = [0xD026, 0xD027, 0xD030, 0xD031]
usage_page = 0xFF60
usage = 0x61
hidraw_path = "/dev/hidraw5"
protocol = "auto"
timeout_ms = 800
```

Important semantics:

- refresh intervals below 60 seconds are clamped/rejected at the accessor boundary; do not bypass this safety limit
- low-battery thresholds above `100` are clamped to `100`
- `keyboard.enabled = false` must leave the tray application usable without HID hardware
- the configured `hidraw_path` is only a hint; paths can change after reconnects

Keep configuration backward compatible when practical. If a setting is renamed or its semantics change, update `config.toml.example` and the README in the same change.

## Low-battery behavior

The threshold comparison is strictly `< threshold`.

At the default threshold, exactly `20%` is **not** considered low. Each device should notify at most once during one low-battery episode; notification state is re-armed only after that device returns to the threshold or above.

This state is per device. Do not accidentally turn it into a single global notification latch.

The tray icon follows the same low-battery condition. Keep the normal icon path/theme behavior and the explicit red fallback behavior consistent with the notification threshold.

## Tray/UI behavior

The tray item intentionally stays `Active`. Do not switch it to `NeedsAttention` just to signal low battery; the current implementation avoids Plasma animation/highlighting side effects.

Normal icon behavior:

- request the Freedesktop icon name `battery`
- prefer the installed Breeze/Breeze-dark theme where available
- use bundled white pixmap assets as fallback

Low-battery behavior:

- use the bundled red icon fallback
- keep the tray item otherwise active

Preserve the existing menu semantics: show headset and keyboard battery values, show status/error text, offer `Force refresh`, and offer `Quit`.

## Linux integration

The application is Linux/XDG-specific. Changes involving HID access, systemd user services, udev, DBus, StatusNotifierItem, or desktop notifications must remain compatible with normal Linux desktop environments.

The provided udev rule uses `TAG+="uaccess"` for the supported Keychron keyboard/receiver VID/PID combinations. Do not broaden device permissions more than necessary.

The provided user service is intended to run after `graphical-session.target` and restarts on failure. Keep the service user-scoped and avoid requiring root for the tray process itself.

## Error handling and logging

Use `anyhow` for contextual operational errors and `tracing` for diagnostics/logging. User-visible device failures should become device state errors rather than crashing the whole tray process.

Keep error messages actionable and include relevant context such as the operation, command, path, or HTTP endpoint. Avoid logging raw secrets or unrelated sensitive data.

A failed headset read or keyboard read should not prevent the other device from being shown.

## Testing guidance

Prioritize deterministic tests for logic that can be tested without hardware, especially:

- configuration defaults and bounds
- Keychron response parsing, including direct, wireless/XOR, report-id, truncated, and invalid-percentage cases
- OpenLinkHub JSON extraction and optional device matching
- low-battery notification re-arming behavior
- tray formatting helpers and low-battery threshold semantics

Hardware-specific tests should be isolated so ordinary `cargo test` does not require a connected Keychron/Corsair setup.

When modifying HID protocol code, add or update parser tests before relying on manual hardware verification.

## Performance and resource usage

Do not introduce a tighter default polling loop, busy-waiting, repeated HTTP client creation, or repeated HID enumeration on every successful poll.

Preserve the current low-overhead design:

- 10-minute default polling interval
- persistent Keychron HID connection
- re-enumeration only after I/O failure
- one reused OpenLinkHub HTTP client
- single-threaded Tokio runtime
- tray updates only when state/status actually changes

A performance change should explain why the additional work is necessary and should avoid turning the tray app into a continuously active poller.

## Dependency and API changes

Keep dependencies minimal. Before adding a crate, check whether the standard library or an existing dependency already solves the problem.

When changing dependency versions, preserve the repository's Linux feature selection for `hidapi` and avoid enabling unnecessary default features.

Avoid broad public APIs for internal helpers. Prefer private functions/types unless external consumers actually need the symbol.

## Documentation and packaging

Keep these files synchronized with source behavior when relevant:

- `README.md`
- `config.toml.example`
- `systemd/dual-battery-tray.service`
- `udev/70-dual-battery-tray-keychron.rules`

The project is licensed under `AGPL-3.0-or-later`; do not remove or weaken existing license notices.

## Change checklist

Before considering a source change complete:

1. Format the code.
2. Run `cargo check` and relevant tests.
3. Run Clippy with warnings denied where the environment supports it.
4. Update configuration/docs/service/udev files when behavior changes.
5. Preserve the documented hardware and low-resource invariants.
6. For protocol changes, verify both success and malformed/error paths.

When unsure about hardware behavior, prefer the existing `--probe` diagnostics and documented firmware protocol over assumptions.
