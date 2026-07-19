#![forbid(unsafe_code)]

use std::fmt;

use pulsehub_core::{DeviceCapabilities, DeviceIdentity, Profile};

pub mod discovery;
pub mod hidpp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceState {
    pub current_dpi: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    pub dpi_applied: bool,
    pub button_mappings_applied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceError {
    Unsupported,
    Disconnected,
    Busy,
    Protocol(String),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => formatter.write_str("设备或功能不受支持"),
            Self::Disconnected => formatter.write_str("设备已断开"),
            Self::Busy => formatter.write_str("设备忙"),
            Self::Protocol(message) => write!(formatter, "HID++ 协议错误：{message}"),
        }
    }
}

impl std::error::Error for DeviceError {}

pub trait MouseDevice: Send {
    fn identity(&self) -> &DeviceIdentity;
    fn capabilities(&self) -> &DeviceCapabilities;
    fn read_state(&mut self) -> Result<DeviceState, DeviceError>;
    fn apply_profile(&mut self, profile: &Profile) -> Result<ApplyReport, DeviceError>;
}
