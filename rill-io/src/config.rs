//! Audio device configuration

use crate::backend::BackendType;

/// Default number of `buffer_size` blocks per I/O callback DMA buffer.
fn default_buffer_blocks() -> usize {
    16
}

/// Audio device configuration
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-config", derive(serde::Serialize, serde::Deserialize))]
pub struct AudioConfig {
    /// Sample rate (Hz)
    pub sample_rate: u32,

    /// Buffer size (in samples)
    pub buffer_size: u32,

    /// Number of `buffer_size` blocks per I/O callback DMA buffer.
    ///
    /// Only used by callback-driven backends that can size their DMA buffer
    /// (PipeWire via `SPA_PARAM_Buffers`, PortAudio via `frames_per_buffer`).
    /// A single `buffer_size` (256-frame) period is unstable through PipeWire
    /// (crackling), so the backend requests `buffer_size × buffer_blocks` frames
    /// and chunks it back into `buffer_size` pieces (one `ClockTick` per block).
    /// The buffer duration is also the async-control look-ahead
    /// (`ClockTick.io_quantum`), so larger values are more robust on
    /// constrained/untuned systems but add control latency; the stable minimum
    /// is hardware/config dependent. ALSA (period fixed to `buffer_size`) and
    /// JACK (buffer size set by the JACK server) ignore this.
    #[cfg_attr(feature = "serde-config", serde(default = "default_buffer_blocks"))]
    pub buffer_blocks: usize,

    /// Number of input channels
    pub input_channels: u32,

    /// Number of output channels
    pub output_channels: u32,

    /// Target latency (ms)
    pub target_latency_ms: u32,

    /// Input device name (if None, uses default)
    pub input_device: Option<String>,

    /// Output device name (if None, uses default)
    pub output_device: Option<String>,

    /// Backend type
    pub backend_type: BackendType,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            buffer_size: 256,
            buffer_blocks: default_buffer_blocks(),
            input_channels: 2,
            output_channels: 2,
            target_latency_ms: 10,
            input_device: None,
            output_device: None,
            backend_type: BackendType::Cpal,
        }
    }
}

impl AudioConfig {
    /// Create a new configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the sample rate
    pub fn with_sample_rate(mut self, sample_rate: u32) -> Self {
        self.sample_rate = sample_rate;
        self
    }

    /// Set the buffer size
    pub fn with_buffer_size(mut self, buffer_size: u32) -> Self {
        self.buffer_size = buffer_size;
        self
    }

    /// Set the number of `buffer_size` blocks per I/O callback DMA buffer
    /// (callback-driven backends only; see [`Self::buffer_blocks`]).
    pub fn with_buffer_blocks(mut self, buffer_blocks: usize) -> Self {
        self.buffer_blocks = buffer_blocks;
        self
    }

    /// Set the number of channels (same for input and output)
    pub fn with_channels(mut self, channels: u32) -> Self {
        self.input_channels = channels;
        self.output_channels = channels;
        self
    }

    /// Set the number of input channels
    pub fn with_input_channels(mut self, channels: u32) -> Self {
        self.input_channels = channels;
        self
    }

    /// Set the number of output channels
    pub fn with_output_channels(mut self, channels: u32) -> Self {
        self.output_channels = channels;
        self
    }

    /// Set the input device
    pub fn with_input_device(mut self, device: impl Into<String>) -> Self {
        self.input_device = Some(device.into());
        self
    }

    /// Set the output device
    pub fn with_output_device(mut self, device: impl Into<String>) -> Self {
        self.output_device = Some(device.into());
        self
    }

    /// Set the backend type
    pub fn with_backend(mut self, backend: BackendType) -> Self {
        self.backend_type = backend;
        self
    }

    /// Calculate actual latency in seconds
    pub fn latency_seconds(&self) -> f64 {
        self.buffer_size as f64 / self.sample_rate as f64
    }

    /// Calculate actual latency in milliseconds
    pub fn latency_ms(&self) -> f64 {
        self.latency_seconds() * 1000.0
    }
}
