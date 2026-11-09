use crate::config::MouseConfig;
use crate::state::DeviceState;
use anyhow::{anyhow, Context, Result};
use hidapi::{HidApi, HidDevice};
use std::thread;
use std::time::{Duration, Instant};

// Vendor protocol of the Keychron M5 / M6 8K mice and the Ultra-Link 8K
// receiver, as used by the keychron-battery-dkms kernel driver and by
// Keychron's web launcher:
//
//   battery request : output report 0xB3 0x06, 64 bytes (SET_REPORT)
//   battery response: input report 0xB4, battery byte at offset 20
//                     bit 7 = charging, bits 6..0 = percentage
//   model query     : output report 0xB5 0x03, 21 bytes; the response holds
//                     up to 3 slots of 5 bytes: VID (le16), PID (le16),
//                     byte 4 = 1 when a mouse is connected in that slot
//
// The receiver (3434:D028) exposes this protocol on a fixed USB interface;
// the same interface number applies to the mice connected by cable.
const STATUS_REQUEST_ID: u8 = 0xB3;
const STATUS_RESPONSE_ID: u8 = 0xB4;
const CMD_STATUS: u8 = 0x06;
const INFO_REQUEST_ID: u8 = 0xB5;
const CMD_RECV_STATE: u8 = 0x03;
const STATUS_REPORT_LEN: usize = 64;
const INFO_REPORT_LEN: usize = 21;
const MAX_INPUT_REPORT_LEN: usize = 64;
const BATTERY_OFFSET: usize = 20;
const CHARGING_BIT: u8 = 0x80;
const LEVEL_MASK: u8 = 0x7F;
const SLOT_OFFSET: usize = 3;
const SLOT_SIZE: usize = 5;
const SLOT_COUNT: usize = 3;
const DRAIN_TIMEOUT_MS: i32 = 10;

const FALLBACK_NAME: &str = "Keychron M5";
const RECEIVER_FALLBACK_NAME: &str = "Keychron (Ultra-Link 8K)";

/// Model names for known paired-mouse PIDs (vendor 0x3434).
fn model_name(pid: u16) -> Option<&'static str> {
    match pid {
        0xD048 => Some("Keychron M5"),
        0xD049 => Some("Keychron M6 8K"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct MouseDevice {
    pid: u16,
    is_receiver: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MouseBattery {
    percent: u8,
    charging: bool,
}

/// Why a mouse query failed. The distinction drives the worker: timeouts and
/// protocol violations are transient (the mouse is frequently asleep), while
/// I/O errors require re-opening the HID device.
#[derive(Debug)]
enum QueryError {
    /// The mouse stayed silent for every attempt (typically asleep).
    NoResponse(String),
    /// The device answered, but the payload violates the protocol.
    Invalid(String),
    /// hidapi I/O failed; the connection must be re-established.
    Io(String),
}

impl QueryError {
    fn message(&self) -> &str {
        match self {
            QueryError::NoResponse(message) => message,
            QueryError::Invalid(message) => message,
            QueryError::Io(message) => message,
        }
    }
}

pub struct MouseReader {
    config: MouseConfig,
}

impl MouseReader {
    pub fn new(config: MouseConfig) -> Result<Self> {
        Ok(Self { config })
    }

    /// Low-overhead persistent worker, mirroring the keyboard reader: the HID
    /// device stays open between polls and is only re-enumerated after an I/O
    /// failure. A sleeping mouse times out instead — that is expected and
    /// keeps the connection.
    pub fn spawn_worker(
        self,
        refresh: Duration,
        tx: tokio::sync::mpsc::UnboundedSender<DeviceState>,
        force_refresh_rx: std::sync::mpsc::Receiver<()>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            if !self.config.enabled {
                let _ = tx.send(DeviceState {
                    name: FALLBACK_NAME.into(),
                    ..Default::default()
                });
                return;
            }

            let mut connection: Option<(HidApi, HidDevice, MouseDevice)> = None;
            let mut model: Option<String> = None;

            loop {
                if connection.is_none() {
                    // Re-detect the paired model after every re-open.
                    model = None;
                    match HidApi::new()
                        .context("failed to initialize hidapi")
                        .and_then(|api| {
                            self.open_device(&api)
                                .map(|(device, info)| (api, device, info))
                        }) {
                        Ok(opened) => connection = Some(opened),
                        Err(error) => {
                            let _ = tx.send(DeviceState {
                                name: FALLBACK_NAME.into(),
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

                let read_result = connection
                    .as_ref()
                    .map(|(_, device, info)| self.read_from_device(device, info, &mut model));

                match read_result {
                    Some(Ok(state)) => {
                        let _ = tx.send(state);
                    }
                    Some(Err(error)) => {
                        // Drop the device only after a real I/O failure so
                        // reconnects can rediscover a changed hidraw path.
                        if matches!(error, QueryError::Io(_)) {
                            connection = None;
                        }
                        let _ = tx.send(DeviceState {
                            name: FALLBACK_NAME.into(),
                            error: Some(error.message().to_string()),
                            ..Default::default()
                        });
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

    fn read_from_device(
        &self,
        device: &HidDevice,
        info: &MouseDevice,
        model: &mut Option<String>,
    ) -> Result<DeviceState, QueryError> {
        // The receiver reports the same USB ID for every mouse; ask it which
        // mouse is currently paired. Best effort: a sleeping mouse cannot
        // answer this either, and the battery query below will report that.
        if info.is_receiver && model.is_none() {
            if let Ok(bytes) = self.query(
                device,
                INFO_REQUEST_ID,
                CMD_RECV_STATE,
                None,
                INFO_REPORT_LEN,
            ) {
                if let Some((vid, pid)) = parse_paired_mouse(&bytes) {
                    *model = Some(
                        model_name(pid)
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("Keychron ({vid:04X}:{pid:04X})")),
                    );
                }
            }
        }

        let bytes = self.query(
            device,
            STATUS_REQUEST_ID,
            CMD_STATUS,
            Some(STATUS_RESPONSE_ID),
            STATUS_REPORT_LEN,
        )?;
        let battery =
            parse_battery_response(&bytes).map_err(|e| QueryError::Invalid(e.to_string()))?;

        let name = if info.is_receiver {
            model
                .clone()
                .unwrap_or_else(|| RECEIVER_FALLBACK_NAME.into())
        } else {
            model_name(info.pid)
                .map(str::to_string)
                .unwrap_or_else(|| format!("Keychron ({:04X})", info.pid))
        };

        Ok(DeviceState {
            name,
            percent: Some(battery.percent),
            charging: Some(battery.charging),
            error: None,
        })
    }

    fn open_device(&self, api: &HidApi) -> Result<(HidDevice, MouseDevice)> {
        for info in api.device_list() {
            if info.vendor_id() != self.config.vendor_id {
                continue;
            }
            let is_receiver = info.product_id() == self.config.receiver_pid;
            let is_wired = self.config.wired_pids.contains(&info.product_id());
            if !is_receiver && !is_wired {
                continue;
            }
            if self.config.interface_number >= 0
                && info.interface_number() != self.config.interface_number
            {
                continue;
            }
            if let Ok(device) = info.open_device(api) {
                return Ok((
                    device,
                    MouseDevice {
                        pid: info.product_id(),
                        is_receiver,
                    },
                ));
            }
        }
        let wired = self
            .config
            .wired_pids
            .iter()
            .map(|pid| format!("{pid:04X}"))
            .collect::<Vec<_>>()
            .join(", ");
        Err(anyhow!(
            "no Keychron mouse vendor HID interface found (receiver {:04X}:{:04X}, \
             wired {wired}, interface {})",
            self.config.vendor_id,
            self.config.receiver_pid,
            self.config.interface_number
        ))
    }

    /// Sends one vendor query and waits for the matching input report, with
    /// bounded retries — same structure as the kernel driver's query loop.
    fn query(
        &self,
        device: &HidDevice,
        report_id: u8,
        command: u8,
        response_id: Option<u8>,
        length: usize,
    ) -> Result<Vec<u8>, QueryError> {
        let mut attempts = self.config.retries().max(1);
        let mut last_error = QueryError::NoResponse(format!("no 0x{report_id:02X} response"));
        while attempts > 0 {
            attempts -= 1;
            match self.query_once(device, report_id, command, response_id, length) {
                Ok(bytes) => return Ok(bytes),
                // A silent mouse is retried; protocol/I/O errors surface
                // immediately so the worker can react.
                Err(error @ QueryError::NoResponse(_)) => last_error = error,
                Err(error) => return Err(error),
            }
            if attempts > 0 {
                thread::sleep(self.config.retry_delay());
            }
        }
        Err(last_error)
    }

    fn query_once(
        &self,
        device: &HidDevice,
        report_id: u8,
        command: u8,
        response_id: Option<u8>,
        length: usize,
    ) -> Result<Vec<u8>, QueryError> {
        // Discard unsolicited reports buffered since the last query so a
        // stale frame cannot be mistaken for the fresh response.
        let mut drain = [0u8; MAX_INPUT_REPORT_LEN];
        loop {
            let drained = device
                .read_timeout(&mut drain, DRAIN_TIMEOUT_MS)
                .map_err(|e| QueryError::Io(format!("failed to drain responses: {e}")))?;
            if drained == 0 {
                break;
            }
        }

        let mut request = vec![0u8; length];
        request[0] = report_id;
        request[1] = command;
        device.write(&request).map_err(|e| {
            QueryError::Io(format!("failed to send 0x{report_id:02X} request: {e}"))
        })?;

        let timeout = self.config.timeout();
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(QueryError::NoResponse(format!(
                    "no 0x{report_id:02X} response within {} ms (mouse asleep?)",
                    timeout.as_millis()
                )));
            }
            let mut response = [0u8; MAX_INPUT_REPORT_LEN];
            match device.read_timeout(
                &mut response,
                remaining.as_millis().min(i32::MAX as u128) as i32,
            ) {
                Ok(0) => continue,
                Ok(n) => {
                    let bytes = &response[..n];
                    // Validate like the kernel driver: command echo always,
                    // report id when it is known for this query. Unsolicited
                    // reports are skipped and the wait continues.
                    if n < 2 || bytes[1] != command {
                        continue;
                    }
                    if let Some(id) = response_id {
                        if bytes[0] != id {
                            continue;
                        }
                    }
                    return Ok(bytes.to_vec());
                }
                Err(e) => {
                    return Err(QueryError::Io(format!(
                        "failed to read 0x{report_id:02X} response: {e}"
                    )))
                }
            }
        }
    }

    pub fn probe(&self) -> Result<Vec<String>> {
        let api = HidApi::new()?;
        let mut lines = Vec::new();
        for info in api.device_list() {
            if info.vendor_id() == self.config.vendor_id {
                lines.push(format!(
                    "{:04X}:{:04X} path={} interface={} product={:?} manufacturer={:?}",
                    info.vendor_id(),
                    info.product_id(),
                    info.path().to_string_lossy(),
                    info.interface_number(),
                    info.product_string(),
                    info.manufacturer_string()
                ));
            }
        }

        match self.open_device(&api) {
            Ok((device, info)) => {
                lines.push(format!(
                    "-- Keychron mouse vendor HID diagnostics (pid={:04X}, receiver={}) --",
                    info.pid, info.is_receiver
                ));
                if info.is_receiver {
                    match self.query(
                        &device,
                        INFO_REQUEST_ID,
                        CMD_RECV_STATE,
                        None,
                        INFO_REPORT_LEN,
                    ) {
                        Ok(bytes) => match parse_paired_mouse(&bytes) {
                            Some((vid, pid)) => lines.push(format!(
                                "paired mouse: {vid:04X}:{pid:04X} ({})",
                                model_name(pid).unwrap_or("unknown")
                            )),
                            None => lines
                                .push("paired mouse: no connected slot in 0xB5 response".into()),
                        },
                        Err(e) => lines.push(format!("paired mouse probe error: {}", e.message())),
                    }
                }
                match self.query(
                    &device,
                    STATUS_REQUEST_ID,
                    CMD_STATUS,
                    Some(STATUS_RESPONSE_ID),
                    STATUS_REPORT_LEN,
                ) {
                    Ok(bytes) => match parse_battery_response(&bytes) {
                        Ok(battery) => lines.push(format!(
                            "battery={}%{} (vendor 0xB3/0x06 -> 0xB4 protocol)",
                            battery.percent,
                            if battery.charging { ", charging" } else { "" }
                        )),
                        Err(e) => lines.push(format!("battery probe error: {e}")),
                    },
                    Err(e) => lines.push(format!("battery probe error: {}", e.message())),
                }
            }
            Err(e) => lines.push(format!("-- mouse vendor HID diagnostics: {e} --")),
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

fn parse_battery_response(bytes: &[u8]) -> Result<MouseBattery> {
    if bytes.len() < 2 {
        return Err(anyhow!(
            "battery response is truncated (got {})",
            hex(bytes)
        ));
    }
    if bytes[0] != STATUS_RESPONSE_ID {
        return Err(anyhow!(
            "no battery response with report id 0x{STATUS_RESPONSE_ID:02X} (got {})",
            hex(bytes)
        ));
    }
    if bytes[1] != CMD_STATUS {
        return Err(anyhow!(
            "no status command echo 0x{CMD_STATUS:02X} (got {})",
            hex(bytes)
        ));
    }
    if bytes.len() < BATTERY_OFFSET + 1 {
        return Err(anyhow!(
            "battery response is truncated before the battery byte (got {} bytes)",
            bytes.len()
        ));
    }
    let raw = bytes[BATTERY_OFFSET];
    let percent = raw & LEVEL_MASK;
    if percent > 100 {
        return Err(anyhow!(
            "invalid battery percentage {percent} (got {})",
            hex(bytes)
        ));
    }
    Ok(MouseBattery {
        percent,
        charging: raw & CHARGING_BIT != 0,
    })
}

fn parse_paired_mouse(bytes: &[u8]) -> Option<(u16, u16)> {
    if bytes.len() < 2 || bytes[1] != CMD_RECV_STATE {
        return None;
    }
    for slot in 0..SLOT_COUNT {
        let offset = SLOT_OFFSET + slot * SLOT_SIZE;
        if offset + SLOT_SIZE > bytes.len() {
            break;
        }
        let connected = bytes[offset + 4];
        if connected == 1 {
            let vid = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
            let pid = u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]);
            return Some((vid, pid));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{model_name, parse_battery_response, parse_paired_mouse};

    fn battery_response(raw_battery_byte: u8) -> Vec<u8> {
        let mut bytes = vec![0u8; super::STATUS_REPORT_LEN];
        bytes[0] = super::STATUS_RESPONSE_ID;
        bytes[1] = super::CMD_STATUS;
        bytes[super::BATTERY_OFFSET] = raw_battery_byte;
        bytes
    }

    fn info_response(slots: [Option<(u16, u16, u8)>; 3]) -> Vec<u8> {
        let mut bytes = vec![0u8; super::INFO_REPORT_LEN];
        bytes[0] = 0xB6; // response report id is not validated for 0xB5 queries
        bytes[1] = super::CMD_RECV_STATE;
        for (slot, value) in slots.into_iter().enumerate() {
            if let Some((vid, pid, connected)) = value {
                let offset = super::SLOT_OFFSET + slot * super::SLOT_SIZE;
                bytes[offset..offset + 2].copy_from_slice(&vid.to_le_bytes());
                bytes[offset + 2..offset + 4].copy_from_slice(&pid.to_le_bytes());
                bytes[offset + 4] = connected;
            }
        }
        bytes
    }

    #[test]
    fn parses_battery_response() {
        let value = parse_battery_response(&battery_response(87)).unwrap();
        assert_eq!(value.percent, 87);
        assert!(!value.charging);
    }

    #[test]
    fn parses_charging_battery_response() {
        let value = parse_battery_response(&battery_response(0x80 | 55)).unwrap();
        assert_eq!(value.percent, 55);
        assert!(value.charging);
    }

    #[test]
    fn accepts_battery_boundaries() {
        let empty = parse_battery_response(&battery_response(0)).unwrap();
        assert_eq!(empty.percent, 0);
        let full = parse_battery_response(&battery_response(100)).unwrap();
        assert_eq!(full.percent, 100);
        let full_charging = parse_battery_response(&battery_response(0x80 | 100)).unwrap();
        assert_eq!(full_charging.percent, 100);
        assert!(full_charging.charging);
    }

    #[test]
    fn rejects_invalid_battery_percentage() {
        let error = parse_battery_response(&battery_response(101))
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid battery percentage"), "{error}");
        // Also invalid while the charging bit is set (level bits only).
        let error = parse_battery_response(&battery_response(0x80 | 101))
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid battery percentage"), "{error}");
    }

    #[test]
    fn rejects_truncated_battery_response() {
        assert!(parse_battery_response(&[0xB4, 0x06]).is_err());
        assert!(parse_battery_response(&battery_response(50)[..20]).is_err());
    }

    #[test]
    fn rejects_wrong_report_id_and_command_echo() {
        let mut bytes = battery_response(50);
        bytes[0] = 0xC7;
        let error = parse_battery_response(&bytes).unwrap_err().to_string();
        assert!(error.contains("report id"), "{error}");

        let mut bytes = battery_response(50);
        bytes[1] = 0x07;
        let error = parse_battery_response(&bytes).unwrap_err().to_string();
        assert!(error.contains("command echo"), "{error}");
    }

    #[test]
    fn parses_paired_mouse_in_first_slot() {
        let bytes = info_response([Some((0x3434, 0xD048, 1)), None, None]);
        assert_eq!(parse_paired_mouse(&bytes), Some((0x3434, 0xD048)));
    }

    #[test]
    fn parses_paired_mouse_in_last_slot() {
        let bytes = info_response([
            Some((0x3434, 0xD048, 0)),
            Some((0x3434, 0xD048, 0)),
            Some((0x3434, 0xD049, 1)),
        ]);
        assert_eq!(parse_paired_mouse(&bytes), Some((0x3434, 0xD049)));
    }

    #[test]
    fn paired_mouse_without_connected_slot_is_none() {
        let bytes = info_response([Some((0x3434, 0xD048, 0)), None, None]);
        assert_eq!(parse_paired_mouse(&bytes), None);
        assert_eq!(parse_paired_mouse(&info_response([None, None, None])), None);
    }

    #[test]
    fn rejects_malformed_paired_mouse_responses() {
        // Wrong command echo.
        let mut bytes = info_response([Some((0x3434, 0xD048, 1)), None, None]);
        bytes[1] = 0x04;
        assert_eq!(parse_paired_mouse(&bytes), None);
        // Truncated before the first slot.
        assert_eq!(parse_paired_mouse(&bytes[..7]), None);
    }

    #[test]
    fn maps_known_model_names() {
        assert_eq!(model_name(0xD048), Some("Keychron M5"));
        assert_eq!(model_name(0xD049), Some("Keychron M6 8K"));
        assert_eq!(model_name(0xD028), None);
    }
}
