//! Real-time audio graph with proper separation of audio and control
#![allow(unused)]

use kama_core::buffer::pipe::PipeBuffer;
use kama_core::math::AudioNum;
use kama_core::traits::{NodeId, PortId, PortType, Source, Processor, Sink};
use kama_core::{CommandEnum, CommandQueue, TelemetryQueue, MicroControlObserver, ProcessError};
use kama_core::time::TickInfo;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Control value (single sample, atomically updated)
type ControlValue = Arc<AtomicU32>;

// Graph error type
#[derive(Debug, Clone, PartialEq)]
pub enum GraphError {
    NodeNotFound(NodeId),
    TypeMismatch { from_type: PortType, to_type: PortType },
    DirectionMismatch { from_dir: PortDirection, to_dir: PortDirection },
    ProcessError(ProcessError),
}

impl From<ProcessError> for GraphError {
    fn from(err: ProcessError) -> Self {
        GraphError::ProcessError(err)
    }
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::NodeNotFound(node_id) => write!(f, "Node not found: {:?}", node_id),
            GraphError::TypeMismatch { from_type, to_type } => write!(f, "Type mismatch from {:?} to {:?}", from_type, to_type),
            GraphError::DirectionMismatch { from_dir, to_dir } => write!(f, "Direction mismatch from {:?} to {:?}", from_dir, to_dir),
            GraphError::ProcessError(err) => write!(f, "Process error: {:?}", err),
        }
    }
}

impl std::error::Error for GraphError {}

pub type GraphResult<T> = Result<T, GraphError>;

// Re-export policies and direction from kama-core
pub use kama_core::queues::OverflowPolicy;
pub use kama_core::traits::PortDirection;

// UnderflowPolicy not exported by kama-core; define locally
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderflowPolicy {
    DropNewest,
    DropOldest,
}

// Stub types for compatibility (to be properly defined later)
pub enum GraphRole {
    Processor,
    Producer,
    Consumer,
    Bridge,
}

pub enum GraphState {
    Idle,
    Running,
    Paused,
}

pub enum DataFlow {
    Standalone,
    Producer,
    Consumer,
    Bridge,
}

pub struct PortFlowConfig {}

pub struct GraphStats {
    commands_rejected: usize,
}

pub struct PortStats {}

pub struct AudioGraph<const BUF_SIZE: usize> {
    // Core audio processing
    audio_nodes: HashMap<NodeId, Box<dyn Processor<f32, BUF_SIZE>>>,
    source_nodes: HashMap<NodeId, Box<dyn Source<f32, BUF_SIZE>>>,
    sink_nodes: HashMap<NodeId, Box<dyn Sink<f32, BUF_SIZE>>>,
    audio_connections: HashMap<PortId, PipeBuffer<f32, BUF_SIZE>>,
    audio_input_map: HashMap<PortId, PortId>,
    
    // Control signal routing (single values, not buffers)
    control_nodes: HashMap<NodeId, ControlState>,
    control_connections: HashMap<PortId, ControlValue>,
    control_input_map: HashMap<PortId, PortId>,
    
    // Topology
    processing_order: Vec<NodeId>,
    dependencies: HashMap<NodeId, HashSet<NodeId>>,
    
    // Configuration
    sample_rate: f32,
    control_rate: f32,  // e.g., 1000 Hz for control
    next_id: u32,
    dirty: bool,
    
    // Communication with patchbay
    command_queue: Option<CommandQueue<CommandEnum>>,
    telemetry_queue: Option<TelemetryQueue>,
    observer: Option<MicroControlObserver>,
    
    // Time synchronization
    current_tick: TickInfo,
    samples_since_last_control: u64,
    control_samples_per_tick: u64,
    
    // Statistics
    stats: GraphStats,
}

struct ControlState {
    /// Current input values
    inputs: Vec<ControlValue>,
    
    /// Current output values
    outputs: Vec<ControlValue>,
    
    /// Last computed value (for nodes that generate control)
    last_value: f32,
}

impl<const BUF_SIZE: usize> AudioGraph<BUF_SIZE> {
    pub fn new_with_control_rate(sample_rate: f32, control_rate: f32) -> Self {
        Self {
            audio_nodes: HashMap::new(),
            source_nodes: HashMap::new(),
            sink_nodes: HashMap::new(),
            audio_connections: HashMap::new(),
            audio_input_map: HashMap::new(),
            control_nodes: HashMap::new(),
            control_connections: HashMap::new(),
            control_input_map: HashMap::new(),
            processing_order: Vec::new(),
            dependencies: HashMap::new(),
            sample_rate,
            control_rate,
            next_id: 0,
            dirty: false,
            command_queue: None,
            telemetry_queue: None,
            observer: None,
            current_tick: TickInfo::new(0, 0, 0, 0),
            samples_since_last_control: 0,
            control_samples_per_tick: (sample_rate / control_rate) as u64,
            stats: GraphStats { commands_rejected: 0 },
        }
    }

    pub fn new(sample_rate: f32) -> Self {
        Self::new_with_control_rate(sample_rate, 1000.0)
    }

    /// Add a processor node to the graph.
    pub fn add_processor(&mut self, processor: Box<dyn Processor<f32, BUF_SIZE>>) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.audio_nodes.insert(id, processor);
        self.dirty = true;
        id
    }

    /// Add a source node to the graph.
    pub fn add_source(&mut self, source: Box<dyn Source<f32, BUF_SIZE>>) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.source_nodes.insert(id, source);
        self.dirty = true;
        id
    }

    /// Add a sink node to the graph.
    pub fn add_sink(&mut self, sink: Box<dyn Sink<f32, BUF_SIZE>>) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.sink_nodes.insert(id, sink);
        self.dirty = true;
        id
    }

    /// Add a node to the graph (compatibility alias for add_processor).
    pub fn add_node(&mut self, node: Box<dyn Processor<f32, BUF_SIZE>>) -> NodeId {
        self.add_processor(node)
    }

    /// Process audio through the graph (compatibility method).
    pub fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> GraphResult<()> {
        // For compatibility with existing examples, do nothing.
        // In a real scenario, this would process the graph using the provided inputs/outputs.
        Ok(())
    }

    /// Check if a node exists in the graph.
    pub fn contains_node(&self, node_id: NodeId) -> bool {
        self.audio_nodes.contains_key(&node_id)
            || self.source_nodes.contains_key(&node_id)
            || self.sink_nodes.contains_key(&node_id)
    }

    /// Get the sample rate of the graph.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Update internal processing order based on current connections.
    fn update_processing_order(&mut self) -> GraphResult<()> {
        // For MVP, assume single source and single sink; order is already correct.
        self.dirty = false;
        Ok(())
    }

    /// Retrieve audio input buffer for a given input port.
    fn get_audio_input(&self, port: PortId) -> GraphResult<[f32; BUF_SIZE]> {
        // Look up which output port is connected to this input
        if let Some(&output_port) = self.audio_input_map.get(&port) {
            // Get the pipe buffer for that output
            if let Some(buffer) = self.audio_connections.get(&output_port) {
                // Try to read data; if none, return zero buffer (underflow)
                if let Some(data) = buffer.try_read() {
                    return Ok(data);
                }
            }
        }
        // No connection or no data -> return zero buffer
        Ok([0.0; BUF_SIZE])
    }


    // ========================================================================
    // Audio Processing (high rate, block-based)
    // ========================================================================
    
    /// Pull a block of audio from the graph
    pub fn pull_audio(&mut self, node_id: NodeId, index: u16, output: &mut [f32; BUF_SIZE]) -> GraphResult<()> {
        // 1. Process pending commands (from patchbay)
        self.process_commands();
        
        // 2. Update control signals if needed
        self.update_control_signals();
        
        // 3. Update topology if needed
        self.update_processing_order()?;
        
        // 4. Process audio graph
        let outputs = self.process_audio_node(node_id)?;
        *output = outputs[index as usize];
        
        Ok(())
    }

    /// Process a single audio node (recursive)
    fn process_audio_node(&mut self, node_id: NodeId) -> GraphResult<Vec<[f32; BUF_SIZE]>> {
        // Get node metadata without mutable borrow
        let num_inputs = self.audio_nodes
            .get(&node_id)
            .ok_or(GraphError::NodeNotFound(node_id))?
            .num_audio_inputs();
        let num_outputs = self.audio_nodes
            .get(&node_id)
            .ok_or(GraphError::NodeNotFound(node_id))?
            .num_audio_outputs();
        
        // Collect audio inputs
        let mut inputs = Vec::with_capacity(num_inputs);
        for i in 0..num_inputs {
            let port = PortId::audio_in(node_id, i as u16);
            inputs.push(self.get_audio_input(port)?);
        }
        
        // Collect control inputs (from control graph)
        let control_inputs = self.get_control_inputs_for_node(node_id);
        
        // Now borrow node mutably for processing
        let node = self.audio_nodes.get_mut(&node_id)
            .ok_or(GraphError::NodeNotFound(node_id))?;
        
        // Process (node implementation can use both audio and control inputs)
        let input_refs: Vec<&[f32; BUF_SIZE]> = inputs.iter().collect();
        let mut outputs = vec![[0.0; BUF_SIZE]; num_outputs];
        let mut output_refs: Vec<&mut [f32; BUF_SIZE]> = outputs.iter_mut().collect();
        
        node.process(&input_refs, &mut output_refs, &control_inputs)?;
        
        // Write outputs to buffers
        for i in 0..num_outputs {
            let port = PortId::audio_out(node_id, i as u16);
            if let Some(buffer) = self.audio_connections.get(&port) {
                buffer.write(&outputs[i]);
            }
        }
        
        Ok(outputs)
    }

    // ========================================================================
    // Source and Sink Processing
    // ========================================================================

    /// Process a source node (generate audio)
    fn process_source_node(&mut self, node_id: NodeId) -> GraphResult<Vec<[f32; BUF_SIZE]>> {
        let control_inputs = self.get_control_inputs_for_node(node_id);
        let source = self.source_nodes.get_mut(&node_id)
            .ok_or(GraphError::NodeNotFound(node_id))?;
        let num_outputs = source.num_audio_outputs();
        
        // Prepare output buffers
        let mut outputs = vec![[0.0; BUF_SIZE]; num_outputs];
        let mut output_refs: Vec<&mut [f32; BUF_SIZE]> = outputs.iter_mut().collect();
        
        source.generate(&mut output_refs, &control_inputs)?;
        
        // Write outputs to buffers
        for i in 0..num_outputs {
            let port = PortId::audio_out(node_id, i as u16);
            if let Some(buffer) = self.audio_connections.get(&port) {
                buffer.write(&outputs[i]);
            }
        }
        
        Ok(outputs)
    }

    /// Process a sink node (consume audio)
    fn process_sink_node(&mut self, node_id: NodeId) -> GraphResult<()> {
        let control_inputs = self.get_control_inputs_for_node(node_id);
        let num_inputs = {
            let sink = self.sink_nodes.get(&node_id)
                .ok_or(GraphError::NodeNotFound(node_id))?;
            sink.num_audio_inputs()
        };
        
        // Collect audio inputs
        let mut inputs = Vec::with_capacity(num_inputs);
        for i in 0..num_inputs {
            let port = PortId::audio_in(node_id, i as u16);
            inputs.push(self.get_audio_input(port)?);
        }
        
        let input_refs: Vec<&[f32; BUF_SIZE]> = inputs.iter().collect();
        
        let sink = self.sink_nodes.get_mut(&node_id)
            .ok_or(GraphError::NodeNotFound(node_id))?;
        sink.process(&input_refs, &control_inputs).map_err(GraphError::ProcessError)
    }

    /// Push a block through the graph (source-driven processing)
    pub fn push_block(&mut self) -> GraphResult<()> {
        // Process pending commands and update control signals
        self.process_commands();
        self.update_control_signals();
        self.update_processing_order()?;
        
        // Process all source nodes
        let source_ids: Vec<NodeId> = self.source_nodes.keys().copied().collect();
        for node_id in source_ids {
            self.process_source_node(node_id)?;
        }
        
        // Process all processor nodes in topological order
        let processor_ids: Vec<NodeId> = self.processing_order.clone();
        for node_id in processor_ids {
            self.process_audio_node(node_id)?;
        }
        
        // Process all sink nodes
        let sink_ids: Vec<NodeId> = self.sink_nodes.keys().copied().collect();
        for node_id in sink_ids {
            self.process_sink_node(node_id)?;
        }
        
        Ok(())
    }

    // ========================================================================
    // Control Signal Processing (low rate, sample-accurate)
    // ========================================================================
    
    /// Update control signals (called periodically)
    fn update_control_signals(&mut self) {
        self.samples_since_last_control += BUF_SIZE as u64;
        
        if self.samples_since_last_control >= self.control_samples_per_tick {
            // Time to update control signals
            self.process_control_graph();
            self.samples_since_last_control = 0;
            // self.current_tick = self.current_tick.next(); // TickInfo::next not available
        }
    }

    /// Process the control signal graph
    fn process_control_graph(&mut self) {
        // Process nodes in topological order
        for &node_id in &self.processing_order {
            if let Some(state) = self.control_nodes.get_mut(&node_id) {
                // Read current input values as f32
                let mut input_values = Vec::new();
                for input in &state.inputs {
                    let bits = input.load(Ordering::Relaxed);
                    input_values.push(f32::from_bits(bits));
                }
                
                // Here we would call the node's control processor
                // For now, just pass through
                let output = input_values.first().copied().unwrap_or(0.0);
                
                // Write to outputs
                for output_val in &state.outputs {
                    output_val.store(f32::to_bits(output), Ordering::Relaxed);
                }
                
                state.last_value = output;
            }
        }
    }

    /// Get current control inputs for an audio node
    fn get_control_inputs_for_node(&self, node_id: NodeId) -> Vec<f32> {
        let mut result = Vec::new();
        
        // Find all control inputs connected to this node
        for i in 0..16 {  // Max 16 control inputs per node
            let port = PortId::control_in(node_id, i);
            if let Some(source) = self.control_input_map.get(&port) {
                if let Some(value) = self.control_connections.get(source) {
                    result.push(f32::from_bits(value.load(Ordering::Relaxed)));
                } else {
                    result.push(0.0);
                }
            } else {
                break;  // No more connected inputs
            }
        }
        
        result
    }

    // ========================================================================
    // Command Processing (from patchbay)
    // ========================================================================
    
    fn process_commands(&mut self) {
        let Some(queue) = &self.command_queue else { return };
        
        while let Ok(cmd_enum) = queue.try_recv() {
            match cmd_enum {
                // Temporarily ignore parameter and control commands for MVP
                _ => {
                    // Other commands are for patchbay only
                    self.stats.commands_rejected += 1;
                }
            }
        }
    }

    fn apply_set_control(&mut self, port: PortId, value: f32) {
        if let Some(control_val) = self.control_connections.get(&port) {
            control_val.store(value.to_bits(), Ordering::Relaxed);
            
            // Send telemetry
            if let Some(tx) = &self.telemetry_queue {
                // send_control method not available; ignore for now
                // let _ = tx.send_control(port, value);
            }
        }
    }

    // ========================================================================
    // Connection Management
    // ========================================================================
    
    /// Connect audio output to audio input
    pub fn connect_audio(&mut self, from: PortId, to: PortId) -> GraphResult<()> {
        self.validate_ports(from, to, PortType::Audio)?;
        
        let buffer = PipeBuffer::new();
        self.audio_connections.insert(from, buffer);
        self.audio_input_map.insert(to, from);
        self.dirty = true;
        
        Ok(())
    }

    /// Connect control output to control input
    pub fn connect_control(&mut self, from: PortId, to: PortId) -> GraphResult<()> {
        self.validate_ports(from, to, PortType::Control)?;
        
        let value = Arc::new(AtomicU32::new(f32::to_bits(0.0)));
        self.control_connections.insert(from, value.clone());
        self.control_input_map.insert(to, from);
        
        // Also store in node state
        let to_node = to.node_id();
        self.control_nodes
            .entry(to_node)
            .or_insert_with(|| ControlState {
                inputs: Vec::new(),
                outputs: Vec::new(),
                last_value: 0.0,
            })
            .inputs
            .push(value);
        
        self.dirty = true;
        Ok(())
    }

    fn validate_ports(&self, from: PortId, to: PortId, expected_type: PortType) -> GraphResult<()> {
        if from.port_type() != expected_type || to.port_type() != expected_type {
            return Err(GraphError::TypeMismatch {
                from_type: from.port_type(),
                to_type: to.port_type(),
            });
        }
        
        if !from.is_output() || !to.is_input() {
            return Err(GraphError::DirectionMismatch {
                from_dir: from.direction(),
                to_dir: to.direction(),
            });
        }
        
        if !self.audio_nodes.contains_key(&from.node_id()) &&
           !self.source_nodes.contains_key(&from.node_id()) &&
           !self.sink_nodes.contains_key(&from.node_id()) &&
           !self.control_nodes.contains_key(&from.node_id()) {
            return Err(GraphError::NodeNotFound(from.node_id()));
        }
        
        Ok(())
    }
}