use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct DeviceState {
    pub name: String,
    pub percent: Option<u8>,
    pub charging: Option<bool>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub headset: DeviceState,
    pub keyboard: DeviceState,
}
