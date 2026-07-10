//! Serialization of graph topology and node parameters.
//!
//! # Feature gate
//!
//! This module is available only when the `serialization` feature is enabled
//! (requires `serde`, `serde_json`, `serde_cbor`).
//!
//! # Formats
//!
//! - **JSON** — human-readable, for debugging / manual editing.
//! - **CBOR** — compact binary, for network transfer and preset storage.
//!
//! Both encode the same `GraphDef` structure.

use std::collections::{HashMap, HashSet};

use rill_core::math::Transcendental;
use rill_core::traits::{NodeId, ParamValue, Params};

use crate::graph::GraphBuilder;

use serde::de;
use serde::{Deserialize, Serialize};

// ============================================================================
// GraphDef structure
// ============================================================================

/// A named resource (e.g. a tape loop) shared between graph nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDef {
    /// Unique name referenced by node parameters (e.g. `"tape_0"`).
    pub name: String,
    /// Resource kind: `"tape"` for a [`TapeLoop`](rill_core::buffer::TapeLoop).
    pub kind: String,
    /// Capacity in samples (for `"tape"` kind).
    pub capacity: usize,
}

/// A serialisable graph document.
///
/// Contains everything needed to reconstruct a signal graph:
/// node definitions with parameters, named resources, and connections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphDef {
    /// Format identifier for forward compatibility (e.g. `"rill/1"`).
    pub format_version: String,

    /// Sample rate the graph was designed for.
    pub sample_rate: f32,

    /// Block / buffer size.
    pub block_size: usize,

    /// Named resources shared between nodes (tape loops, etc.).
    #[serde(default)]
    pub resources: Vec<ResourceDef>,

    /// Node definitions.
    pub nodes: Vec<NodeDef>,

    /// Connection wiring.
    pub connections: Vec<ConnectionDef>,

    /// Optional human-readable description (attribution, preset notes, ...).
    /// Not interpreted by the engine; preserved through serialisation round-trips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A single node in the serialised graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeDef {
    /// Signal source — generates signal (oscillator, input, sampler).
    Source(SourceDef),
    /// DSP processor — transforms signal (filter, effect, gain).
    Processor(ProcessorDef),
    /// Signal router — N×M dynamic routing (mixer, splitter).
    Router(RouterDef),
    /// Signal sink — consumes signal (output, recorder).
    Sink(SinkDef),
}

/// Definition of a source node (generates signal).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDef {
    /// Unique node identifier.
    pub id: u32,
    /// Canonical type name for factory lookup.
    pub type_name: String,
    /// Human-readable instance name.
    pub name: String,
    /// Optional backend name for I/O nodes (e.g. `"portaudio"`, `"ay38910"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// Runtime parameters.
    #[serde(
        deserialize_with = "deserialize_params",
        serialize_with = "serialize_params"
    )]
    pub parameters: HashMap<String, ParamValue>,
}

/// Definition of a processor node (transforms signal).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessorDef {
    /// Unique node identifier.
    pub id: u32,
    /// Canonical type name for factory lookup.
    pub type_name: String,
    /// Human-readable instance name.
    pub name: String,
    /// Runtime parameters.
    #[serde(
        deserialize_with = "deserialize_params",
        serialize_with = "serialize_params"
    )]
    pub parameters: HashMap<String, ParamValue>,
}

/// Definition of a router node (N×M signal routing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterDef {
    /// Unique node identifier.
    pub id: u32,
    /// Canonical type name for factory lookup.
    pub type_name: String,
    /// Human-readable instance name.
    pub name: String,
    /// Runtime parameters.
    #[serde(
        deserialize_with = "deserialize_params",
        serialize_with = "serialize_params"
    )]
    pub parameters: HashMap<String, ParamValue>,
    /// Pre-configured routing matrix entries (from_input, to_output, gain).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routing_matrix: Vec<RoutingEntry>,
}

/// A single entry in a router's N×M routing matrix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingEntry {
    /// Input port index.
    pub from: usize,
    /// Output port index.
    pub to: usize,
    /// Gain coefficient.
    pub gain: f32,
}

/// Definition of a sink node (consumes signal).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinkDef {
    /// Unique node identifier.
    pub id: u32,
    /// Canonical type name for factory lookup.
    pub type_name: String,
    /// Human-readable instance name.
    pub name: String,
    /// Optional backend name for I/O nodes (e.g. `"portaudio"`, `"ay38910"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// Runtime parameters.
    #[serde(
        deserialize_with = "deserialize_params",
        serialize_with = "serialize_params"
    )]
    pub parameters: HashMap<String, ParamValue>,
}

impl NodeDef {
    /// Return the node's unique identifier.
    pub fn id(&self) -> u32 {
        match self {
            NodeDef::Source(s) => s.id,
            NodeDef::Processor(p) => p.id,
            NodeDef::Router(r) => r.id,
            NodeDef::Sink(s) => s.id,
        }
    }

    /// Return the node's canonical type name.
    pub fn type_name(&self) -> &str {
        match self {
            NodeDef::Source(s) => &s.type_name,
            NodeDef::Processor(p) => &p.type_name,
            NodeDef::Router(r) => &r.type_name,
            NodeDef::Sink(s) => &s.type_name,
        }
    }

    /// Return the node's human-readable name.
    pub fn name(&self) -> &str {
        match self {
            NodeDef::Source(s) => &s.name,
            NodeDef::Processor(p) => &p.name,
            NodeDef::Router(r) => &r.name,
            NodeDef::Sink(s) => &s.name,
        }
    }

    /// Return the node's runtime parameters.
    pub fn parameters(&self) -> &HashMap<String, ParamValue> {
        match self {
            NodeDef::Source(s) => &s.parameters,
            NodeDef::Processor(p) => &p.parameters,
            NodeDef::Router(r) => &r.parameters,
            NodeDef::Sink(s) => &s.parameters,
        }
    }

    /// Return a mutable reference to the node's runtime parameters.
    pub fn parameters_mut(&mut self) -> &mut HashMap<String, ParamValue> {
        match self {
            NodeDef::Source(s) => &mut s.parameters,
            NodeDef::Processor(p) => &mut p.parameters,
            NodeDef::Router(r) => &mut r.parameters,
            NodeDef::Sink(s) => &mut s.parameters,
        }
    }

    /// Return the node's backend name (only meaningful for Source and Sink).
    pub fn backend(&self) -> Option<&str> {
        match self {
            NodeDef::Source(s) => s.backend.as_deref(),
            NodeDef::Processor(_) => None,
            NodeDef::Router(_) => None,
            NodeDef::Sink(s) => s.backend.as_deref(),
        }
    }

    /// Return the archetype kind as a static string.
    pub fn kind_str(&self) -> &'static str {
        match self {
            NodeDef::Source(_) => "Source",
            NodeDef::Processor(_) => "Processor",
            NodeDef::Router(_) => "Router",
            NodeDef::Sink(_) => "Sink",
        }
    }
}

/// A connection between two ports.
///
/// Nodes are identified by their [`NodeDef::id`](NodeDef::id) (not by position in the
/// `nodes` array).  The importer resolves IDs to indices internally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDef {
    /// Signal kind.
    pub kind: SignalKind,

    /// Source node [`NodeDef::id`].
    pub from_node: u32,

    /// Source port index.
    pub from_port: usize,

    /// Target node [`NodeDef::id`].
    pub to_node: u32,

    /// Target port index.
    pub to_port: usize,
}

/// Kind of signal carried by a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalKind {
    /// Sample-rate signal data.
    Signal,
    /// Low-frequency control signal (one value per block).
    Control,
    /// Clock/timing signal.
    Clock,
    /// Feedback loop signal (delay/state).
    Feedback,
}

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur during graph serialization / deserialization.
#[derive(Debug, Clone)]
pub enum SerializationError {
    /// A node type in the document is not registered in the factory.
    UnknownType(String),
    /// Two nodes in the document share the same [`NodeId`].
    DuplicateNodeId(u32),
    /// A required field is missing or malformed.
    InvalidFormat(String),
}

impl std::fmt::Display for SerializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownType(t) => write!(f, "unknown node type: {t}"),
            Self::DuplicateNodeId(id) => write!(f, "duplicate NodeId: {id}"),
            Self::InvalidFormat(d) => write!(f, "invalid format: {d}"),
        }
    }
}

impl std::error::Error for SerializationError {}

// ============================================================================
// Construction helpers (for incremental / interactive graph building)
// ============================================================================

impl GraphDef {
    /// Create an empty document with sensible defaults.
    pub fn new(sample_rate: f32, block_size: usize) -> Self {
        Self {
            format_version: "rill/1".to_string(),
            sample_rate,
            block_size,
            resources: Vec::new(),
            nodes: Vec::new(),
            connections: Vec::new(),
            description: None,
        }
    }

    /// Append a node definition.
    ///
    /// Returns an error if the node's id duplicates an existing one.
    pub fn add_node(&mut self, def: NodeDef) -> Result<(), SerializationError> {
        let id = def.id();
        if self.nodes.iter().any(|n| n.id() == id) {
            return Err(SerializationError::DuplicateNodeId(id));
        }
        self.nodes.push(def);
        Ok(())
    }

    /// Append a connection.
    ///
    /// Validity of the node IDs is checked only at [`populate`](Self::populate) time.
    pub fn add_connection(&mut self, conn: ConnectionDef) {
        self.connections.push(conn);
    }

    /// Set a parameter value on an existing node (identified by its id).
    pub fn set_node_param(&mut self, node_id: u32, key: &str, value: ParamValue) {
        if let Some(nd) = self.nodes.iter_mut().find(|n| n.id() == node_id) {
            nd.parameters_mut().insert(key.to_string(), value);
        }
    }

    /// Remove all nodes and connections.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.connections.clear();
    }
}

// ============================================================================
// Import (GraphDef → GraphBuilder)
// ============================================================================

impl GraphDef {
    /// Populate an existing [`GraphBuilder`] from this definition.
    ///
    /// The builder must already have node types registered in its
    /// registry before calling this method.
    /// Validates IDs, resources, and connections, then adds nodes
    /// and edges to the builder.
    pub fn populate<T: Transcendental, const B: usize>(
        &self,
        builder: &mut GraphBuilder<T, B>,
    ) -> Result<(), SerializationError> {
        builder.set_sample_rate(self.sample_rate);

        let mut seen = HashSet::new();
        for nd in &self.nodes {
            if !seen.insert(nd.id()) {
                return Err(SerializationError::DuplicateNodeId(nd.id()));
            }
        }

        if self.block_size != B {
            return Err(SerializationError::InvalidFormat(format!(
                "expected block_size={B}, document has block_size={}",
                self.block_size
            )));
        }

        for rd in &self.resources {
            builder.add_resource(crate::graph::GraphResource {
                name: rd.name.clone(),
                kind: rd.kind.clone(),
                capacity: rd.capacity,
            });
        }

        for nd in &self.nodes {
            let mut p = Params::new(self.sample_rate);
            for (k, v) in nd.parameters() {
                p = p.with(k.clone(), v.clone());
            }
            let idx = builder.add_node_with_id(nd.type_name(), &p, NodeId(nd.id()));

            if let NodeDef::Router(ref r) = nd {
                for entry in &r.routing_matrix {
                    builder.add_routing_entry(idx, entry.from, entry.to, entry.gain);
                }
            }
        }

        let id_to_idx: HashMap<u32, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id(), i))
            .collect();

        for conn in &self.connections {
            let from = *id_to_idx.get(&conn.from_node).ok_or_else(|| {
                SerializationError::InvalidFormat(format!(
                    "connection references unknown from_node {}",
                    conn.from_node
                ))
            })?;
            let to = *id_to_idx.get(&conn.to_node).ok_or_else(|| {
                SerializationError::InvalidFormat(format!(
                    "connection references unknown to_node {}",
                    conn.to_node
                ))
            })?;

            match conn.kind {
                SignalKind::Signal => {
                    builder.connect_signal(from, conn.from_port, to, conn.to_port);
                }
                SignalKind::Control => {
                    builder.connect_control(from, conn.from_port, to, conn.to_port);
                }
                SignalKind::Clock => {
                    builder.connect_clock(from, conn.from_port, to, conn.to_port);
                }
                SignalKind::Feedback => {
                    builder.connect_feedback(from, conn.from_port, to, conn.to_port);
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// Custom serde for parameters — accepts both plain and tagged formats
// ============================================================================

fn deserialize_params<'de, D>(deserializer: D) -> Result<HashMap<String, ParamValue>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: HashMap<String, serde_json::Value> = HashMap::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(k, v)| {
            json_to_param_value(v)
                .map(|pv| (k, pv))
                .map_err(de::Error::custom)
        })
        .collect()
}

fn json_to_param_value(v: serde_json::Value) -> Result<ParamValue, String> {
    match v {
        serde_json::Value::Number(n) => n
            .as_f64()
            .map(|f| ParamValue::Float(f as f32))
            .ok_or_else(|| "invalid number".to_string()),
        serde_json::Value::String(s) => Ok(ParamValue::String(s)),
        serde_json::Value::Bool(b) => Ok(ParamValue::Bool(b)),
        serde_json::Value::Object(obj) => {
            if let Some(val) = obj.get("Float").and_then(|v| v.as_f64()) {
                return Ok(ParamValue::Float(val as f32));
            }
            if let Some(val) = obj.get("Int").and_then(|v| v.as_i64()) {
                return Ok(ParamValue::Int(val as i32));
            }
            if let Some(val) = obj.get("Bool").and_then(|v| v.as_bool()) {
                return Ok(ParamValue::Bool(val));
            }
            if let Some(val) = obj.get("String").and_then(|v| v.as_str()) {
                return Ok(ParamValue::String(val.to_string()));
            }
            if let Some(val) = obj.get("Choice").and_then(|v| v.as_str()) {
                return Ok(ParamValue::Choice(val.to_string()));
            }
            if let Some(arr) = obj.get("Bytes").and_then(|v| v.as_array()) {
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect();
                return Ok(ParamValue::Bytes(bytes));
            }
            Err("unknown variant in tagged format".to_string())
        }
        serde_json::Value::Array(arr) => {
            let bytes: Vec<u8> = arr
                .iter()
                .filter_map(|v| v.as_u64().map(|n| n as u8))
                .collect();
            Ok(ParamValue::Bytes(bytes))
        }
        _ => Err("invalid param value type".to_string()),
    }
}

fn serialize_params<S>(
    params: &HashMap<String, ParamValue>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(params.len()))?;
    for (k, v) in params {
        let json_val = param_value_to_json(v);
        map.serialize_entry(k, &json_val)?;
    }
    map.end()
}

fn param_value_to_json(v: &ParamValue) -> serde_json::Value {
    match v {
        ParamValue::Float(f) => {
            serde_json::Value::Number(serde_json::Number::from_f64(*f as f64).unwrap_or(0.into()))
        }
        ParamValue::Int(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
        ParamValue::Bool(b) => serde_json::Value::Bool(*b),
        ParamValue::String(s) => serde_json::Value::String(s.clone()),
        ParamValue::Choice(s) => serde_json::Value::String(s.clone()),
        ParamValue::Bytes(b) => serde_json::Value::Array(
            b.iter()
                .map(|&x| serde_json::Value::Number(x.into()))
                .collect(),
        ),
        ParamValue::SignalSlab(_) => serde_json::Value::Null,
    }
}

// ============================================================================
// Convenience functions
// ============================================================================

/// Deserialise a graph from JSON.
pub fn from_json(json: &str) -> Result<GraphDef, SerializationError> {
    serde_json::from_str(json).map_err(|e| SerializationError::InvalidFormat(e.to_string()))
}

/// Deserialise a graph from CBOR binary.
pub fn from_cbor(bytes: &[u8]) -> Result<GraphDef, SerializationError> {
    serde_cbor::from_slice(bytes).map_err(|e| SerializationError::InvalidFormat(e.to_string()))
}
