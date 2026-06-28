//! Global PipeWire context — lazily initialised once.
//!
//! In the new architecture backends are created externally via
//! `BackendFactory` and implement `IoDriver` + optionally
//! `IoCapture` / `IoPlayback`.

use std::sync::Arc;

use rill_core::io::IoDriver;

/// Ensure a PipeWire backend is available (stub when `pipewire` feature is disabled).
#[cfg(not(feature = "pipewire"))]
pub fn ensure(_sample_rate: u32, _buf_size: u32, _channels: u32) -> Option<Box<dyn IoDriver>> {
    None
}

/// Ensure a PipeWire backend with the given configuration is available.
#[cfg(feature = "pipewire")]
pub fn ensure(sample_rate: u32, buf_size: u32, channels: u32) -> Option<Box<dyn IoDriver>> {
    use crate::backends::PipewireBackend;
    use crate::config::AudioConfig;

    let config = AudioConfig::new()
        .with_sample_rate(sample_rate)
        .with_buffer_size(buf_size)
        .with_channels(channels);
    PipewireBackend::new(config)
        .ok()
        .map(|b| Box::new(b) as Box<dyn IoDriver>)
}
