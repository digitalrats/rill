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
//! Both encode the same [`GraphDocument`] structure.

use std::collections::{HashMap, HashSet};

use rill_core::math::Transcendental;
use rill_core::traits::{SignalNode, NodeId, NodeParams, NodeVariant, ParamValue};
use rill_core::ParameterId;

use crate::graph::{GraphBuilder, NodeEntry};
use crate::registry::{NodeRegistry, RegistryError};

// Re-export serde unconditionally — the whole module is feature-gated.
use serde::{Deserialize, Serialize};

// ============================================================================
// Document structure
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
pub struct GraphDocument {
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

    /// Optional human-readable description (attribution, preset notes, …).
    /// Not interpreted by the engine; preserved through serialisation round-trips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A single node in the serialised graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDef {
    /// Unique node identifier (must match what `rill-patchbay` uses).
    pub id: u32,

    /// Canonical type name for factory lookup (e.g. `"rill/sine_osc"`).
    pub type_name: String,

    /// Human-readable instance name.
    pub name: String,

    /// Runtime parameters (frequency, gain, …).
    pub parameters: HashMap<String, ParamValue>,
}

/// A connection between two ports.
///
/// Nodes are identified by their [`NodeDef::id`] (not by position in the
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
    Signal,
    Control,
    Clock,
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
    /// The inner registry error.
    Registry(RegistryError),
}

impl From<RegistryError> for SerializationError {
    fn from(e: RegistryError) -> Self {
        Self::Registry(e)
    }
}

impl std::fmt::Display for SerializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownType(t) => write!(f, "unknown node type: {t}"),
            Self::DuplicateNodeId(id) => write!(f, "duplicate NodeId: {id}"),
            Self::InvalidFormat(d) => write!(f, "invalid format: {d}"),
            Self::Registry(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SerializationError {}

// ============================================================================
// Construction helpers (for incremental / interactive graph building)
// ============================================================================

impl GraphDocument {
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
    /// Returns an error if the [`NodeDef::id`] duplicates an existing one.
    pub fn add_node(&mut self, def: NodeDef) -> Result<(), SerializationError> {
        if self.nodes.iter().any(|n| n.id == def.id) {
            return Err(SerializationError::DuplicateNodeId(def.id));
        }
        self.nodes.push(def);
        Ok(())
    }

    /// Append a connection.
    ///
    /// Validity of the node IDs is checked only at [`into_builder`] time.
    pub fn add_connection(&mut self, conn: ConnectionDef) {
        self.connections.push(conn);
    }

    /// Set a parameter value on an existing node (identified by [`NodeDef::id`]).
    pub fn set_node_param(&mut self, node_id: u32, key: &str, value: ParamValue) {
        if let Some(nd) = self.nodes.iter_mut().find(|n| n.id == node_id) {
            nd.parameters.insert(key.to_string(), value);
        }
    }

    /// Remove all nodes and connections.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.connections.clear();
    }
}

// ============================================================================
// Export (SignalGraph → GraphDocument)
// ============================================================================

impl GraphDocument {
    /// Build a document from an in-memory graph.
    ///
    /// Iterates every node, reads its metadata and current parameters,
    /// and reconstructs all connections from port routing state.
    pub fn from_graph<T: Transcendental, const B: usize>(
        graph: &super::SignalGraph<T, B>,
    ) -> Self {
        let entries = graph.node_entries();
        let sample_rate = graph.sample_rate();

        let nodes: Vec<NodeDef> = entries.iter().map(|entry| node_to_def(&entry.node)).collect();
        let connections = extract_connections(entries);

        let resources = graph
            .resources()
            .iter()
            .map(|r| ResourceDef {
                name: r.name.clone(),
                kind: r.kind.clone(),
                capacity: r.capacity,
            })
            .collect();

        Self {
            format_version: "rill/1".to_string(),
            sample_rate,
            block_size: B as usize,
            resources,
            nodes,
            connections,
            description: None,
        }
    }
}

fn node_to_def<T: Transcendental, const B: usize>(
    variant: &NodeVariant<T, B>,
) -> NodeDef {
    let meta = variant.metadata();
    let type_name = meta
        .type_name
        .clone()
        .unwrap_or_else(|| meta.name.clone());

    let mut parameters = HashMap::new();
    for pm in &meta.parameters {
        let pid = match ParameterId::new(&pm.name) {
            Ok(id) => id,
            Err(_) => continue,
        };
        if let Some(val) = variant.get_parameter(&pid) {
            parameters.insert(pm.name.clone(), val);
        }
    }

    NodeDef {
        id: variant.id().inner(),
        type_name,
        name: meta.name.clone(),
        parameters,
    }
}

fn extract_connections<T: Transcendental, const B: usize>(
    entries: &[NodeEntry<T, B>],
) -> Vec<ConnectionDef> {
    let mut conns = Vec::new();

    for (from_idx, entry) in entries.iter().enumerate() {
        let variant = &entry.node;
        let from_id = variant.id().inner();

        let audio_outs = variant.metadata().signal_outputs;
        for from_port in 0..audio_outs {
            if let Some(port) = variant.output_port(from_port) {
                for &(to_idx, to_port) in &port.downstream {
                    let to_id = entries[to_idx].node.id().inner();
                    conns.push(ConnectionDef {
                        kind: SignalKind::Signal,
                        from_node: from_id,
                        from_port,
                        to_node: to_id,
                        to_port,
                    });
                }
                for &(to_idx, to_port) in &port.feedback_downstream {
                    let to_id = entries[to_idx].node.id().inner();
                    conns.push(ConnectionDef {
                        kind: SignalKind::Feedback,
                        from_node: from_id,
                        from_port,
                        to_node: to_id,
                        to_port,
                    });
                }
            }
        }
    }

    conns
}

// ============================================================================
// Import (GraphDocument → GraphBuilder)
// ============================================================================

impl GraphDocument {
    /// Reconstitute a mutable graph builder from this document.
    ///
    /// Validates that all node types are registered and no [`NodeId`] is
    /// duplicated, then constructs every node and wires every connection.
    pub fn into_builder<T: Transcendental, const B: usize>(
        self,
        registry: &NodeRegistry<T, B>,
    ) -> Result<GraphBuilder<T, B>, SerializationError> {
        // ── validate IDs ──
        let mut seen = HashSet::new();
        for nd in &self.nodes {
            if !seen.insert(nd.id) {
                return Err(SerializationError::DuplicateNodeId(nd.id));
            }
        }

        // ── validate sample rate / block size ──
        if self.block_size != B {
            return Err(SerializationError::InvalidFormat(format!(
                "expected block_size={B}, document has block_size={}",
                self.block_size
            )));
        }

        let mut builder = GraphBuilder::new();

        // ── register resources ──
        for rd in &self.resources {
            builder.add_resource(crate::graph::GraphResource {
                name: rd.name.clone(),
                kind: rd.kind.clone(),
                capacity: rd.capacity,
            });
        }

        // ── construct nodes ──
        for nd in &self.nodes {
            let mut p = NodeParams::new(self.sample_rate);
            for (k, v) in &nd.parameters {
                p = p.with(k.clone(), v.clone());
            }
            builder.add_node_with_id(registry, &nd.type_name, &p, NodeId(nd.id))?;
        }

        // ── build NodeId → index map ──
        let id_to_idx: HashMap<u32, usize> =
            self.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();

        // ── wire connections ──
        for conn in &self.connections {
            let from = *id_to_idx
                .get(&conn.from_node)
                .ok_or_else(|| SerializationError::InvalidFormat(
                    format!("connection references unknown from_node {}", conn.from_node),
                ))?;
            let to = *id_to_idx
                .get(&conn.to_node)
                .ok_or_else(|| SerializationError::InvalidFormat(
                    format!("connection references unknown to_node {}", conn.to_node),
                ))?;

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

        Ok(builder)
    }
}

// ============================================================================
// Convenience functions
// ============================================================================

/// Serialise a graph to pretty-printed JSON.
pub fn to_json<T: Transcendental, const B: usize>(
    graph: &super::SignalGraph<T, B>,
) -> Result<String, SerializationError> {
    let doc = GraphDocument::from_graph(graph);
    serde_json::to_string_pretty(&doc)
        .map_err(|e| SerializationError::InvalidFormat(e.to_string()))
}

/// Deserialise a graph from JSON.
pub fn from_json<T: Transcendental, const B: usize>(
    json: &str,
    registry: &NodeRegistry<T, B>,
) -> Result<GraphBuilder<T, B>, SerializationError> {
    let doc: GraphDocument = serde_json::from_str(json)
        .map_err(|e| SerializationError::InvalidFormat(e.to_string()))?;
    doc.into_builder(registry)
}

/// Serialise a graph to CBOR binary.
pub fn to_cbor<T: Transcendental, const B: usize>(
    graph: &super::SignalGraph<T, B>,
) -> Result<Vec<u8>, SerializationError> {
    let doc = GraphDocument::from_graph(graph);
    serde_cbor::to_vec(&doc)
        .map_err(|e| SerializationError::InvalidFormat(e.to_string()))
}

/// Deserialise a graph from CBOR binary.
pub fn from_cbor<T: Transcendental, const B: usize>(
    bytes: &[u8],
    registry: &NodeRegistry<T, B>,
) -> Result<GraphBuilder<T, B>, SerializationError> {
    let doc: GraphDocument = serde_cbor::from_slice(bytes)
        .map_err(|e| SerializationError::InvalidFormat(e.to_string()))?;
    doc.into_builder(registry)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::SignalGraph;
    use crate::registry::NodeConstructor;
    use rill_core::buffer::FixedBuffer;
    use rill_core::math::Transcendental;
    use rill_core::time::ClockTick;
    use rill_core::traits::node::NodeState;
    use rill_core::traits::port::Port;
    use rill_core::traits::{
        NodeCategory, ParamMetadata, ParamType, ParamValue as PV, ParameterId, ProcessResult,
        Processor, Source,
    };
    use rill_core::ParamMetadata as PM;

    // ==================================================================
    // Test node — configurable metadata, parameters, feedback ports
    // ==================================================================

    struct TestNode<T: Transcendental, const B: usize> {
        id: NodeId,
        state: NodeState<T, B>,
        output: Port<T, B>,
        type_name: Option<String>,
        cat: NodeCategory,
        param_defs: Vec<PM>,
        params: HashMap<String, f32>,
        has_feedback: bool,
    }

    impl<T: Transcendental, const B: usize> TestNode<T, B> {
        fn new_raw(cat: NodeCategory) -> Self {
            Self {
                id: NodeId(0),
                state: NodeState::new(44100.0),
                output: Port::output(NodeId(0), 0, "out"),
                type_name: None,
                cat,
                param_defs: vec![],
                params: HashMap::new(),
                has_feedback: false,
            }
        }

        fn source() -> Self {
            Self::new_raw(NodeCategory::Source)
        }

        fn processor() -> Self {
            let mut s = Self::new_raw(NodeCategory::Processor);
            s.has_feedback = true;
            s
        }

        fn with_type_name(mut self, tn: &str) -> Self {
            self.type_name = Some(tn.to_string());
            self
        }

        fn with_param(mut self, name: &str, default: f32) -> Self {
            self.param_defs.push(PM::new(name, ParamType::Float, PV::Float(default)));
            self.params.insert(name.to_string(), default);
            self
        }
    }

    impl<T: Transcendental, const B: usize> SignalNode<T, B> for TestNode<T, B> {
        fn metadata(&self) -> rill_core::traits::NodeMetadata {
            NodeMetadata {
                name: "TestNode".to_string(),
                type_name: self.type_name.clone(),
                category: self.cat,
                description: String::new(),
                author: String::new(),
                version: String::new(),
                signal_inputs: if self.cat == NodeCategory::Source { 0 } else { 1 },
                signal_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: if self.has_feedback { 1 } else { 0 },
                parameters: self.param_defs.clone(),
            }
        }
        fn init(&mut self, _: f32) {}
        fn reset(&mut self) {}
        fn get_parameter(&self, id: &ParameterId) -> Option<PV> {
            self.params.get(id.as_str()).map(|&v| PV::Float(v))
        }
        fn set_parameter(&mut self, id: &ParameterId, value: PV) -> ProcessResult<()> {
            if let Some(f) = value.as_f32() {
                self.params.insert(id.as_str().to_string(), f);
                Ok(())
            } else {
                Err(rill_core::ProcessError::parameter("type mismatch"))
            }
        }
        fn id(&self) -> NodeId { self.id }
        fn set_id(&mut self, id: NodeId) { self.id = id; }
        fn input_port(&self, _: usize) -> Option<&Port<T, B>> { None }
        fn input_port_mut(&mut self, _: usize) -> Option<&mut Port<T, B>> { None }
        fn output_port(&self, index: usize) -> Option<&Port<T, B>> {
            if index == 0 { Some(&self.output) } else { None }
        }
        fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, B>> {
            if index == 0 { Some(&mut self.output) } else { None }
        }
        fn control_port(&self, _: usize) -> Option<&Port<T, B>> { None }
        fn control_port_mut(&mut self, _: usize) -> Option<&mut Port<T, B>> { None }
        fn state(&self) -> &NodeState<T, B> { &self.state }
        fn state_mut(&mut self) -> &mut NodeState<T, B> { &mut self.state }
    }

    impl<T: Transcendental, const B: usize> Source<T, B> for TestNode<T, B> {
        fn generate(&mut self, _: &ClockTick, _: &[T], _: &[ClockTick]) -> ProcessResult<()> { Ok(()) }
    }
    impl<T: Transcendental, const B: usize> Processor<T, B> for TestNode<T, B> {
        fn process(&mut self, _: &ClockTick, _: &[&[T; B]], _: &[T], _: &[ClockTick], _: &[&[T; B]]) -> ProcessResult<()> { Ok(()) }
        fn latency(&self) -> usize { 0 }
    }

    struct TestCtor;
    impl<T: Transcendental, const B: usize> NodeConstructor<T, B> for TestCtor {
        fn type_name(&self) -> &'static str { "rill/test" }
        fn construct(&self, id: NodeId, params: &NodeParams) -> NodeVariant<T, B> {
            let mut node = TestNode::<T, B>::source()
                .with_type_name("rill/test");
            node.set_id(id);
            node.init(params.sample_rate);
            NodeVariant::Source(Box::new(node))
        }
    }

    struct ParamCtor;
    impl<T: Transcendental, const B: usize> NodeConstructor<T, B> for ParamCtor {
        fn type_name(&self) -> &'static str { "rill/param" }
        fn construct(&self, id: NodeId, params: &NodeParams) -> NodeVariant<T, B> {
            let mut node = TestNode::<T, B>::processor()
                .with_type_name("rill/param")
                .with_param("frequency", 440.0)
                .with_param("amplitude", 0.5);
            if let Some(f) = params.get("frequency").and_then(|v| v.as_f32()) {
                node.params.insert("frequency".into(), f);
            }
            if let Some(a) = params.get("amplitude").and_then(|v| v.as_f32()) {
                node.params.insert("amplitude".into(), a);
            }
            node.set_id(id);
            node.init(params.sample_rate);
            NodeVariant::Processor(Box::new(node))
        }
    }

    // ── Helpers ────────────────────────────────────────────────────

    fn empty_registry() -> NodeRegistry<f32, 64> {
        let mut r = NodeRegistry::<f32, 64>::new();
        r.register(TestCtor);
        r.register(ParamCtor);
        r
    }

    fn build_small_graph(registry: &NodeRegistry<f32, 64>) -> SignalGraph<f32, 64> {
        let mut b = GraphBuilder::new();
        let src = b.add_node(registry, "rill/test", &NodeParams::new(44100.0)).unwrap();
        let proc = b.add_node(registry, "rill/test", &NodeParams::new(44100.0)).unwrap();
        b.connect_signal(src, 0, proc, 0);
        b.build(Box::new(rill_core::time::SystemClock::with_sample_rate(44100.0)))
            .expect("build")
    }

    // ==================================================================
    // Roundtrip
    // ==================================================================

    #[test]
    fn test_json_roundtrip() {
        let reg = empty_registry();
        let graph = build_small_graph(&reg);

        let json = to_json(&graph).expect("to_json");
        assert!(json.contains("rill/test"));
        assert!(json.contains("format_version"));
        assert!(json.contains("connections"));

        let restored = from_json(&json, &reg).expect("from_json");
        assert_eq!(restored.node_count(), 2);

        // Must rebuild without errors
        restored
            .build(Box::new(rill_core::time::SystemClock::with_sample_rate(44100.0)))
            .expect("rebuild");
    }

    #[test]
    fn test_cbor_roundtrip() {
        let reg = empty_registry();
        let graph = build_small_graph(&reg);

        let cbor = to_cbor(&graph).expect("to_cbor");
        assert!(!cbor.is_empty());

        let restored = from_cbor(&cbor, &reg).expect("from_cbor");
        assert_eq!(restored.node_count(), 2);
    }

    #[test]
    fn test_empty_graph_roundtrip() {
        let reg = empty_registry();
        let graph = SignalGraph::<f32, 64>::with_sample_rate(44100.0);

        let json = to_json(&graph).expect("to_json");
        assert!(json.contains(r#""nodes": []"#));
        assert!(json.contains(r#""connections": []"#));

        let restored = from_json(&json, &reg).expect("from_json");
        assert_eq!(restored.node_count(), 0);
    }

    // ==================================================================
    // Parameter export
    // ==================================================================

    #[test]
    fn test_export_parameters() {
        let reg = empty_registry();
        let mut b = GraphBuilder::new();
        b.add_node(
            &reg,
            "rill/param",
            &NodeParams::new(44100.0)
                .with("frequency", PV::Float(220.0))
                .with("amplitude", PV::Float(0.8)),
        )
        .unwrap();
        let graph = b
            .build(Box::new(rill_core::time::SystemClock::with_sample_rate(44100.0)))
            .expect("build");

        let doc = GraphDocument::from_graph(&graph);
        assert_eq!(doc.nodes.len(), 1);

        let nd = &doc.nodes[0];
        assert_eq!(nd.type_name, "rill/param");
        assert_eq!(
            nd.parameters.get("frequency"),
            Some(&PV::Float(220.0))
        );
        assert_eq!(
            nd.parameters.get("amplitude"),
            Some(&PV::Float(0.8))
        );
    }

    #[test]
    fn test_roundtrip_parameters() {
        let reg = empty_registry();
        let mut b = GraphBuilder::new();
        b.add_node(
            &reg,
            "rill/param",
            &NodeParams::new(48000.0)
                .with("frequency", PV::Float(55.0))
                .with("amplitude", PV::Float(0.25)),
        )
        .unwrap();
        let graph = b
            .build(Box::new(rill_core::time::SystemClock::with_sample_rate(48000.0)))
            .expect("build");

        let json = to_json(&graph).expect("to_json");
        let restored = from_json(&json, &reg).expect("from_json");
        assert_eq!(restored.node_count(), 1);
        // Rebuild — should not error
        restored
            .build(Box::new(rill_core::time::SystemClock::with_sample_rate(48000.0)))
            .expect("rebuild");
    }

    // ==================================================================
    // Feedback export
    // ==================================================================

    #[test]
    fn test_export_feedback_connection() {
        let reg = empty_registry();
        let mut b = GraphBuilder::new();
        let src = b.add_node(&reg, "rill/test", &NodeParams::new(44100.0)).unwrap();
        let proc = b.add_node(&reg, "rill/test", &NodeParams::new(44100.0)).unwrap();
        b.connect_signal(src, 0, proc, 0);
        b.connect_feedback(proc, 0, src, 0);
        let graph = b
            .build(Box::new(rill_core::time::SystemClock::with_sample_rate(44100.0)))
            .expect("build");

        let doc = GraphDocument::from_graph(&graph);
        let sigs: Vec<SignalKind> = doc.connections.iter().map(|c| c.kind).collect();
        assert!(sigs.contains(&SignalKind::Signal));
        assert!(sigs.contains(&SignalKind::Feedback));
        assert_eq!(doc.connections.len(), 2);
    }

    // ==================================================================
    // Type name
    // ==================================================================

    #[test]
    fn test_export_type_name_explicit() {
        // ParamCtor declares type_name = Some("rill/param")
        let reg = empty_registry();
        let mut b = GraphBuilder::new();
        b.add_node(&reg, "rill/param", &NodeParams::new(44100.0)).unwrap();
        let graph = b
            .build(Box::new(rill_core::time::SystemClock::with_sample_rate(44100.0)))
            .expect("build");

        let doc = GraphDocument::from_graph(&graph);
        assert_eq!(doc.nodes[0].type_name, "rill/param");
    }

    #[test]
    fn test_export_type_name_fallback_to_name() {
        // Node with type_name: None → doc uses metadata().name
        let mut reg = empty_registry();
        let mut b = GraphBuilder::new();
        // Register a name-only constructor for testing fallback
        struct FallbackCtor;
        impl<T: Transcendental, const B: usize> NodeConstructor<T, B> for FallbackCtor {
            fn type_name(&self) -> &'static str { "rill/fallback" }
            fn construct(&self, id: NodeId, params: &NodeParams) -> NodeVariant<T, B> {
                // No with_type_name → type_name stays None
                let mut node = TestNode::<T, B>::source();
                node.set_id(id);
                node.init(params.sample_rate);
                NodeVariant::Source(Box::new(node))
            }
        }
        reg.register(FallbackCtor);

        b.add_node(&reg, "rill/fallback", &NodeParams::new(44100.0)).unwrap();
        let graph = b
            .build(Box::new(rill_core::time::SystemClock::with_sample_rate(44100.0)))
            .expect("build");

        let doc = GraphDocument::from_graph(&graph);
        assert_eq!(doc.nodes[0].type_name, "TestNode");
    }

    // ==================================================================
    // Node IDs
    // ==================================================================

    #[test]
    fn test_roundtrip_preserves_node_ids() {
        let reg = empty_registry();
        let mut b = GraphBuilder::new();
        // Explicit IDs via add_node_with_id
        b.add_node_with_id(&reg, "rill/test", &NodeParams::new(44100.0), NodeId(100))
            .unwrap();
        b.add_node_with_id(&reg, "rill/param", &NodeParams::new(44100.0), NodeId(200))
            .unwrap();
        b.connect_signal(0, 0, 1, 0);
        let graph = b
            .build(Box::new(rill_core::time::SystemClock::with_sample_rate(44100.0)))
            .expect("build");

        let json = to_json(&graph).expect("to_json");
        assert!(json.contains(r#""id": 100"#));
        assert!(json.contains(r#""id": 200"#));

        let restored = from_json(&json, &reg).expect("from_json");
        let rebuilt = restored
            .build(Box::new(rill_core::time::SystemClock::with_sample_rate(44100.0)))
            .expect("rebuild");
        assert_eq!(rebuilt.node_count(), 2);
    }

    // ==================================================================
    // Complex topology
    // ==================================================================

    #[test]
    fn test_roundtrip_complex_topology() {
        let reg = empty_registry();
        let mut b = GraphBuilder::new();
        let s0 = b.add_node(&reg, "rill/test", &NodeParams::new(44100.0)).unwrap();
        let p1 = b.add_node(&reg, "rill/param", &NodeParams::new(44100.0)).unwrap();
        let p2 = b.add_node(&reg, "rill/param", &NodeParams::new(44100.0)).unwrap();
        b.connect_signal(s0, 0, p1, 0);
        b.connect_signal(p1, 0, p2, 0);

        let graph = b
            .build(Box::new(rill_core::time::SystemClock::with_sample_rate(44100.0)))
            .expect("build");

        let json = to_json(&graph).expect("to_json");
        let mut restored = from_json(&json, &reg).expect("from_json");
        assert_eq!(restored.node_count(), 3);

        // Verify connections
        let rebuilt = restored
            .build(Box::new(rill_core::time::SystemClock::with_sample_rate(44100.0)))
            .expect("rebuild");

        // Topological order: source must be first
        assert_eq!(rebuilt.topo_order().len(), 3);
    }

    // ==================================================================
    // Error cases
    // ==================================================================

    #[test]
    fn test_unknown_type_error() {
        let reg = empty_registry();
        let doc = GraphDocument {
            format_version: "rill/1".to_string(),
            sample_rate: 44100.0,
            block_size: 64,
            resources: vec![],
            nodes: vec![NodeDef {
                id: 0,
                type_name: "rill/nonexistent".to_string(),
                name: "x".to_string(),
                parameters: HashMap::new(),
            }],
            connections: vec![],
            description: None,
        };
        let result = doc.into_builder(&reg);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_id_error() {
        let reg = empty_registry();
        let doc = GraphDocument {
            format_version: "rill/1".to_string(),
            sample_rate: 44100.0,
            block_size: 64,
            resources: vec![],
            nodes: vec![
                NodeDef {
                    id: 0,
                    type_name: "rill/test".to_string(),
                    name: "a".to_string(),
                    parameters: HashMap::new(),
                },
                NodeDef {
                    id: 0,
                    type_name: "rill/test".to_string(),
                    name: "b".to_string(),
                    parameters: HashMap::new(),
                },
            ],
            connections: vec![],
            description: None,
        };
        match doc.into_builder(&reg) {
            Err(SerializationError::DuplicateNodeId(id)) => assert_eq!(id, 0),
            _ => panic!("expected DuplicateNodeId"),
        }
    }

    #[test]
    fn test_block_size_mismatch() {
        let doc = GraphDocument {
            format_version: "rill/1".to_string(),
            sample_rate: 44100.0,
            block_size: 128,
            resources: vec![],
            nodes: vec![],
            connections: vec![],
            description: None,
        };
        let r = NodeRegistry::<f32, 256>::new();
        match doc.into_builder(&r) {
            Err(SerializationError::InvalidFormat(_)) => {}
            _ => panic!("expected InvalidFormat"),
        }
    }

    #[test]
    fn test_invalid_json() {
        let reg = empty_registry();
        assert!(from_json::<f32, 64>("not json", &reg).is_err());
    }
}
