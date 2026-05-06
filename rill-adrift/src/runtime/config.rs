#[cfg(feature = "serialization")]
use std::path::PathBuf;

/// Host-level configuration for a [`Runtime`](super::Runtime).
///
/// Separate from `rill_graph::serialization::GraphDocument` and
/// `rill_patchbay::document::PatchbayDocument` — this struct holds
/// **host-level** parameters: sample rate, OSC bind address,
/// and optional paths to initial preset files.
pub struct RuntimeConfig {
    /// Audio sample rate (default 48000.0).
    pub sample_rate: f32,

    /// Block / buffer size (default 256).
    pub block_size: usize,

    /// Optional path to a `GraphDocument` JSON file to load at startup.
    #[cfg(feature = "serialization")]
    pub graph_path: Option<PathBuf>,

    /// Optional path to a `PatchbayDocument` JSON file to load at startup.
    #[cfg(feature = "serialization")]
    pub patchbay_path: Option<PathBuf>,

    /// OSC listen address, e.g. `"0.0.0.0:9999"`.
    /// `None` = no OSC server.
    #[cfg(feature = "osc")]
    pub osc_bind: Option<String>,
}

impl RuntimeConfig {
    /// Create a default runtime configuration.
    pub fn new() -> Self {
        Self {
            sample_rate: 48000.0,
            block_size: 256,
            #[cfg(feature = "serialization")]
            graph_path: None,
            #[cfg(feature = "serialization")]
            patchbay_path: None,
            #[cfg(feature = "osc")]
            osc_bind: None,
        }
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self::new()
    }
}
