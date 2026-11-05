use crate::config::HeadsetControlConfig;
use crate::state::DeviceState;
use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio::process::Command;
use tokio::time::timeout;

pub struct HeadsetControlReader {
    config: HeadsetControlConfig,
}

impl HeadsetControlReader {
    pub fn new(config: HeadsetControlConfig) -> Result<Self> {
        if config.binary_path.trim().is_empty() {
            return Err(anyhow!("headsetcontrol.binary_path must not be empty"));
        }
        Ok(Self { config })
    }

    /// Queries the headset battery by invoking `headsetcontrol -b -ojson`.
    ///
    /// HeadsetControl is a one-shot CLI: every read spawns a short-lived
    /// process and parses its JSON document. There is no daemon and no
    /// persistent connection to keep open between polls.
    pub async fn read(&self) -> Result<DeviceState> {
        let mut command = Command::new(&self.config.binary_path);
        // kill_on_drop ensures the aborted timeout future reaps a hung process.
        command.args(["-b", "-ojson"]).kill_on_drop(true);

        let output = timeout(self.config.timeout(), command.output())
            .await
            .map_err(|_| {
                anyhow!(
                    "headsetcontrol timed out after {}s (binary '{}')",
                    self.config.timeout_seconds,
                    self.config.binary_path
                )
            })?
            .map_err(|e| anyhow!("failed to execute '{}': {e}", self.config.binary_path))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

        if !output.status.success() && serde_json::from_str::<Value>(&stdout).is_err() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "headsetcontrol exited with {}: {}",
                output.status.code().unwrap_or(-1),
                first_line(stderr.trim()).unwrap_or_else(|| "no output".into())
            ));
        }

        let value: Value = serde_json::from_str(&stdout).map_err(|e| {
            anyhow!(
                "headsetcontrol produced invalid JSON ({e}): {}",
                snippet(&stdout)
            )
        })?;

        // HeadsetControl 4.x exits non-zero when no headset was found but still
        // prints a valid JSON document, so the parsed document is preferred
        // over the exit status.
        let device =
            find_battery_device(&value, self.config.device_match.as_deref()).ok_or_else(|| {
                let stderr = String::from_utf8_lossy(&output.stderr);
                match first_line(stderr.trim()) {
                    Some(line) => {
                        anyhow!("no battery device found in headsetcontrol output ({line})")
                    }
                    None => anyhow!("no battery device found in headsetcontrol output"),
                }
            })?;

        parse_battery_device(device)
    }
}

/// Returns the first device that reports a battery and, when `device_match`
/// is set, matches the filter case-insensitively against its known names.
fn find_battery_device<'a>(value: &'a Value, device_match: Option<&str>) -> Option<&'a Value> {
    let devices = value.get("devices")?.as_array()?;
    let needle = device_match.map(str::to_lowercase);
    devices.iter().find(|device| {
        let has_battery = device
            .get("battery")
            .and_then(|battery| battery.get("status"))
            .and_then(Value::as_str)
            .is_some();
        has_battery
            && match &needle {
                None => true,
                Some(needle) => device_names(device)
                    .iter()
                    .any(|name| name.to_lowercase().contains(needle)),
            }
    })
}

/// Device names HeadsetControl reports per device. `device` is the name
/// registered in HeadsetControl, `product` is the HID product string.
fn device_names(device: &Value) -> Vec<&str> {
    ["device", "product", "name"]
        .into_iter()
        .filter_map(|key| device.get(key).and_then(Value::as_str))
        .collect()
}

fn parse_battery_device(device: &Value) -> Result<DeviceState> {
    let name = device_names(device)
        .first()
        .copied()
        .unwrap_or("Headset")
        .to_string();

    let battery = device
        .get("battery")
        .ok_or_else(|| anyhow!("device '{name}' reports no battery object"))?;
    let status = battery
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("battery object of '{name}' has no status"))?;
    // Newer HeadsetControl versions report `level`; very old ones used `charge`.
    let level = battery
        .get("level")
        .or_else(|| battery.get("charge"))
        .and_then(number_i64);

    let (percent, charging) = match status {
        // Exact percentage while running on battery.
        "BATTERY_AVAILABLE" => (validated_percent(&name, level)?, Some(false)),
        // While charging, not every device reports a plausible percentage;
        // the charging state is still worth showing.
        "BATTERY_CHARGING" => (plausible_percent(level), Some(true)),
        other => {
            return Err(anyhow!(
                "headset battery status is {other}: {}",
                battery_error_message(device)
            ))
        }
    };

    Ok(DeviceState {
        name,
        percent,
        charging,
        error: None,
    })
}

/// Extracts a diagnostic message from HeadsetControl's error output.
/// 4.x serializes `errors` as an object (e.g. `{"battery": "message"}`);
/// tolerate array-shaped output defensively.
fn battery_error_message(device: &Value) -> String {
    let message = match device.get("errors") {
        Some(Value::Object(map)) => map.values().find_map(Value::as_str),
        Some(Value::Array(items)) => items.iter().find_map(|item| {
            ["error_message", "message", "error"]
                .iter()
                .find_map(|key| item.get(*key).and_then(Value::as_str))
        }),
        _ => None,
    };
    message.unwrap_or_default().trim().to_string()
}

fn validated_percent(device: &str, level: Option<i64>) -> Result<Option<u8>> {
    match level {
        Some(value) if (0..=100).contains(&value) => Ok(Some(value as u8)),
        _ => Err(anyhow!(
            "headset '{device}' reported no valid battery percentage (level {level:?})"
        )),
    }
}

fn plausible_percent(level: Option<i64>) -> Option<u8> {
    level
        .filter(|value| (0..=100).contains(value))
        .map(|v| v as u8)
}

fn number_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|n| n.round() as i64))
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn snippet(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= 120 {
        return trimmed.to_string();
    }
    let mut cut = 120;
    while !trimmed.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}...", &trimmed[..cut])
}

#[cfg(test)]
mod tests {
    use super::{find_battery_device, parse_battery_device};
    use serde_json::json;

    fn g_pro_x2_sample() -> serde_json::Value {
        json!({
            "name": "HeadsetControl",
            "version": "4.2.0",
            "api_version": "1.9",
            "hidapi_version": "0.14.0",
            "device_count": 1,
            "devices": [
                {
                    "status": "success",
                    "device": "Logitech G PRO X 2 LIGHTSPEED",
                    "vendor": "Logitech",
                    "product": "G PRO X 2 LIGHTSPEED",
                    "id_vendor": "0x046d",
                    "id_product": "0x0af7",
                    "capabilities": ["CAP_BATTERY_STATUS", "CAP_SIDETONE"],
                    "capabilities_str": ["battery", "sidetone"],
                    "battery": {
                        "status": "BATTERY_AVAILABLE",
                        "level": 86
                    }
                }
            ]
        })
    }

    #[test]
    fn parses_g_pro_x2_battery_sample() {
        let value = g_pro_x2_sample();
        let device = find_battery_device(&value, None).unwrap();
        let state = parse_battery_device(device).unwrap();
        assert_eq!(state.name, "Logitech G PRO X 2 LIGHTSPEED");
        assert_eq!(state.percent, Some(86));
        assert_eq!(state.charging, Some(false));
        assert_eq!(state.error, None);
    }

    #[test]
    fn parses_charging_status() {
        let value = json!({
            "devices": [
                {
                    "device": "Logitech G PRO X 2 LIGHTSPEED",
                    "battery": { "status": "BATTERY_CHARGING", "level": 42 }
                }
            ]
        });
        let device = find_battery_device(&value, None).unwrap();
        let state = parse_battery_device(device).unwrap();
        assert_eq!(state.percent, Some(42));
        assert_eq!(state.charging, Some(true));
    }

    #[test]
    fn charging_without_plausible_level_shows_no_percent() {
        let value = json!({
            "devices": [
                {
                    "device": "Logitech G PRO X 2 LIGHTSPEED",
                    "battery": { "status": "BATTERY_CHARGING", "level": -1 }
                }
            ]
        });
        let device = find_battery_device(&value, None).unwrap();
        let state = parse_battery_device(device).unwrap();
        assert_eq!(state.percent, None);
        assert_eq!(state.charging, Some(true));
    }

    #[test]
    fn uses_charge_fallback_for_legacy_output() {
        let value = json!({
            "devices": [
                {
                    "device": "Logitech G PRO X 2 LIGHTSPEED",
                    "battery": { "status": "BATTERY_AVAILABLE", "charge": 57 }
                }
            ]
        });
        let device = find_battery_device(&value, None).unwrap();
        let state = parse_battery_device(device).unwrap();
        assert_eq!(state.percent, Some(57));
    }

    #[test]
    fn matches_device_filter_case_insensitive_on_product_name() {
        let value = json!({
            "devices": [
                {
                    "device": "Arctis 7",
                    "battery": { "status": "BATTERY_AVAILABLE", "level": 10 }
                },
                {
                    "device": "Logitech G PRO X 2 LIGHTSPEED",
                    "product": "G PRO X 2 LIGHTSPEED",
                    "battery": { "status": "BATTERY_AVAILABLE", "level": 86 }
                }
            ]
        });
        let device = find_battery_device(&value, Some("g pro x 2")).unwrap();
        let state = parse_battery_device(device).unwrap();
        assert_eq!(state.percent, Some(86));
    }

    #[test]
    fn filter_without_hit_finds_nothing() {
        let value = g_pro_x2_sample();
        assert!(find_battery_device(&value, Some("WH-1000XM5")).is_none());
    }

    #[test]
    fn no_devices_yields_no_device() {
        let value = json!({ "device_count": 0, "devices": [] });
        assert!(find_battery_device(&value, None).is_none());
    }

    #[test]
    fn missing_devices_key_yields_no_device() {
        let value = json!({ "device_count": 0 });
        assert!(find_battery_device(&value, None).is_none());
    }

    #[test]
    fn rejects_invalid_percentage() {
        let value = json!({
            "devices": [
                {
                    "device": "Logitech G PRO X 2 LIGHTSPEED",
                    "battery": { "status": "BATTERY_AVAILABLE", "level": 250 }
                }
            ]
        });
        let device = find_battery_device(&value, None).unwrap();
        let error = parse_battery_device(device).unwrap_err().to_string();
        assert!(error.contains("no valid battery percentage"), "{error}");
    }

    #[test]
    fn reports_unavailable_status_with_error_message() {
        let value = json!({
            "devices": [
                {
                    "device": "Logitech G PRO X 2 LIGHTSPEED",
                    "status": "partial",
                    "battery": { "status": "BATTERY_UNAVAILABLE", "level": -1 },
                    "errors": { "battery": "Failed to open the HID device" }
                }
            ]
        });
        let device = find_battery_device(&value, None).unwrap();
        let error = parse_battery_device(device).unwrap_err().to_string();
        assert!(error.contains("BATTERY_UNAVAILABLE"), "{error}");
        assert!(error.contains("Failed to open the HID device"), "{error}");
    }

    #[test]
    fn device_without_battery_object_yields_no_device() {
        let value = json!({
            "devices": [
                { "device": "Some keyboard", "status": "success" }
            ]
        });
        assert!(find_battery_device(&value, None).is_none());
    }
}
