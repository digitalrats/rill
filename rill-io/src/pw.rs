//! Global PipeWire context — lazily initialised once, shared by
//! AudioInput and AudioOutput nodes via [`PwBuffers`].
//!
//! The first node factory that calls `ensure()` creates the PW main loop
//! thread with both input and output streams. Subsequent calls return the
//! same ring handles — no duplication.

use std::sync::Arc;

use crate::PwBuffers;

// ─── Stub (no PipeWire feature) ─────────────────────────────────────

#[cfg(not(feature = "pipewire"))]
pub fn ensure(_sample_rate: u32, _buf_size: u32, _channels: u32) -> Option<&'static Arc<PwBuffers>> {
    None
}

// ─── Real PipeWire initialisation ──────────────────────────────────

#[cfg(feature = "pipewire")]
pub fn ensure(
    sample_rate: u32,
    buf_size: u32,
    channels: u32,
) -> Option<&'static Arc<PwBuffers>> {
    use std::sync::OnceLock;

    use crate::backends::pipewire::PipewireBackend;
    use crate::config::AudioConfig;

    struct PwContext {
        rings: Arc<PwBuffers>,
        _backend: PipewireBackend,
    }

    static CTX: OnceLock<Result<PwContext, String>> = OnceLock::new();

    let result = CTX.get_or_init(|| {
        let config = AudioConfig::new()
            .with_sample_rate(sample_rate)
            .with_buffer_size(buf_size)
            .with_channels(channels);
        let backend = PipewireBackend::new(config).map_err(|e| e.to_string())?;
        let rings = backend.rings();
        backend.start().map_err(|e| e.to_string())?;
        Ok(PwContext {
            rings,
            _backend: backend,
        })
    });

    match result {
        Ok(ctx) => Some(&ctx.rings),
        Err(e) => {
            log::warn!("PipeWire init failed: {e}");
            None
        }
    }
}
