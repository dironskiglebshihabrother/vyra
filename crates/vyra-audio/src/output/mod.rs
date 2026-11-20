//! Audio output devices.

use crate::spec::StreamSpec;

#[cfg(windows)]
pub mod wasapi;

/// An output device as the system lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable identifier, usable to open the device later.
    pub id: String,
    /// Name shown to the user.
    pub name: String,
    /// True for the device Windows plays to by default.
    pub is_default: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("output device not found")]
    DeviceNotFound,
    #[error("the device is used by another program")]
    DeviceBusy,
    #[error("exclusive mode is switched off for this device in the Windows sound settings")]
    ExclusiveNotAllowed,
    #[error("the device does not accept {spec} in exclusive mode")]
    FormatNotSupported { spec: StreamSpec },
    #[error("the output stream stopped: {0}")]
    Stopped(String),
    #[error("audio system error: {0}")]
    Backend(String),
}
