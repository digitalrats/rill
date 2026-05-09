//! Global PipeWire context — lazily initialised once, shared by
//! AudioInput and AudioOutput nodes via [`PwBuffers`].

use std::sync::Arc;

use crate::PwBuffers;
use rill_core::io::IoBackend;

type BackendTriple = (Box<dyn IoBackend<f32>>, Arc<PwBuffers>);

/// Ensure a PipeWire backend is available (stub when `pipewire` feature is disabled).
#[cfg(not(feature = "pipewire"))]
pub fn ensure(_sample_rate: u32, _buf_size: u32, _channels: u32) -> Option<BackendTriple> {
    None
}

/// Ensure a PipeWire backend with the given configuration is available.
#[cfg(feature = "pipewire")]
pub fn ensure(sample_rate: u32, buf_size: u32, channels: u32) -> Option<BackendTriple> {
    use crate::backends::PipewireBackend;
    use crate::config::AudioConfig;

    let config = AudioConfig::new()
        .with_sample_rate(sample_rate)
        .with_buffer_size(buf_size)
        .with_channels(channels);
    let backend = PipewireBackend::new(config).ok()?;
    let rings = backend.rings();
    let backend: Box<dyn IoBackend<f32>> = Box::new(backend);
    Some((backend, rings))
}
