//! Serialization for modular system documents.
//!
//! A [`ModularSystemDef`] describes a modular processing system — one or more
//! racks, each with a rill-lang program and control modules — in a single
//! self-contained JSON document.
//!
//! The document round-trips through JSON and validates on load; it does not
//! imply a live control plane. The current host constructs control modules but
//! wires no clock or parameter source, so a servo, sensor, or clock has nothing
//! to drive (see the [`modular`](super) module docs).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use rill_core::traits::ParamValue;
use rill_patchbay::serialization::{AutomatonDef, ClockDef, MappingDef, SensorDef, ServoDef};

// ============================================================================
// ModuleDef
// ============================================================================

/// A rack module — Servo (automaton → parameter), Sensor, Clock, or Custom
/// (factory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModuleDef {
    /// MIDI clock output module. Constructed, but not driven: the current host
    /// wires no clock source, so no ticks reach it after construction.
    Clock(ClockDef),
    /// Servo — an automaton → parameter bridge. Constructed, but not driven:
    /// no clock ticks reach it in the current inert module host.
    Servo(ServoDef),
    /// Sensor — external input source (MIDI, OSC, etc.). Constructed, but the
    /// current host wires no consumer for its events.
    Sensor(SensorDef),
    /// Custom module constructed via the module factory.
    Custom {
        /// Factory constructor key.
        type_name: String,
        /// Constructor parameters.
        #[serde(default)]
        params: HashMap<String, ParamValue>,
    },
}

// ============================================================================
// RackDef
// ============================================================================

/// A modular processing rack — one rill-lang program + its control modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RackDef {
    /// Human-readable rack identifier.
    pub name: String,
    /// rill-lang program source: `main : IO ()`.
    ///
    /// The program is compiled and executed to completion by the IO runner on
    /// the rack's own thread. A `main` that does not type as `IO ()` is a
    /// compile error, reported by `launch`.
    pub program: String,
    /// Arguments bound to `main`'s former String λ-parameters, in order.
    ///
    /// Retained for format compatibility only: `main` no longer takes launch
    /// arguments, and `launch` ignores this field.
    #[serde(default)]
    pub args: Vec<String>,
    /// Control automatons (LFO, envelope, sequencer) powering servos.
    #[serde(default)]
    pub automatons: Vec<AutomatonDef>,
    /// Rack modules — servos, sensors, clocks, and custom modules. They are
    /// constructed at launch; the current host does not drive them.
    #[serde(default)]
    pub modules: Vec<ModuleDef>,
    /// Parameter-to-UI/controller mappings. Reserved for the future control
    /// plane: nothing consumes them in the current inert module host.
    #[serde(default)]
    pub mappings: Vec<MappingDef>,
    /// Optional human-readable description of the rack.
    pub description: Option<String>,
}

impl RackDef {
    /// Create a new rack definition with the given name and program source.
    pub fn new(name: impl Into<String>, program: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args: Vec::new(),
            automatons: Vec::new(),
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
    /// Processing racks, each with its own program and control modules.
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
