#[cfg(feature = "serialization")]
use serde::Deserialize;

/// Modular system configuration types.
///
/// This module holds **host-level** parameters: sample rate, block size, and
/// the optional OSC bind address. A rack's program is part of the rack
/// document itself (see [`super::serialization`]).
#[cfg_attr(feature = "serialization", derive(Deserialize))]
pub struct ModularConfig {
    /// Sample rate (default 48000.0).
    pub sample_rate: f32,

    /// Block / buffer size (default 256).
    pub block_size: usize,

    /// OSC listen address, e.g. `"0.0.0.0:9999"`.
    /// `None` = no OSC server.
    #[cfg(feature = "osc")]
    pub osc_bind: Option<String>,
}

impl ModularConfig {
    /// Create a default modular system configuration.
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            block_size: 256,
            #[cfg(feature = "osc")]
            osc_bind: None,
        }
    }
}

impl Default for ModularConfig {
    fn default() -> Self {
        Self::new()
    }
}
