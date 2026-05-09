//! Backend types and device info (legacy).

use std::fmt::Debug;

/// Backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-config", derive(serde::Serialize, serde::Deserialize))]
pub enum BackendType {
    /// CPAL (cross-platform)
    Cpal,
    /// ALSA (Linux)
    Alsa,
    /// PipeWire (Linux)
    PipeWire,
    /// JACK (Linux/macOS)
    Jack,
    /// Null (testing)
    Null,
}

impl BackendType {
    /// Get the backend name
    pub fn name(&self) -> &'static str {
        match self {
            BackendType::Cpal => "CPAL",
            BackendType::Alsa => "ALSA",
            BackendType::PipeWire => "PipeWire",
            BackendType::Jack => "JACK",
            BackendType::Null => "Null",
        }
    }

    /// Whether the backend is available on the current platform
    pub fn is_available(&self) -> bool {
        match self {
            BackendType::Cpal => true,
            BackendType::Alsa => cfg!(target_os = "linux"),
            BackendType::PipeWire => cfg!(target_os = "linux"),
            BackendType::Jack => cfg!(any(target_os = "linux", target_os = "macos")),
            BackendType::Null => true,
        }
    }
}

/// Device information
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Device name
    pub name: String,
    /// Backend type
    pub backend: BackendType,
    /// Whether this is the default device
    pub is_default: bool,
    /// Maximum number of input channels
    pub max_input_channels: u32,
    /// Maximum number of output channels
    pub max_output_channels: u32,
    /// Supported sample rates
    pub supported_sample_rates: Vec<u32>,
    /// Supported buffer sizes
    pub supported_buffer_sizes: Vec<u32>,
}
