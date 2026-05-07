#[cfg(feature = "serialization")]
use std::path::PathBuf;

use std::collections::HashMap;

#[cfg(feature = "serialization")]
use serde::Deserialize;

/// Host-level configuration for a [`Runtime`](super::Runtime).
///
/// Separate from `rill_graph::serialization::GraphDef` and
/// `rill_patchbay::serialization::PatchbayDef` — this struct holds
/// **host-level** parameters: sample rate, default backend config,
/// OSC bind address, and optional paths to initial preset files.
///
/// The `backend_name` + `backend_params` pair sets the default audio
/// backend via [`Runtime::set_default_backend`](super::Runtime::set_default_backend)
/// at construction time. All values in `backend_params` are strings —
/// each backend constructor is responsible for parsing them.
#[cfg_attr(feature = "serialization", derive(Deserialize))]
pub struct RuntimeConfig {
    /// Audio sample rate (default 48000.0).
    pub sample_rate: f32,

    /// Block / buffer size (default 256).
    pub block_size: usize,

    /// Default audio backend name (e.g. `"pipewire"`, `"alsa"`, `"null"`).
    /// `None` = no default backend (graph built without audio I/O).
    pub backend_name: Option<String>,

    /// Raw string-keyed parameters for the default backend.
    /// Converted to `HashMap<String, ParamValue>` at Runtime creation.
    /// Typical keys: `"sample_rate"`, `"buffer_size"`, `"channels"`.
    pub backend_params: HashMap<String, String>,

    /// Optional path to a `GraphDef` JSON file to load at startup.
    #[cfg(feature = "serialization")]
    pub graph_path: Option<PathBuf>,

    /// Optional path to a `PatchbayDef` JSON file to load at startup.
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
            backend_name: None,
            backend_params: HashMap::new(),
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
