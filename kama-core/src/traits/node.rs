//! Core node traits for the Kama Audio ecosystem
//!
//! Defines the fundamental building blocks of the audio graph:
//! - `AudioNode`: Base trait for all nodes
//! - `Source`: Active generator (has no inputs)
//! - `Processor`: Passive processor (has inputs and outputs)
//! - `Sink`: Active consumer (has no outputs)

use crate::traits::param::{ParameterId, ParamValue, ParamMetadata};
use crate::time::ClockTick;
use crate::traits::ProcessResult;
use std::any::TypeId;
use std::fmt;

// ============================================================================
// Node Identification
// ============================================================================

/// Unique identifier for a node in the graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    /// Create a new node ID
    pub const fn new(id: u32) -> Self {
        Self(id)
    }
    
    /// Get the inner value
    pub const fn inner(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.0)
    }
}

impl From<u32> for NodeId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

// ============================================================================
// Node Category
// ============================================================================

/// Category of a node (for UI/organization)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeCategory {
    /// Source nodes (generators)
    Source,
    
    /// Processor nodes (effects, filters)
    Processor,
    
    /// Sink nodes (outputs)
    Sink,
    
    /// Utility nodes (routing, mixing)
    Utility,
    
    /// Analyzer nodes (meters, scopes)
    Analyzer,
    
    /// Sequencer nodes (pattern generators)
    Sequencer,
}

impl NodeCategory {
    /// Get the name of the category
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Processor => "processor",
            Self::Sink => "sink",
            Self::Utility => "utility",
            Self::Analyzer => "analyzer",
            Self::Sequencer => "sequencer",
        }
    }
}

impl fmt::Display for NodeCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// Node Type ID
// ============================================================================

/// Type identifier for a node (for downcasting)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeTypeId(TypeId);

impl NodeTypeId {
    /// Create a new node type ID from a type
    pub fn of<T: 'static>() -> Self {
        Self(TypeId::of::<T>())
    }
    
    /// Get the inner TypeId
    pub fn as_type_id(&self) -> TypeId {
        self.0
    }
}

// ============================================================================
// Node Metadata
// ============================================================================

/// Metadata about a node
#[derive(Debug, Clone)]
pub struct NodeMetadata {
    /// Name of the node
    pub name: String,
    
    /// Category of the node
    pub category: NodeCategory,
    
    /// Description of what the node does
    pub description: String,
    
    /// Author of the node
    pub author: String,
    
    /// Version of the node
    pub version: String,
    
    /// Number of audio input ports
    pub audio_inputs: usize,
    
    /// Number of audio output ports
    pub audio_outputs: usize,
    
    /// Number of control input ports
    pub control_inputs: usize,
    
    /// Number of control output ports
    pub control_outputs: usize,
    
    /// Number of clock input ports
    pub clock_inputs: usize,
    
    /// Number of clock output ports
    pub clock_outputs: usize,
    
    /// Number of feedback ports
    pub feedback_ports: usize,
    
    /// Parameters exposed by the node
    pub parameters: Vec<ParamMetadata>,
}

// ============================================================================
// AudioNode Trait (Base for all nodes)
// ============================================================================

/// Base trait for all audio nodes
///
/// This trait provides the fundamental operations that every node must implement:
/// - Port counting
/// - Parameter access
/// - Initialization and reset
///
/// The actual processing is split into specialized traits:
/// - `Source` for generators
/// - `Processor` for processors with inputs/outputs
/// - `Sink` for consumers
pub trait AudioNode<T: crate::math::AudioNum, const BUF_SIZE: usize>: Send + Sync {
    /// Get node metadata
    fn metadata(&self) -> NodeMetadata;
    
    /// Get the node's type ID
    fn node_type_id(&self) -> NodeTypeId 
    where 
         Self: 'static + Sized
    {
        NodeTypeId::of::<Self>()
    }
    
    /// Initialize the node with a sample rate
    fn init(&mut self, sample_rate: f32);
    
    /// Reset the node to its initial state
    fn reset(&mut self);
    
    /// Get the value of a parameter
    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue>;
    
    /// Set the value of a parameter
    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()>;
    
    // ========================================================================
    // Port Counting (with defaults)
    // ========================================================================
    
    /// Number of audio input ports
    fn num_audio_inputs(&self) -> usize { 0 }
    
    /// Number of audio output ports
    fn num_audio_outputs(&self) -> usize { 0 }
    
    /// Number of control input ports
    fn num_control_inputs(&self) -> usize { 0 }
    
    /// Number of control output ports
    fn num_control_outputs(&self) -> usize { 0 }
    
    /// Number of clock input ports
    fn num_clock_inputs(&self) -> usize { 0 }
    
    /// Number of clock output ports
    fn num_clock_outputs(&self) -> usize { 0 }
    
    /// Number of feedback ports
    fn num_feedback_ports(&self) -> usize { 0 }
    
    /// Total number of input ports
    fn num_inputs(&self) -> usize {
        self.num_audio_inputs() + self.num_control_inputs() + 
        self.num_clock_inputs() + self.num_feedback_ports()
    }
    
    /// Total number of output ports
    fn num_outputs(&self) -> usize {
        self.num_audio_outputs() + self.num_control_outputs() + self.num_clock_outputs()
    }
}

// ============================================================================
// Source Trait (Active generators)
// ============================================================================

/// Active source of audio signals
///
/// Sources generate audio from internal state. They have no audio inputs,
/// but may have control and clock inputs for modulation.
pub trait Source<T: crate::math::AudioNum, const BUF_SIZE: usize>: AudioNode<T, BUF_SIZE> {
    /// Generate the next block of audio
    ///
    /// # Arguments
    /// * `clock` - Current clock tick
    /// * `control_inputs` - Control signal values (one per control input)
    /// * `clock_inputs` - Clock signal values (one per clock input)
    /// * `outputs` - Audio output buffers to fill
    fn generate(
        &mut self,
        clock: &ClockTick,
        control_inputs: &[T],
        clock_inputs: &[ClockTick],
        outputs: &mut [&mut [T; BUF_SIZE]],
    ) -> ProcessResult<()>;
}

// ============================================================================
// Processor Trait (Passive processors)
// ============================================================================

/// Passive processor of audio signals
///
/// Processors transform input signals into output signals.
/// They have audio inputs and outputs, and may have control and clock ports.
pub trait Processor<T: crate::math::AudioNum, const BUF_SIZE: usize>: AudioNode<T, BUF_SIZE> {
    /// Process a block of audio
    ///
    /// # Arguments
    /// * `clock` - Current clock tick
    /// * `audio_inputs` - Audio input buffers (one per audio input)
    /// * `control_inputs` - Control signal values (one per control input)
    /// * `clock_inputs` - Clock signal values (one per clock input)
    /// * `feedback_inputs` - Feedback values from previous blocks (one per feedback port)
    /// * `audio_outputs` - Audio output buffers to fill
    /// * `control_outputs` - Control output values to send (one per control output)
    /// * `clock_outputs` - Clock output values to send (one per clock output)
    /// * `feedback_outputs` - Feedback values to store for next block
    fn process(
        &mut self,
        clock: &ClockTick,
        audio_inputs: &[&[T; BUF_SIZE]],
        control_inputs: &[T],
        clock_inputs: &[ClockTick],
        feedback_inputs: &[&[T; BUF_SIZE]],
        audio_outputs: &mut [&mut [T; BUF_SIZE]],
        control_outputs: &mut [T],
        clock_outputs: &mut [ClockTick],
        feedback_outputs: &mut [&mut [T; BUF_SIZE]],
    ) -> ProcessResult<()>;
}

// ============================================================================
// Sink Trait (Active consumers)
// ============================================================================

/// Active sink of audio signals
///
/// Sinks consume audio and send it to external destinations.
/// They have no audio outputs, but may have control and clock ports.
pub trait Sink<T: crate::math::AudioNum, const BUF_SIZE: usize>: AudioNode<T, BUF_SIZE> {
    /// Consume a block of audio
    ///
    /// # Arguments
    /// * `clock` - Current clock tick
    /// * `audio_inputs` - Audio input buffers (one per audio input)
    /// * `control_inputs` - Control signal values (one per control input)
    /// * `clock_inputs` - Clock signal values (one per clock input)
    /// * `feedback_inputs` - Feedback values from previous blocks
    /// * `control_outputs` - Control output values to send
    /// * `clock_outputs` - Clock output values to send
    fn consume(
        &mut self,
        clock: &ClockTick,
        audio_inputs: &[&[T; BUF_SIZE]],
        control_inputs: &[T],
        clock_inputs: &[ClockTick],
        feedback_inputs: &[&[T; BUF_SIZE]],
        control_outputs: &mut [T],
        clock_outputs: &mut [ClockTick],
    ) -> ProcessResult<()>;
}

// ============================================================================
// Sequencer Trait (Pattern generators)
// ============================================================================

/// Sequencer node for pattern generation
///
/// Sequencers generate control signals based on patterns and clock ticks.
/// They are a specialized form of source for live-coding and generative music.
pub trait Sequencer<T: crate::math::AudioNum, const BUF_SIZE: usize>: AudioNode<T, BUF_SIZE> {
    /// Generate the next block of control signals
    ///
    /// # Arguments
    /// * `clock` - Current clock tick
    /// * `control_inputs` - Control inputs (for modulating the sequencer)
    /// * `clock_inputs` - Clock inputs (multiple clocks possible)
    /// * `outputs` - Control outputs (MIDI, CV, gates)
    fn tick(
        &mut self,
        clock: &ClockTick,
        control_inputs: &[T],
        clock_inputs: &[ClockTick],
        outputs: &mut [T],
    ) -> ProcessResult<()>;
}

// ============================================================================
// Type Erasure Helpers
// ============================================================================

/// Type-erased audio node for storage in collections
pub trait DynAudioNode: Send + Sync {
    fn dyn_node_type_id(&self) -> NodeTypeId;
    fn dyn_metadata(&self) -> NodeMetadata;
    fn dyn_init(&mut self, sample_rate: f32);
    fn dyn_reset(&mut self);
    fn dyn_get_parameter(&self, id: &ParameterId) -> Option<ParamValue>;
    fn dyn_set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()>;
    
    // Port counts
    fn dyn_num_audio_inputs(&self) -> usize;
    fn dyn_num_audio_outputs(&self) -> usize;
    fn dyn_num_control_inputs(&self) -> usize;
    fn dyn_num_control_outputs(&self) -> usize;
    fn dyn_num_clock_inputs(&self) -> usize;
    fn dyn_num_clock_outputs(&self) -> usize;
    fn dyn_num_feedback_ports(&self) -> usize;
}

/// Type-erased processor
pub trait DynProcessor: DynAudioNode {
    fn dyn_process(
        &mut self,
        clock: &ClockTick,
        audio_inputs: &[&[f32]],
        control_inputs: &[f32],
        clock_inputs: &[ClockTick],
        feedback_inputs: &[&[f32]],
        audio_outputs: &mut [&mut [f32]],
        control_outputs: &mut [f32],
        clock_outputs: &mut [ClockTick],
        feedback_outputs: &mut [&mut [f32]],
    ) -> ProcessResult<()>;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::AudioNum;
    
    struct TestNode;
    
    impl<T: AudioNum, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE> for TestNode {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "Test".to_string(),
                category: NodeCategory::Utility,
                description: "Test node".to_string(),
                author: "Kama".to_string(),
                version: "1.0".to_string(),
                audio_inputs: 0,
                audio_outputs: 0,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            }
        }
        
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> { None }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> { Ok(()) }
    }
    
    #[test]
    fn test_node_id() {
        let id = NodeId::new(42);
        assert_eq!(id.inner(), 42);
        assert_eq!(format!("{}", id), "Node(42)");
    }
}