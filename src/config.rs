use crate::notifications::DEFAULT_LOW_BATTERY_THRESHOLD;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_refresh")]
    pub refresh_seconds: u64,
    #[serde(default = "default_low_battery_threshold")]
    pub low_battery_threshold: u8,
    #[serde(default)]
    pub headsetcontrol: HeadsetControlConfig,
    #[serde(default)]
    pub keyboard: KeyboardConfig,
    #[serde(default)]
    pub mouse: MouseConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeadsetControlConfig {
    #[serde(default = "default_headsetcontrol_binary")]
    pub binary_path: String,
    #[serde(default)]
    pub device_match: Option<String>,
    #[serde(default = "default_headsetcontrol_timeout")]
    pub timeout_seconds: u64,
}

impl HeadsetControlConfig {
    /// Effective per-invocation timeout for the headsetcontrol process.
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_seconds.clamp(5, 120))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeyboardConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_vid")]
    pub keyboard_vid: u16,
    #[serde(default = "default_pid")]
    pub keyboard_pid: u16,
    #[serde(default = "default_link_vid")]
    pub receiver_vid: u16,
    #[serde(default = "default_receiver_pids")]
    pub receiver_pids: Vec<u16>,
    #[serde(default = "default_usage_page")]
    pub usage_page: u16,
    #[serde(default = "default_usage")]
    pub usage: u16,
    #[serde(default)]
    pub hidraw_path: Option<String>,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MouseConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_mouse_vid")]
    pub vendor_id: u16,
    #[serde(default = "default_mouse_receiver_pid")]
    pub receiver_pid: u16,
    #[serde(default = "default_mouse_wired_pids")]
    pub wired_pids: Vec<u16>,
    #[serde(default = "default_mouse_interface")]
    pub interface_number: i32,
    #[serde(default = "default_mouse_timeout_ms")]
    pub timeout_ms: i32,
    #[serde(default = "default_mouse_retries")]
    pub retries: u32,
    #[serde(default = "default_mouse_retry_delay_ms")]
    pub retry_delay_ms: u64,
}

impl MouseConfig {
    /// Effective per-attempt response timeout, clamped to 50..=5000 ms so a
    /// zero/negative value cannot turn the poll into a busy loop.
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.clamp(50, 5000) as u64)
    }

    /// Query attempts per poll, clamped to 1..=10.
    pub fn retries(&self) -> u32 {
        self.retries.clamp(1, 10)
    }

    /// Pause between retry attempts, clamped to 0..=1000 ms.
    pub fn retry_delay(&self) -> Duration {
        Duration::from_millis(self.retry_delay_ms.min(1000))
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("invalid TOML in {}", path.display()))
    }

    pub fn refresh_seconds(&self) -> u64 {
        self.refresh_seconds.max(60)
    }

    pub fn low_battery_threshold(&self) -> u8 {
        self.low_battery_threshold.min(100)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_seconds: default_refresh(),
            low_battery_threshold: default_low_battery_threshold(),
            headsetcontrol: HeadsetControlConfig::default(),
            keyboard: KeyboardConfig::default(),
            mouse: MouseConfig::default(),
        }
    }
}

impl Default for HeadsetControlConfig {
    fn default() -> Self {
        Self {
            binary_path: default_headsetcontrol_binary(),
            device_match: None,
            timeout_seconds: default_headsetcontrol_timeout(),
        }
    }
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keyboard_vid: default_vid(),
            keyboard_pid: default_pid(),
            receiver_vid: default_link_vid(),
            receiver_pids: default_receiver_pids(),
            usage_page: default_usage_page(),
            usage: default_usage(),
            hidraw_path: Some("/dev/hidraw5".into()),
            protocol: default_protocol(),
            timeout_ms: default_timeout_ms(),
        }
    }
}

fn default_refresh() -> u64 {
    600
}
fn default_low_battery_threshold() -> u8 {
    DEFAULT_LOW_BATTERY_THRESHOLD
}
fn default_headsetcontrol_binary() -> String {
    "headsetcontrol".into()
}
fn default_headsetcontrol_timeout() -> u64 {
    15
}
fn default_true() -> bool {
    true
}
fn default_vid() -> u16 {
    0x3434
}
fn default_pid() -> u16 {
    0x0E21
}
fn default_link_vid() -> u16 {
    0x3434
}
fn default_receiver_pids() -> Vec<u16> {
    vec![0xD026, 0xD027, 0xD030, 0xD031]
}
fn default_usage_page() -> u16 {
    0xFF60
}
fn default_usage() -> u16 {
    0x61
}
fn default_protocol() -> String {
    "auto".into()
}
fn default_timeout_ms() -> i32 {
    800
}
fn default_mouse_vid() -> u16 {
    0x3434
}
fn default_mouse_receiver_pid() -> u16 {
    0xD028
}
fn default_mouse_wired_pids() -> Vec<u16> {
    vec![0xD048, 0xD049]
}
fn default_mouse_interface() -> i32 {
    4
}
fn default_mouse_timeout_ms() -> i32 {
    500
}
fn default_mouse_retries() -> u32 {
    3
}
fn default_mouse_retry_delay_ms() -> u64 {
    100
}

impl Default for MouseConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            vendor_id: default_mouse_vid(),
            receiver_pid: default_mouse_receiver_pid(),
            wired_pids: default_mouse_wired_pids(),
            interface_number: default_mouse_interface(),
            timeout_ms: default_mouse_timeout_ms(),
            retries: default_mouse_retries(),
            retry_delay_ms: default_mouse_retry_delay_ms(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn mouse_defaults_match_ultra_link_8k_protocol() {
        let config = Config::default();
        assert!(config.mouse.enabled);
        assert_eq!(config.mouse.vendor_id, 0x3434);
        assert_eq!(config.mouse.receiver_pid, 0xD028);
        assert_eq!(config.mouse.wired_pids, vec![0xD048, 0xD049]);
        assert_eq!(config.mouse.interface_number, 4);
        assert_eq!(
            config.mouse.timeout(),
            std::time::Duration::from_millis(500)
        );
        assert_eq!(config.mouse.retries(), 3);
        assert_eq!(
            config.mouse.retry_delay(),
            std::time::Duration::from_millis(100)
        );
    }

    #[test]
    fn partial_mouse_config_uses_defaults() {
        let config: Config = toml::from_str("[mouse]\nenabled = false\n").unwrap();
        assert!(!config.mouse.enabled);
        assert_eq!(config.mouse.receiver_pid, 0xD028);
        assert_eq!(config.mouse.interface_number, 4);
    }

    #[test]
    fn mouse_timeout_is_clamped() {
        let config: Config = toml::from_str("[mouse]\ntimeout_ms = 0\n").unwrap();
        assert_eq!(config.mouse.timeout(), std::time::Duration::from_millis(50));
        let config: Config = toml::from_str("[mouse]\ntimeout_ms = 90000\n").unwrap();
        assert_eq!(
            config.mouse.timeout(),
            std::time::Duration::from_millis(5000)
        );
    }

    #[test]
    fn mouse_retries_are_clamped() {
        let config: Config = toml::from_str("[mouse]\nretries = 0\n").unwrap();
        assert_eq!(config.mouse.retries(), 1);
        let config: Config = toml::from_str("[mouse]\nretries = 99\n").unwrap();
        assert_eq!(config.mouse.retries(), 10);
    }

    #[test]
    fn mouse_config_without_section_defaults_to_enabled() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.mouse.enabled);
        assert!(config.keyboard.enabled);
    }
}
