use crate::config::OpenLinkHubConfig;
use crate::state::DeviceState;
use anyhow::{anyhow, Result};
use serde_json::Value;

pub struct OpenLinkHubReader {
    client: reqwest::Client,
    config: OpenLinkHubConfig,
}

impl OpenLinkHubReader {
    pub fn new(config: OpenLinkHubConfig) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder().build()?,
            config,
        })
    }

    pub async fn read(&self) -> Result<DeviceState> {
        let url = format!("{}/api/systray", self.config.base_url.trim_end_matches('/'));
        let value: Value = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut candidates = Vec::new();
        collect_devices(&value, &mut candidates);

        let filtered = candidates
            .into_iter()
            .filter(|(name, _, _)| {
                self.config
                    .device_match
                    .as_ref()
                    .map_or(true, |m| name.to_lowercase().contains(&m.to_lowercase()))
            })
            .collect::<Vec<_>>();

        let (name, percent, charging) = filtered
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no battery device found in /api/systray"))?;
        Ok(DeviceState {
            name,
            percent: Some(percent),
            charging,
            error: None,
        })
    }
}

fn collect_devices(v: &Value, out: &mut Vec<(String, u8, Option<bool>)>) {
    match v {
        Value::Array(items) => items.iter().for_each(|x| collect_devices(x, out)),
        Value::Object(map) => {
            let name = map
                .get("Device")
                .or_else(|| map.get("device"))
                .or_else(|| map.get("Name"))
                .or_else(|| map.get("name"))
                .and_then(Value::as_str);
            let level = map
                .get("Level")
                .or_else(|| map.get("level"))
                .or_else(|| map.get("Battery"))
                .or_else(|| map.get("battery"))
                .and_then(number_u8);
            let charging = map
                .get("Charging")
                .or_else(|| map.get("charging"))
                .and_then(Value::as_bool);
            if let (Some(name), Some(level)) = (name, level) {
                if level <= 100 {
                    out.push((name.to_string(), level, charging));
                }
            }
            map.values().for_each(|x| collect_devices(x, out));
        }
        _ => {}
    }
}

fn number_u8(v: &Value) -> Option<u8> {
    v.as_u64()
        .and_then(|n| u8::try_from(n).ok())
        .or_else(|| v.as_f64().map(|n| n.round() as u8))
}
