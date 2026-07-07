//! Serialization for modular system documents.
//!
//! A [`ModularSystemDef`] describes a modular processing system — one or more
//! racks, each with a signal graph and control modules — in a single
//! self-contained JSON document.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use rill_core::traits::ParamValue;
use rill_graph::serialization::GraphDef;
use rill_patchbay::serialization::{AutomatonDef, ClockDef, MappingDef, SensorDef, ServoDef};

// ============================================================================
// ModuleDef
// ============================================================================

/// A rack module — Servo (automaton → parameter), Sensor, Custom (factory),
/// or Graph (signal graph with its own I/O loop).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModuleDef {
    /// MIDI clock output module.
    Clock(ClockDef),
    /// Servo — bridges an automaton (LFO, envelope, etc.) to a graph parameter.
    Servo(ServoDef),
    /// Sensor — external input source (MIDI, OSC, etc.).
    Sensor(SensorDef),
    /// Custom module constructed via the module factory.
    Custom {
        /// Factory constructor key.
        type_name: String,
        /// Constructor parameters.
        #[serde(default)]
        params: HashMap<String, ParamValue>,
    },
    /// Signal graph — owns the I/O loop.
    Graph {
        /// The embedded signal graph definition.
        graph: GraphDef,
    },
}

// ============================================================================
// RackDef
// ============================================================================

/// A modular processing rack — one signal graph + its control modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RackDef {
    /// Human-readable rack identifier.
    pub name: String,
    /// Signal graph topology.
    pub graph: GraphDef,
    /// Control automatons (LFO, envelope, sequencer) powering servos.
    #[serde(default)]
    pub automata: Vec<AutomatonDef>,
    /// Rack modules — servos, sensors, and custom modules.
    #[serde(default)]
    pub modules: Vec<ModuleDef>,
    /// Parameter-to-UI/controller mappings.
    #[serde(default)]
    pub mappings: Vec<MappingDef>,
    /// Optional human-readable description of the rack.
    pub description: Option<String>,
}

impl RackDef {
    /// Create a new rack definition with the given name and signal graph.
    pub fn new(name: impl Into<String>, graph: GraphDef) -> Self {
        Self {
            name: name.into(),
            graph,
            automata: Vec::new(),
            modules: Vec::new(),
            mappings: Vec::new(),
            description: None,
        }
    }
}

// ============================================================================
// ModularSystemDef
// ============================================================================

/// Top-level document describing a full modular processing system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModularSystemDef {
    /// Format version string (e.g. `"1.0"`).
    pub format_version: String,
    /// Global sample rate in Hz.
    pub sample_rate: f32,
    /// Signal block size in samples.
    pub block_size: usize,
    /// Processing racks, each with its own signal graph and control modules.
    pub racks: Vec<RackDef>,
    /// Optional human-readable description of the system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Serialize a `ModularSystemDef` to pretty-printed JSON.
pub fn to_json(def: &ModularSystemDef) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(def)
}

/// Deserialize a `ModularSystemDef` from a JSON string.
pub fn from_json(json: &str) -> Result<ModularSystemDef, serde_json::Error> {
    serde_json::from_str(json)
}
