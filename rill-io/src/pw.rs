//! Global PipeWire context — lazily initialised once, shared by
//! AudioInput and AudioOutput nodes via [`PwBuffers`].

use std::sync::Arc;

use crate::audio_io::{AudioIo, AudioIoPtr};
use crate::PwBuffers;

/// Ensure a PipeWire backend is available (stub when `pipewire` feature is disabled).
///
/// Returns `None` — PipeWire is not compiled in.
#[cfg(not(feature = "pipewire"))]
pub fn ensure(
    _sample_rate: u32,
    _buf_size: u32,
    _channels: u32,
) -> Option<(Box<dyn AudioIo>, Arc<PwBuffers>, AudioIoPtr)> {
    None
}

/// Ensure a PipeWire backend with the given configuration is available.
///
/// Creates the backend, stores its ring buffers, and starts the stream.
/// Returns the backend, shared buffer handles, and a borrow pointer.
#[cfg(feature = "pipewire")]
pub fn ensure(
    sample_rate: u32,
    buf_size: u32,
    channels: u32,
) -> Option<(Box<dyn AudioIo>, Arc<PwBuffers>, AudioIoPtr)> {
    use crate::backends::PipewireBackend;
    use crate::config::AudioConfig;

    let config = AudioConfig::new()
        .with_sample_rate(sample_rate)
        .with_buffer_size(buf_size)
        .with_channels(channels);
    let backend = PipewireBackend::new(config).ok()?;
    let ptr = AudioIoPtr::from_ref(&backend as &dyn AudioIo);
    let rings = backend.rings();
    backend.start().ok()?;
    Some((Box::new(backend) as Box<dyn AudioIo>, rings, ptr))
}
