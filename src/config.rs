use crate::notifications::DEFAULT_LOW_BATTERY_THRESHOLD;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_refresh")]
    pub refresh_seconds: u64,
    #[serde(default = "default_low_battery_threshold")]
    pub low_battery_threshold: u8,
    #[serde(default)]
    pub openlinkhub: OpenLinkHubConfig,
    #[serde(default)]
    pub keyboard: KeyboardConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenLinkHubConfig {
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub device_match: Option<String>,
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
            openlinkhub: OpenLinkHubConfig::default(),
            keyboard: KeyboardConfig::default(),
        }
    }
}

impl Default for OpenLinkHubConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            device_match: None,
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
fn default_base_url() -> String {
    "http://127.0.0.1:27003".into()
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
