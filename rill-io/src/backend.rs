//! Audio backend trait and related types

use crate::config::AudioConfig;
use crate::error::IoResult;
use std::fmt::Debug;
use std::time::Duration;

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

/// Audio backend trait
pub trait AudioBackend: Debug {
    /// Get the backend type
    fn backend_type(&self) -> BackendType;

    /// Get the configuration
    fn config(&self) -> &AudioConfig;

    /// Get the mutable configuration
    fn config_mut(&mut self) -> &mut AudioConfig;

    /// Initialize the backend
    fn init(&mut self) -> IoResult<()>;

    /// Start processing
    fn start(&mut self) -> IoResult<()>;

    /// Stop processing
    fn stop(&mut self) -> IoResult<()>;

    /// Read data from the input stream
    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize>;

    /// Write data to the output stream
    fn write(&mut self, buffer: &[f32]) -> IoResult<usize>;

    /// Number of xruns (underflows/overflows)
    fn xruns(&self) -> u32;

    /// Current latency
    fn latency(&self) -> Duration;

    /// Get list of available input devices
    fn list_input_devices(&self) -> Vec<String>;

    /// Get list of available output devices
    fn list_output_devices(&self) -> Vec<String>;
}

// Blanket impl so that `Box<dyn AudioBackend>` satisfies `B: AudioBackend`.
impl<T: AudioBackend + ?Sized> AudioBackend for Box<T> {
    fn backend_type(&self) -> BackendType {
        (**self).backend_type()
    }

    fn config(&self) -> &AudioConfig {
        (**self).config()
    }

    fn config_mut(&mut self) -> &mut AudioConfig {
        (**self).config_mut()
    }

    fn init(&mut self) -> IoResult<()> {
        (**self).init()
    }

    fn start(&mut self) -> IoResult<()> {
        (**self).start()
    }

    fn stop(&mut self) -> IoResult<()> {
        (**self).stop()
    }

    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize> {
        (**self).read(buffer)
    }

    fn write(&mut self, buffer: &[f32]) -> IoResult<usize> {
        (**self).write(buffer)
    }

    fn xruns(&self) -> u32 {
        (**self).xruns()
    }

    fn latency(&self) -> Duration {
        (**self).latency()
    }

    fn list_input_devices(&self) -> Vec<String> {
        (**self).list_input_devices()
    }

    fn list_output_devices(&self) -> Vec<String> {
        (**self).list_output_devices()
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
