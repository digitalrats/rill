//! Serialization for complete modular system documents.
//!
//! A [`ModularSystemDef`] describes an entire modular audio processing
//! system — cases with their graphs and patchbays — in a single
//! self-contained JSON document.

use serde::{Deserialize, Serialize};

use rill_graph::serialization::GraphDef;
use rill_patchbay::serialization::PatchbayDef;

/// Top-level document for a complete modular audio processing system.
///
/// Contains everything needed to reconstruct: cases with their graphs
/// and patchbays, shared configuration, and inter-case routing.
///
/// # JSON format
///
/// ```json
/// {
///   "format_version": "rill/1",
///   "sample_rate": 48000.0,
///   "block_size": 256,
///   "cases": [
///     {
///       "name": "synth",
///       "graph": { ... },
///       "patchbay": null
///     }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModularSystemDef {
    /// Format identifier (`"rill/1"`).
    pub format_version: String,

    /// Global sample rate in Hz.
    pub sample_rate: f32,

    /// Processing block size in samples.
    pub block_size: usize,

    /// Eurorack cases that make up the system.
    pub cases: Vec<CaseDef>,

    /// Optional human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A single Eurorack case within a modular system.
///
/// Each case has its own signal graph and, optionally, a control
/// patchbay for automation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseDef {
    /// Case name — unique within the system.
    pub name: String,

    /// Signal graph topology (nodes, connections, resources).
    pub graph: GraphDef,

    /// Control patchbay configuration (automata, mappings, sensors).
    /// `None` means no control rack — audio passthrough only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patchbay: Option<PatchbayDef>,
}

// --- JSON serialization ---

/// Serialise a system definition to pretty-printed JSON.
pub fn to_json(def: &ModularSystemDef) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(def)
}

/// Deserialise a system definition from a JSON string.
pub fn from_json(json: &str) -> Result<ModularSystemDef, serde_json::Error> {
    serde_json::from_str(json)
}
