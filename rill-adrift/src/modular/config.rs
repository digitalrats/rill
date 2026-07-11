#[cfg(feature = "serialization")]
use std::path::PathBuf;

use std::collections::HashMap;

#[cfg(feature = "serialization")]
use serde::Deserialize;

#[cfg(feature = "serialization")]
use rill_graph::serialization::GraphDef;
#[cfg(feature = "serialization")]
use rill_patchbay::serialization::PatchbayDef;

/// Modular system configuration types.
///
/// Separate from `rill_graph::serialization::GraphDef` and
/// `rill_patchbay::serialization::PatchbayDef` — this module holds
/// **host-level** parameters: sample rate, default backend config,
/// OSC bind address, and optional paths to initial preset files.
///
/// The `backend_name` + `backend_params` pair sets the default I/O
/// backend via [`ModularSystem::set_default_backend`](super::ModularSystem::set_default_backend)
/// at construction time. All values in `backend_params` are strings —
/// each backend constructor is responsible for parsing them.
#[cfg_attr(feature = "serialization", derive(Deserialize))]
pub struct ModularConfig {
    /// Sample rate (default 48000.0).
    pub sample_rate: f32,

    /// Block / buffer size (default 256).
    pub block_size: usize,

    /// Default I/O backend name (e.g. `"pipewire"`, `"alsa"`, `"null"`).
    /// `None` = no default backend (graph built without I/O).
    pub backend_name: Option<String>,

    /// Raw string-keyed parameters for the default backend.
    /// Converted to `HashMap<String, ParamValue>` at modular system creation.
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

impl ModularConfig {
    /// Create a default modular system configuration.
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

impl Default for ModularConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// LaunchConfig — all-in-one construction for Runtime::launch()
// ============================================================================

/// Configuration for [`ModularSystem::launch`](super::ModularSystem::launch).
///
/// Bundles everything needed to build and start both racks
/// (signal graph + control patchbay) in one call.
#[cfg(feature = "serialization")]
pub struct LaunchConfig {
    /// Sample rate (e.g. 48000.0).
    pub sample_rate: f32,

    /// Block / buffer size (e.g. 256).
    pub block_size: usize,

    /// I/O backend name (`"pipewire"`, `"alsa"`, `"null"`).
    pub backend_name: Option<String>,

    /// Raw string-keyed backend parameters
    /// (`"channels"`, `"buffer_size"`, etc.).
    pub backend_params: HashMap<String, String>,

    /// Signal graph topology (nodes, connections, resources).
    pub graph_def: GraphDef,

    /// Control rack configuration (automatons, mappings, MIDI).
    /// `None` = no control rack, signal passthrough only.
    pub rack_def: Option<PatchbayDef>,
}

#[cfg(feature = "serialization")]
impl LaunchConfig {
    /// Create a minimal launch configuration from a [`GraphDef`].
    pub fn from_graph(graph_def: GraphDef) -> Self {
        Self {
            sample_rate: graph_def.sample_rate,
            block_size: graph_def.block_size,
            backend_name: None,
            backend_params: HashMap::new(),
            graph_def,
            rack_def: None,
        }
    }
}
