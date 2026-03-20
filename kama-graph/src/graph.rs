//! Audio graph implementation with clock synchronization
//!
//! This module provides the main `AudioGraph` struct that manages
//! nodes, connections, and clock synchronization between the audio
//! world and the automaton world.

use kama_core::traits::{ProcessError as GraphError, ProcessResult as GraphResult};
use kama_core::prelude::*;
use kama_core::time::{ClockTick, SystemClock, ClockSource};
use kama_core::traits::ConnectionError;
use kama_core::traits::{NodeVariant, ProcessContext, ActivePort, Processable, AudioNode, Source, Processor, Sink};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

// ============================================================================
// Graph Statistics
// ============================================================================

/// Statistics for graph performance monitoring
#[derive(Debug, Clone, Copy, Default)]
pub struct GraphStats {
    /// Total blocks processed
    pub blocks_processed: u64,
    
    /// Commands successfully applied
    pub commands_applied: u64,
    
    /// Commands rejected (wrong type, etc)
    pub commands_rejected: u64,
    
    /// Maximum processing time for a single block (ns)
    pub max_process_time_ns: u64,
    
    /// Average processing time (ns)
    pub avg_process_time_ns: f64,
    
    /// Micro-control violations detected
    pub violations: u64,
}

// ============================================================================
// Connection Type
// ============================================================================

/// A connection between two ports
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// Source port
    pub from: PortId,
    /// Destination port
    pub to: PortId,
}

// ============================================================================
// Node Storage
// ============================================================================

/// Storage for a node with its connections
struct NodeEntry<T: AudioNum, const BUF_SIZE: usize> {
    /// The node itself, wrapped in a NodeVariant
    node: NodeVariant<T, BUF_SIZE>,
    
    /// Output connections: (output_port, target_node, target_port)
    audio_outputs: Vec<(u16, NodeId, u16)>,
    
    /// Control output connections
    control_outputs: Vec<(u16, NodeId, u16)>,
    
    /// Clock output connections
    clock_outputs: Vec<(u16, NodeId, u16)>,
    
    /// Whether this node has been processed in current tick
    processed: bool,
}

// ============================================================================
// Graph Builder (Mutable Construction)
// ============================================================================

/// Mutable builder for an immutable audio graph.
///
/// The builder allows adding nodes and connections. Once the graph is fully
/// constructed, call `build()` to obtain an immutable `AudioGraph`.
pub struct GraphBuilder<T: AudioNum, const BUF_SIZE: usize> {
    /// Nodes added so far
    nodes: HashMap<NodeId, NodeEntry<T, BUF_SIZE>>,
    
    /// Next available node ID
    next_id: u32,
    
    /// Connections added so far
    connections: Vec<Connection>,
}

impl<T: AudioNum, const BUF_SIZE: usize> GraphBuilder<T, BUF_SIZE> {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            next_id: 0,
            connections: Vec::new(),
        }
    }
    
    /// Add a source node to the graph.
    pub fn add_source(&mut self, source: Box<dyn Source<T, BUF_SIZE>>) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        
        let entry = NodeEntry {
            node: NodeVariant::Source(source),
            audio_outputs: Vec::new(),
            control_outputs: Vec::new(),
            clock_outputs: Vec::new(),
            processed: false,
        };
        
        self.nodes.insert(id, entry);
        id
    }
    
    /// Add a processor node to the graph.
    pub fn add_processor(&mut self, processor: Box<dyn Processor<T, BUF_SIZE>>) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        
        let entry = NodeEntry {
            node: NodeVariant::Processor(processor),
            audio_outputs: Vec::new(),
            control_outputs: Vec::new(),
            clock_outputs: Vec::new(),
            processed: false,
        };
        
        self.nodes.insert(id, entry);
        id
    }
    
    /// Add a sink node to the graph.
    pub fn add_sink(&mut self, sink: Box<dyn Sink<T, BUF_SIZE>>) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        
        let entry = NodeEntry {
            node: NodeVariant::Sink(sink),
            audio_outputs: Vec::new(),
            control_outputs: Vec::new(),
            clock_outputs: Vec::new(),
            processed: false,
        };
        
        self.nodes.insert(id, entry);
        id
    }
    
    /// Connect audio output to audio input.
    pub fn connect_audio(
        &mut self,
        from: NodeId,
        from_port: u16,
        to: NodeId,
        to_port: u16,
    ) -> GraphResult<()> {
        self.validate_connection(from, to)?;
        
        if let Some(entry) = self.nodes.get_mut(&from) {
            entry.audio_outputs.push((from_port, to, to_port));
            self.connections.push(Connection {
                from: PortId::audio_out(from, from_port),
                to: PortId::audio_in(to, to_port),
            });
            Ok(())
        } else {
            Err(GraphError::NodeNotFound(from.0))
        }
    }
    
    /// Connect control output to control input.
    pub fn connect_control(
        &mut self,
        from: NodeId,
        from_port: u16,
        to: NodeId,
        to_port: u16,
    ) -> GraphResult<()> {
        self.validate_connection(from, to)?;
        
        if let Some(entry) = self.nodes.get_mut(&from) {
            entry.control_outputs.push((from_port, to, to_port));
            self.connections.push(Connection {
                from: PortId::control_out(from, from_port),
                to: PortId::control_in(to, to_port),
            });
            Ok(())
        } else {
            Err(GraphError::NodeNotFound(from.0))
        }
    }
    
    /// Connect clock output to clock input.
    pub fn connect_clock(
        &mut self,
        from: NodeId,
        from_port: u16,
        to: NodeId,
        to_port: u16,
    ) -> GraphResult<()> {
        self.validate_connection(from, to)?;
        
        if let Some(entry) = self.nodes.get_mut(&from) {
            entry.clock_outputs.push((from_port, to, to_port));
            self.connections.push(Connection {
                from: PortId::clock_out(from, from_port),
                to: PortId::clock_in(to, to_port),
            });
            Ok(())
        } else {
            Err(GraphError::NodeNotFound(from.0))
        }
    }
    
    /// Validate that a connection is possible.
    fn validate_connection(&self, from: NodeId, to: NodeId) -> GraphResult<()> {
        if !self.nodes.contains_key(&from) {
            return Err(GraphError::NodeNotFound(from.0));
        }
        if !self.nodes.contains_key(&to) {
            return Err(GraphError::NodeNotFound(to.0));
        }
        Ok(())
    }
    
    /// Build the immutable audio graph.
    pub fn build(self, clock_source: Box<dyn ClockSource>) -> AudioGraph<T, BUF_SIZE> {
        // Precompute topological order and reachable subgraph
        let (topological_order, reachable) = self.compute_processing_order();
        let sample_rate = clock_source.sample_rate();
        
        AudioGraph {
            nodes: self.nodes,
            connections: self.connections,
            topological_order,
            reachable,
            clock_source,
            current_tick: ClockTick::new(0, BUF_SIZE as u32, sample_rate),
            stats: GraphStats::default(),
            last_process_time: Instant::now(),
        }
    }
    
    /// Compute topological order of nodes based on audio and control connections.
    /// Returns (order, reachable_map).
    fn compute_processing_order(&self) -> (Vec<NodeId>, HashMap<NodeId, HashSet<NodeId>>) {
        // TODO: implement Kahn's algorithm and compute reachable subgraph
        // For now, return nodes in insertion order and empty reachable map.
        let order: Vec<NodeId> = self.nodes.keys().copied().collect();
        let reachable = HashMap::new();
        (order, reachable)
    }
}

// ============================================================================
// Audio Graph (Immutable Processing)
// ============================================================================

/// Immutable audio graph with precomputed topology.
///
/// Once built, the graph cannot be modified. Processing is done via the
/// `process_block` method, which iterates over nodes in topological order.
pub struct AudioGraph<T: AudioNum, const BUF_SIZE: usize> {
    /// Nodes in the graph
    nodes: HashMap<NodeId, NodeEntry<T, BUF_SIZE>>,
    
    /// All connections
    connections: Vec<Connection>,
    
    /// Topological order of nodes for processing
    topological_order: Vec<NodeId>,
    
    /// Reachable subgraph mapping (node -> set of reachable nodes)
    reachable: HashMap<NodeId, HashSet<NodeId>>,
    
    /// Clock source for timing
    clock_source: Box<dyn ClockSource>,
    
    /// Current clock tick
    current_tick: ClockTick,
    
    /// Statistics
    stats: GraphStats,
    
    /// Last process time for statistics
    last_process_time: Instant,
}

impl<T: AudioNum, const BUF_SIZE: usize> AudioGraph<T, BUF_SIZE> {
    /// Create a new graph with the given clock source using a builder.
    pub fn new(clock_source: Box<dyn ClockSource>) -> Self {
        GraphBuilder::new().build(clock_source)
    }
    
    /// Create a new graph with a system clock.
    pub fn with_sample_rate(sample_rate: f32) -> Self {
        Self::new(Box::new(SystemClock::with_sample_rate(sample_rate)))
    }
    
    /// Process one block of audio.
    ///
    /// This method iterates over nodes in topological order, gathers input
    /// buffers from connections, calls `process_block` on each node, and
    /// routes outputs to downstream nodes.
    pub fn process_block(&mut self) -> GraphResult<()> {
        let start = Instant::now();
        
        // Advance clock
        self.current_tick = self.clock_source.next_tick(BUF_SIZE);
        
        // Reset processed flags
        for entry in self.nodes.values_mut() {
            entry.processed = false;
        }
        
        // Process nodes in topological order
        let order = self.topological_order.clone();
        for &node_id in &order {
            self.process_node(node_id)?;
        }
        
        // Update statistics
        let elapsed = start.elapsed();
        let elapsed_ns = elapsed.as_nanos() as u64;
        
        self.stats.blocks_processed += 1;
        self.stats.max_process_time_ns = self.stats.max_process_time_ns.max(elapsed_ns);
        self.stats.avg_process_time_ns = self.stats.avg_process_time_ns * 0.95 + elapsed_ns as f64 * 0.05;
        
        Ok(())
    }
    
    /// Process a single node and its dependencies.
    fn process_node(&mut self, node_id: NodeId) -> GraphResult<()> {
        let entry = self.nodes.get_mut(&node_id)
            .ok_or(GraphError::NodeNotFound(node_id.0))?;
        
        if entry.processed {
            return Ok(());
        }
        
        // Prepare buffers for this node
        // TODO: gather inputs from connections using ActivePort / PipeBuffer
        // For now, create empty buffers.
        let mut audio_inputs = Vec::new();
        let mut control_inputs = Vec::new();
        let mut clock_inputs = vec![self.current_tick; 0]; // placeholder
        let mut feedback_inputs = Vec::new();
        
        let mut audio_outputs = vec![[T::ZERO; BUF_SIZE]; 0]; // placeholder
        let mut audio_output_refs: Vec<&mut [T; BUF_SIZE]> = audio_outputs.iter_mut().collect();
        let mut control_outputs = Vec::new();
        let mut clock_outputs = Vec::new();
        let mut feedback_outputs = Vec::new();
        let mut feedback_output_refs: Vec<&mut [T; BUF_SIZE]> = feedback_outputs.iter_mut().collect();
        
        // Build ProcessContext
        let mut ctx = ProcessContext {
            clock: &self.current_tick,
            audio_inputs: &audio_inputs,
            control_inputs: &control_inputs,
            clock_inputs: &clock_inputs,
            feedback_inputs: &feedback_inputs,
            audio_outputs: &mut audio_output_refs,
            control_outputs: &mut control_outputs,
            clock_outputs: &mut clock_outputs,
            feedback_outputs: &mut feedback_output_refs,
        };
        
        // Process the node via Processable trait
        entry.node.process_block(&mut ctx)?;
        
        entry.processed = true;
        
        // TODO: route outputs to connected nodes
        
        Ok(())
    }
    
    /// Get current statistics.
    pub fn stats(&self) -> GraphStats {
        self.stats
    }
    
    /// Get current clock tick.
    pub fn current_tick(&self) -> ClockTick {
        self.current_tick
    }
    
    /// Check if node exists.
    pub fn contains_node(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }
    
    /// Get number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    
    /// Returns a topological order of node IDs based on audio and control connections.
    /// Clock connections are ignored for processing order.
    /// Returns an error if a cycle is detected.
    pub fn processing_order(&self) -> GraphResult<Vec<NodeId>> {
        // Return precomputed order
        Ok(self.topological_order.clone())
    }
    
    /// Returns a slice of all connections in the graph.
    pub fn connections(&self) -> Vec<Connection> {
        self.connections.clone()
    }
    
    /// Validate that a connection is possible between two ports.
    pub fn validate_connection(&self, from: PortId, to: PortId) -> Result<(), ConnectionError> {
        // TODO: implement validation logic
        // For now, just return Ok(())
        Ok(())
    }
    
    /// Advance the clock source and return the new tick.
    pub fn advance_clock(&mut self) -> ClockTick {
        self.current_tick = self.clock_source.next_tick(BUF_SIZE);
        self.current_tick
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_graph_creation() {
        let graph = AudioGraph::<f32, 64>::with_sample_rate(44100.0);
        assert_eq!(graph.node_count(), 0);
    }
    
    #[test]
    fn test_builder_add_nodes() {
        // Temporarily disabled due to missing kama_core_dsp dependency
    }
}