use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct DeviceState {
    pub name: String,
    pub percent: Option<u8>,
    pub charging: Option<bool>,
    pub error: Option<String>,
}

impl DeviceState {
    /// The reader-provided device name, or `fallback` when it is unset/blank.
    pub fn display_name(&self, fallback: &str) -> String {
        if self.name.trim().is_empty() {
            fallback.to_string()
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub headset: DeviceState,
    pub keyboard: DeviceState,
}
