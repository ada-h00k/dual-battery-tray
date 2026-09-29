use crate::config::KeyboardConfig;
use crate::state::DeviceState;
use anyhow::{anyhow, Context, Result};
use hidapi::{HidApi, HidDevice};
use std::ffi::CString;
use std::thread;
use std::time::Duration;

const RAW_REPORT_LEN: usize = 32;
const KEYCHRON_GET_PROTOCOL: u8 = 0xA0;
const KEYCHRON_GET_FIRMWARE: u8 = 0xA1;
const KEYCHRON_GET_FEATURES: u8 = 0xA2;
const KEYCHRON_GET_DEFAULT_LAYER: u8 = 0xA3;
const KEYCHRON_GET_BATTERY: u8 = 0xAC;
const WIRELESS_RAW_HID_XOR_KEY: u8 = 0x28;

pub struct KeychronReader {
    config: KeyboardConfig,
}

impl KeychronReader {
    pub fn new(config: KeyboardConfig) -> Result<Self> {
        Ok(Self { config })
    }

    /// Low-overhead persistent worker: keeps the HID device open between polls
    /// and only re-enumerates after an I/O failure.
    pub fn spawn_worker(
        self,
        refresh: Duration,
        tx: tokio::sync::mpsc::UnboundedSender<DeviceState>,
        force_refresh_rx: std::sync::mpsc::Receiver<()>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            if !self.config.enabled {
                let _ = tx.send(DeviceState {
                    name: "Keychron K2 HE".into(),
                    ..Default::default()
                });
                return;
            }

            let mut connection: Option<(HidApi, HidDevice, bool)> = None;

            loop {
                if connection.is_none() {
                    match HidApi::new()
                        .context("failed to initialize hidapi")
                        .and_then(|new_api| {
                            self.open_device(&new_api)
                                .map(|(device, wireless)| (new_api, device, wireless))
                        }) {
                        Ok(opened) => connection = Some(opened),
                        Err(error) => {
                            let _ = tx.send(DeviceState {
                                name: "Keychron K2 HE".into(),
                                error: Some(error.to_string()),
                                ..Default::default()
                            });
                            let wait = refresh.min(Duration::from_secs(30));
                            match force_refresh_rx.recv_timeout(wait) {
                                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                    continue
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                            }
                        }
                    }
                }

                let read_result = connection.as_ref().map(|(_, device, wireless_tunnel)| {
                    self.read_from_device(device, *wireless_tunnel)
                });

                match read_result {
                    Some(Ok(state)) => {
                        let _ = tx.send(state);
                    }
                    Some(Err(error)) => {
                        let _ = tx.send(DeviceState {
                            name: "Keychron K2 HE".into(),
                            error: Some(error.to_string()),
                            ..Default::default()
                        });
                        // Drop the device and HidApi together so reconnects can
                        // rediscover a changed hidraw path.
                        connection = None;
                    }
                    None => {}
                }

                match force_refresh_rx.recv_timeout(refresh) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        })
    }

    fn read_from_device(&self, device: &HidDevice, wireless_tunnel: bool) -> Result<DeviceState> {
        let mut state = DeviceState {
            name: "Keychron K2 HE".into(),
            ..Default::default()
        };

        let protocol = self.config.protocol.to_ascii_lowercase();
        if !matches!(protocol.as_str(), "auto" | "ac" | "keychron-ac" | "") {
            return Err(anyhow!(
                "unsupported keyboard protocol '{protocol}'; this K2 HE firmware uses Keychron 0xAC"
            ));
        }

        let response = self.query_battery(device, wireless_tunnel)?;
        state.percent = Some(response.percent);
        state.charging = None;
        Ok(state)
    }

    fn open_device(&self, api: &HidApi) -> Result<(HidDevice, bool)> {
        if let Some(path) = &self.config.hidraw_path {
            let c_path = CString::new(path.as_bytes())
                .context("invalid hidraw path: contains a NUL byte")?;
            if let Ok(dev) = api.open_path(c_path.as_c_str()) {
                return Ok((dev, true));
            }
        }

        for info in api.device_list() {
            if info.vendor_id() != self.config.receiver_vid {
                continue;
            }
            if !self.config.receiver_pids.is_empty()
                && !self.config.receiver_pids.contains(&info.product_id())
            {
                continue;
            }
            if info.usage_page() != self.config.usage_page || info.usage() != self.config.usage {
                continue;
            }
            if let Ok(dev) = info.open_device(api) {
                return Ok((dev, true));
            }
        }

        for info in api.device_list() {
            if info.vendor_id() == self.config.keyboard_vid
                && info.product_id() == self.config.keyboard_pid
                && info.usage_page() == self.config.usage_page
                && info.usage() == self.config.usage
            {
                return info
                    .open_device(api)
                    .map(|dev| (dev, false))
                    .context("could not open Keychron keyboard raw HID interface");
            }
        }

        Err(anyhow!("no Keychron raw-HID interface found"))
    }

    fn query_battery(&self, device: &HidDevice, wireless_tunnel: bool) -> Result<BatteryResponse> {
        let mut last_error = None;
        for attempt in 0..3 {
            match self.query_command(device, KEYCHRON_GET_BATTERY, wireless_tunnel) {
                Ok(bytes) => match Self::parse_battery_response(&bytes) {
                    Ok(value) => return Ok(value),
                    Err(e) => last_error = Some(e),
                },
                Err(e) => last_error = Some(e),
            }
            if attempt < 2 {
                thread::sleep(Duration::from_millis(75));
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("Keychron battery query failed")))
    }

    fn parse_battery_response(bytes: &[u8]) -> Result<BatteryResponse> {
        // hidapi may return an explicit report-id byte for some interfaces.
        // A zero report ID is not part of the Keychron payload, so strip it
        // when it is followed by a plausible Keychron battery command.
        let encoded_command = KEYCHRON_GET_BATTERY ^ WIRELESS_RAW_HID_XOR_KEY;
        let payload = if bytes.len() >= 3
            && bytes[0] == 0
            && (bytes[1] == KEYCHRON_GET_BATTERY || bytes[1] == encoded_command)
        {
            &bytes[1..]
        } else {
            bytes
        };

        if payload.len() < 2 {
            return Err(anyhow!(
                "Keychron 0xAC response is truncated (got {})",
                hex(bytes)
            ));
        }

        let command = payload[0];
        let percent = if command == KEYCHRON_GET_BATTERY || command == encoded_command {
            payload[1]
        } else {
            return Err(anyhow!(
                "no Keychron 0xAC battery response (got {})",
                hex(bytes)
            ));
        };

        if percent > 100 {
            let decoded = percent ^ WIRELESS_RAW_HID_XOR_KEY;
            if decoded <= 100 {
                return Ok(BatteryResponse { percent: decoded });
            }
            return Err(anyhow!(
                "invalid battery percentage {percent} in Keychron 0xAC response (got {})",
                hex(bytes)
            ));
        }

        Ok(BatteryResponse { percent })
    }

    fn query_command(
        &self,
        device: &HidDevice,
        command: u8,
        wireless_tunnel: bool,
    ) -> Result<Vec<u8>> {
        let mut request = vec![0u8; RAW_REPORT_LEN + 1];
        request[1] = command;
        if wireless_tunnel {
            for byte in &mut request[1..] {
                *byte ^= WIRELESS_RAW_HID_XOR_KEY;
            }
        }

        device
            .write(&request)
            .with_context(|| format!("failed to send Keychron 0x{command:02X} request"))?;

        let mut response = vec![0u8; RAW_REPORT_LEN + 1];
        let len = device
            .read_timeout(&mut response, self.config.timeout_ms)
            .with_context(|| format!("failed to read Keychron 0x{command:02X} response"))?;

        if len == 0 {
            return Err(anyhow!("timeout/no response"));
        }

        Ok(response[..len].to_vec())
    }

    fn query_known_commands(&self, device: &HidDevice, wireless_tunnel: bool) -> Vec<String> {
        let mut lines = Vec::new();
        for command in [
            KEYCHRON_GET_PROTOCOL,
            KEYCHRON_GET_FIRMWARE,
            KEYCHRON_GET_FEATURES,
            KEYCHRON_GET_DEFAULT_LAYER,
            KEYCHRON_GET_BATTERY,
        ] {
            match self.query_command(device, command, wireless_tunnel) {
                Ok(bytes) => lines.push(format!("0x{command:02X}: {}", hex(&bytes))),
                Err(e) => lines.push(format!("0x{command:02X}: error: {e}")),
            }
        }
        lines
    }

    pub fn probe(&self) -> Result<Vec<String>> {
        let api = HidApi::new()?;
        let mut lines = Vec::new();
        for info in api.device_list() {
            if info.vendor_id() == self.config.receiver_vid
                || info.vendor_id() == self.config.keyboard_vid
            {
                lines.push(format!(
                    "{:04X}:{:04X} path={} usage={:04X}:{:02X} product={:?} manufacturer={:?}",
                    info.vendor_id(),
                    info.product_id(),
                    info.path().to_string_lossy(),
                    info.usage_page(),
                    info.usage(),
                    info.product_string(),
                    info.manufacturer_string()
                ));
            }
        }

        if let Ok((device, wireless_tunnel)) = self.open_device(&api) {
            lines.push(format!(
                "-- Keychron raw HID diagnostics (wireless_xor={}) --",
                wireless_tunnel
            ));
            lines.extend(self.query_known_commands(&device, wireless_tunnel));

            match self.query_battery(&device, wireless_tunnel) {
                Ok(value) => lines.push(format!(
                    "battery={}% (Keychron 0xAC, wireless_xor={})",
                    value.percent, wireless_tunnel
                )),
                Err(e) => lines.push(format!("battery probe error: {e}")),
            }
        } else {
            lines.push("-- Keychron raw HID diagnostics: device could not be opened --".into());
        }

        Ok(lines)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::KeychronReader;

    #[test]
    fn parses_observed_keychron_link_battery_response() {
        let response = [0x84, 0x43, 0x00, 0x00];
        let value = KeychronReader::parse_battery_response(&response).unwrap();
        assert_eq!(value.percent, 67);
    }

    #[test]
    fn parses_zero_report_id_variant() {
        let response = [0x00, 0x84, 0x43, 0x00];
        let value = KeychronReader::parse_battery_response(&response).unwrap();
        assert_eq!(value.percent, 67);
    }
}

struct BatteryResponse {
    percent: u8,
}
