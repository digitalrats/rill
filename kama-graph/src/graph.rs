//! Audio graph implementation with clock synchronization
//!
//! This module provides the main `AudioGraph` struct that manages
//! nodes, connections, and clock synchronization between the audio
//! world and the automaton world.

use crate::error::{GraphError, GraphResult};
use crate::connection::Connection;
use kama_core::prelude::*;
use kama_core::queues::{
    CommandQueue, CommandEnum, SetParameter, TelemetryQueue, 
    MicroControlObserver, Telemetry, SignalSource
};
use kama_core::time::clock::{ClockTick, SystemClock, ClockSource};
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
// Node Storage
// ============================================================================

/// Storage for a node with its connections
struct NodeEntry<T: AudioNum, const BUF_SIZE: usize> {
    /// The node itself
    node: Box<dyn DynProcessor<T, BUF_SIZE>>,
    
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
// Audio Graph
// ============================================================================

/// Main audio graph with clock synchronization
///
/// The graph manages a collection of nodes and connections between them.
/// Processing is driven by a clock source (either internal or external),
/// and the graph can be synchronized with the automaton world via queues.
pub struct AudioGraph<T: AudioNum, const BUF_SIZE: usize> {
    /// Nodes in the graph
    nodes: HashMap<NodeId, NodeEntry<T, BUF_SIZE>>,
    
    /// Next available node ID
    next_id: u32,
    
    /// Clock source for timing
    clock_source: Box<dyn ClockSource>,
    
    /// Current clock tick
    current_tick: ClockTick,
    
    /// Queue for receiving commands from automaton world
    command_queue: Option<CommandQueue<CommandEnum>>,
    
    /// Queue for sending telemetry to automaton world
    telemetry_queue: Option<TelemetryQueue>,
    
    /// Observer for micro-control violations
    observer: Option<MicroControlObserver>,
    
    /// Statistics
    stats: GraphStats,
    
    /// Last process time for statistics
    last_process_time: Instant,
}

impl<T: AudioNum, const BUF_SIZE: usize> AudioGraph<T, BUF_SIZE> {
    /// Create a new graph with the given clock source
    pub fn new(clock_source: Box<dyn ClockSource>) -> Self {
        let sample_rate = clock_source.sample_rate();
        
        Self {
            nodes: HashMap::new(),
            next_id: 0,
            clock_source,
            current_tick: ClockTick::new(0, BUF_SIZE as u32, sample_rate),
            command_queue: None,
            telemetry_queue: None,
            observer: None,
            stats: GraphStats::default(),
            last_process_time: Instant::now(),
        }
    }
    
    /// Create a new graph with a system clock
    pub fn with_sample_rate(sample_rate: f32) -> Self {
        Self::new(Box::new(SystemClock::with_sample_rate(sample_rate)))
    }
    
    // ========================================================================
    // Node Management
    // ========================================================================
    
    /// Add a processor node to the graph
    pub fn add_processor(
        &mut self,
        node: Box<dyn DynProcessor<T, BUF_SIZE>>,
    ) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        
        let metadata = node.dyn_metadata();
        let entry = NodeEntry {
            node,
            audio_outputs: Vec::new(),
            control_outputs: Vec::new(),
            clock_outputs: Vec::new(),
            processed: false,
        };
        
        self.nodes.insert(id, entry);
        
        log::debug!("Added processor: {} ({:?})", metadata.name, id);
        id
    }
    
    /// Remove a node from the graph
    pub fn remove_node(&mut self, id: NodeId) -> Option<Box<dyn DynProcessor<T, BUF_SIZE>>> {
        if let Some(entry) = self.nodes.remove(&id) {
            // Remove all connections to this node
            for entry in self.nodes.values_mut() {
                entry.audio_outputs.retain(|(_, target, _)| *target != id);
                entry.control_outputs.retain(|(_, target, _)| *target != id);
                entry.clock_outputs.retain(|(_, target, _)| *target != id);
            }
            
            log::debug!("Removed node: {:?}", id);
            Some(entry.node)
        } else {
            None
        }
    }
    
    // ========================================================================
    // Connection Management
    // ========================================================================
    
    /// Connect audio output to audio input
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
            Ok(())
        } else {
            Err(GraphError::NodeNotFound(from))
        }
    }
    
    /// Connect control output to control input
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
            Ok(())
        } else {
            Err(GraphError::NodeNotFound(from))
        }
    }
    
    /// Connect clock output to clock input
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
            Ok(())
        } else {
            Err(GraphError::NodeNotFound(from))
        }
    }
    
    /// Validate that a connection is possible
    fn validate_connection(&self, from: NodeId, to: NodeId) -> GraphResult<()> {
        if !self.nodes.contains_key(&from) {
            return Err(GraphError::NodeNotFound(from));
        }
        if !self.nodes.contains_key(&to) {
            return Err(GraphError::NodeNotFound(to));
        }
        Ok(())
    }
    
    // ========================================================================
    // Queue Connections
    // ========================================================================
    
    /// Connect command queue for automation
    pub fn connect_command_queue(&mut self, queue: CommandQueue<CommandEnum>) {
        self.command_queue = Some(queue);
    }
    
    /// Connect telemetry queue for feedback
    pub fn connect_telemetry(&mut self, queue: TelemetryQueue) {
        self.telemetry_queue = Some(queue);
    }
    
    /// Connect micro-control observer
    pub fn connect_observer(&mut self, observer: MicroControlObserver) {
        self.observer = Some(observer);
    }
    
    // ========================================================================
    // Command Processing
    // ========================================================================
    
    /// Process pending commands from the automaton world
    fn process_commands(&mut self) {
        let Some(queue) = &self.command_queue else { return };
        
        while let Ok(cmd_enum) = queue.try_recv() {
            match cmd_enum {
                CommandEnum::SetParameter(cmd) => {
                    self.apply_set_parameter(cmd);
                    self.stats.commands_applied += 1;
                }
                _ => {
                    self.stats.commands_rejected += 1;
                }
            }
        }
    }
    
    /// Apply a parameter change command
    fn apply_set_parameter(&mut self, cmd: SetParameter) {
        let port = cmd.port;
        let node_id = port.node_id();
        let param = cmd.parameter;
        let value = cmd.value;
        
        let Some(entry) = self.nodes.get_mut(&node_id) else {
            log::warn!("SetParameter for unknown node {:?}", node_id);
            return;
        };
        
        let start = Instant::now();
        
        if let Err(e) = entry.node.dyn_set_parameter(&param, ParamValue::Float(value)) {
            log::error!("Failed to set parameter: {}", e);
            return;
        }
        
        let elapsed = start.elapsed();
        let elapsed_ns = elapsed.as_nanos() as u64;
        
        // Detect micro-control violations
        if elapsed_ns > 1000 && matches!(cmd.source, SignalSource::Servo(_)) {
            if let Some(obs) = &self.observer {
                obs.record_violation(
                    &cmd.source.to_string(),
                    1000,
                    elapsed_ns,
                    Some(value),
                );
                self.stats.violations += 1;
            }
        }
        
        // Send telemetry
        if let Some(tx) = &self.telemetry_queue {
            let _ = tx.send_parameter(port, param, value);
        }
    }
    
    // ========================================================================
    // Main Processing Loop
    // ========================================================================
    
    /// Process one block of audio
    ///
    /// This is called by the active node (Source or Sink) when it needs data.
    pub fn process_block(&mut self, active_node: NodeId) -> GraphResult<()> {
        let start = Instant::now();
        
        // Process commands from automaton world
        self.process_commands();
        
        // Get next clock tick
        self.current_tick = self.clock_source.next_tick();
        
        // Reset processed flags
        for entry in self.nodes.values_mut() {
            entry.processed = false;
        }
        
        // Start processing from active node
        self.process_node(active_node)?;
        
        // Update statistics
        let elapsed = start.elapsed();
        let elapsed_ns = elapsed.as_nanos() as u64;
        
        self.stats.blocks_processed += 1;
        self.stats.max_process_time_ns = self.stats.max_process_time_ns.max(elapsed_ns);
        self.stats.avg_process_time_ns = self.stats.avg_process_time_ns * 0.95 + elapsed_ns as f64 * 0.05;
        
        Ok(())
    }
    
    /// Process a single node and its dependencies
    fn process_node(&mut self, node_id: NodeId) -> GraphResult<()> {
        let entry = self.nodes.get_mut(&node_id)
            .ok_or(GraphError::NodeNotFound(node_id))?;
        
        if entry.processed {
            return Ok(());
        }
        
        let metadata = entry.node.dyn_metadata();
        
        // Prepare buffers for this node
        let mut audio_inputs = Vec::with_capacity(metadata.audio_inputs);
        let mut control_inputs = vec![T::ZERO; metadata.control_inputs];
        let mut clock_inputs = vec![self.current_tick; metadata.clock_inputs];
        let mut feedback_inputs = Vec::with_capacity(metadata.feedback_ports);
        
        let mut audio_outputs = vec![[T::ZERO; BUF_SIZE]; metadata.audio_outputs];
        let mut audio_output_refs: Vec<&mut [T; BUF_SIZE]> = audio_outputs.iter_mut().collect();
        
        let mut control_outputs = vec![T::ZERO; metadata.control_outputs];
        let mut clock_outputs = vec![self.current_tick; metadata.clock_outputs];
        let mut feedback_outputs = vec![[T::ZERO; BUF_SIZE]; metadata.feedback_ports];
        let mut feedback_output_refs: Vec<&mut [T; BUF_SIZE]> = feedback_outputs.iter_mut().collect();
        
        // TODO: Gather inputs from connections
        
        // Process the node
        entry.node.dyn_process(
            &self.current_tick,
            &audio_inputs,
            &control_inputs,
            &clock_inputs,
            &feedback_inputs,
            &mut audio_output_refs,
            &mut control_outputs,
            &mut clock_outputs,
            &mut feedback_output_refs,
        )?;
        
        entry.processed = true;
        
        // Send outputs to connected nodes
        self.route_outputs(node_id, &audio_outputs, &control_outputs, &clock_outputs)?;
        
        Ok(())
    }
    
    /// Route outputs to connected nodes
    fn route_outputs(
        &mut self,
        from: NodeId,
        audio_outputs: &[[T; BUF_SIZE]],
        control_outputs: &[T],
        clock_outputs: &[ClockTick],
    ) -> GraphResult<()> {
        let entry = self.nodes.get(&from).unwrap();
        
        // Route audio outputs
        for (port, target, target_port) in &entry.audio_outputs {
            if let Some(audio) = audio_outputs.get(*port as usize) {
                // TODO: Deliver audio to target node
                // This would involve storing the audio in the target's input buffers
            }
        }
        
        // Route control outputs
        for (port, target, target_port) in &entry.control_outputs {
            if let Some(control) = control_outputs.get(*port as usize) {
                // TODO: Deliver control to target node
            }
        }
        
        // Route clock outputs
        for (port, target, target_port) in &entry.clock_outputs {
            if let Some(clock) = clock_outputs.get(*port as usize) {
                // TODO: Deliver clock to target node
            }
        }
        
        Ok(())
    }
    
    // ========================================================================
    // Public API
    // ========================================================================
    
    /// Get current statistics
    pub fn stats(&self) -> GraphStats {
        self.stats
    }
    
    /// Get current clock tick
    pub fn current_tick(&self) -> ClockTick {
        self.current_tick
    }
    
    /// Check if node exists
    pub fn contains_node(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }
    
    /// Get number of nodes
    pub fn node_count(&self) -> usize {
        self.nodes.len()
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
}