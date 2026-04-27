//! Core node traits for the Rill ecosystem
//!
//! Defines the fundamental building blocks of the audio graph:
//! - `AudioNode`: Base trait for all nodes
//! - `Source`: Active generator (has no inputs)
//! - `Processor`: Passive processor (has inputs and outputs)
//! - `Sink`: Active consumer (has no outputs)

use crate::queues::signal::SetParameter;
use crate::time::ClockTick;
use crate::traits::param::{ParamMetadata, ParamValue, ParameterId};
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
    pub fn of<T: 'static + ?Sized>() -> Self {
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

impl NodeMetadata {
    /// Create new node metadata with minimal info
    pub fn new(name: &str, category: NodeCategory) -> Self {
        Self {
            name: name.to_string(),
            category,
            description: String::new(),
            author: String::new(),
            version: String::new(),
            audio_inputs: 0,
            audio_outputs: 0,
            control_inputs: 0,
            control_outputs: 0,
            clock_inputs: 0,
            clock_outputs: 0,
            feedback_ports: 0,
            parameters: Vec::new(),
        }
    }
}

// ============================================================================
// Node State
// ============================================================================

/// State of a node during processing
/// State of a node during processing
#[derive(Debug, Clone)]
pub struct NodeState<T: crate::math::Transcendental, const BUF_SIZE: usize> {
    /// Current sample position
    pub sample_pos: u64,

    /// Number of processed blocks
    pub blocks_processed: u64,

    /// Sample rate
    pub sample_rate: f32,

    /// Whether the node is active
    pub active: bool,

    /// Internal phase (for generators)
    pub phase: T,
}

impl<T: crate::math::Transcendental, const BUF_SIZE: usize> NodeState<T, BUF_SIZE> {
    /// Create new node state
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_pos: 0,
            blocks_processed: 0,
            sample_rate,
            active: true,
            phase: T::ZERO,
        }
    }

    /// Advance state by one block
    pub fn advance(&mut self) {
        self.sample_pos += BUF_SIZE as u64;
        self.blocks_processed += 1;
    }

    /// Get current time in seconds
    pub fn current_time_seconds(&self) -> f64 {
        self.sample_pos as f64 / self.sample_rate as f64
    }

    /// Reset state
    pub fn reset(&mut self) {
        self.sample_pos = 0;
        self.blocks_processed = 0;
        self.phase = T::ZERO;
    }
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
pub trait AudioNode<T: crate::math::Transcendental, const BUF_SIZE: usize>: Send + Sync {
    /// Get node metadata
    fn metadata(&self) -> NodeMetadata;

    /// Get the node's type ID
    fn node_type_id(&self) -> NodeTypeId
    where
        Self: 'static + Sized,
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

    /// Apply a `SetParameter` command to this node.
    ///
    /// Routes the command to the appropriate port based on `cmd.port`.
    /// Falls back to `set_parameter()` when the port is not found.
    fn apply_set_parameter(&mut self, cmd: &SetParameter) -> ProcessResult<()> {
        use crate::traits::port::{PortDirection, PortType};
        let value = T::from_f32(cmd.value);
        let port = match cmd.port.port_type() {
            PortType::Control => self.control_port_mut(cmd.port.index() as usize),
            PortType::Audio => match cmd.port.direction() {
                PortDirection::Input => self.input_port_mut(cmd.port.index() as usize),
                PortDirection::Output => self.output_port_mut(cmd.port.index() as usize),
            },
            PortType::Param => self.input_port_mut(cmd.port.index() as usize),
            PortType::Clock | PortType::Feedback => None,
        };
        match port {
            Some(p) => {
                p.set_value(value);
                Ok(())
            }
            None => self.set_parameter(&cmd.parameter, ParamValue::Float(cmd.value)),
        }
    }

    /// Get node ID
    fn id(&self) -> NodeId;

    /// Set node ID
    fn set_id(&mut self, id: NodeId);

    /// Get input port by index
    fn input_port(&self, index: usize) -> Option<&crate::traits::port::Port<T, BUF_SIZE>>;

    /// Get mutable input port by index
    fn input_port_mut(
        &mut self,
        index: usize,
    ) -> Option<&mut crate::traits::port::Port<T, BUF_SIZE>>;

    /// Get output port by index
    fn output_port(&self, index: usize) -> Option<&crate::traits::port::Port<T, BUF_SIZE>>;

    /// Get mutable output port by index
    fn output_port_mut(
        &mut self,
        index: usize,
    ) -> Option<&mut crate::traits::port::Port<T, BUF_SIZE>>;

    /// Get control port by index
    fn control_port(&self, index: usize) -> Option<&crate::traits::port::Port<T, BUF_SIZE>>;

    /// Get mutable control port by index
    fn control_port_mut(
        &mut self,
        index: usize,
    ) -> Option<&mut crate::traits::port::Port<T, BUF_SIZE>>;

    /// Get node state
    fn state(&self) -> &NodeState<T, BUF_SIZE>;

    /// Get mutable node state
    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE>;

    // ========================================================================
    // Port Counting (with defaults)
    // ========================================================================

    /// Number of audio input ports
    fn num_audio_inputs(&self) -> usize {
        0
    }

    /// Number of audio output ports
    fn num_audio_outputs(&self) -> usize {
        0
    }

    /// Number of control input ports
    fn num_control_inputs(&self) -> usize {
        0
    }

    /// Number of control output ports
    fn num_control_outputs(&self) -> usize {
        0
    }

    /// Number of clock input ports
    fn num_clock_inputs(&self) -> usize {
        0
    }

    /// Number of clock output ports
    fn num_clock_outputs(&self) -> usize {
        0
    }

    /// Number of feedback ports
    fn num_feedback_ports(&self) -> usize {
        0
    }

    /// Total number of input ports
    fn num_inputs(&self) -> usize {
        self.num_audio_inputs()
            + self.num_control_inputs()
            + self.num_clock_inputs()
            + self.num_feedback_ports()
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
pub trait Source<T: crate::math::Transcendental, const BUF_SIZE: usize>: AudioNode<T, BUF_SIZE> {
    /// Generate the next block of audio
    ///
    /// # Arguments
    /// * `clock` - Current clock tick
    /// * `control_inputs` - Control signal values (one per control input)
    /// * `clock_inputs` - Clock signal values (one per clock input)
    ///
    /// The source writes output samples into its own output port buffers,
    /// accessible via `self.output_port_mut(index)`.
    fn generate(
        &mut self,
        clock: &ClockTick,
        control_inputs: &[T],
        clock_inputs: &[ClockTick],
    ) -> ProcessResult<()>;

    /// Number of audio outputs (default 1)
    fn num_audio_outputs(&self) -> usize {
        1
    }

    /// Number of control inputs (default 0)
    fn num_control_inputs(&self) -> usize {
        0
    }

    /// Number of clock inputs (default 0)
    fn num_clock_inputs(&self) -> usize {
        0
    }
}

// ============================================================================
// Processor Trait (Passive processors)
// ============================================================================

/// Passive processor of audio signals
///
/// Processors transform input signals into output signals.
/// They have audio inputs and outputs, and may have control and clock ports.
pub trait Processor<T: crate::math::Transcendental, const BUF_SIZE: usize>:
    AudioNode<T, BUF_SIZE>
{
    /// Process a block of audio
    ///
    /// # Arguments
    /// * `clock` - Current clock tick
    /// * `audio_inputs` - Audio input buffers (one per audio input)
    /// * `control_inputs` - Control signal values (one per control input)
    /// * `clock_inputs` - Clock signal values (one per clock input)
    /// * `feedback_inputs` - Feedback values from previous blocks (one per feedback port)
    ///
    /// The processor writes output samples into its own output port buffers,
    /// accessible via `self.output_port_mut(index)`.
    fn process(
        &mut self,
        clock: &ClockTick,
        audio_inputs: &[&[T; BUF_SIZE]],
        control_inputs: &[T],
        clock_inputs: &[ClockTick],
        feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()>;

    /// Latency in samples (for delay compensation)
    fn latency(&self) -> usize {
        0
    }
}

// ============================================================================
// Sink Trait (Active consumers)
// ============================================================================

/// Active sink of audio signals
///
/// Sinks consume audio and send it to external destinations.
/// They have no audio outputs, but may have control and clock ports.
pub trait Sink<T: crate::math::Transcendental, const BUF_SIZE: usize>: AudioNode<T, BUF_SIZE> {
    /// Consume a block of audio
    ///
    /// # Arguments
    /// * `clock` - Current clock tick
    /// * `audio_inputs` - Audio input buffers (one per audio input)
    /// * `control_inputs` - Control signal values (one per control input)
    /// * `clock_inputs` - Clock signal values (one per clock input)
    /// * `feedback_inputs` - Feedback values from previous blocks
    fn consume(
        &mut self,
        clock: &ClockTick,
        audio_inputs: &[&[T; BUF_SIZE]],
        control_inputs: &[T],
        clock_inputs: &[ClockTick],
        feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()>;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Transcendental;

    struct TestNode;

    impl<T: Transcendental, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE> for TestNode {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "Test".to_string(),
                category: NodeCategory::Utility,
                description: "Test node".to_string(),
                author: "Rill".to_string(),
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
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
            None
        }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
            Ok(())
        }

        fn id(&self) -> NodeId {
            NodeId(0)
        }
        fn set_id(&mut self, _id: NodeId) {}

        fn input_port(&self, _index: usize) -> Option<&crate::traits::port::Port<T, BUF_SIZE>> {
            None
        }
        fn input_port_mut(
            &mut self,
            _index: usize,
        ) -> Option<&mut crate::traits::port::Port<T, BUF_SIZE>> {
            None
        }
        fn output_port(&self, _index: usize) -> Option<&crate::traits::port::Port<T, BUF_SIZE>> {
            None
        }
        fn output_port_mut(
            &mut self,
            _index: usize,
        ) -> Option<&mut crate::traits::port::Port<T, BUF_SIZE>> {
            None
        }
        fn control_port(&self, _index: usize) -> Option<&crate::traits::port::Port<T, BUF_SIZE>> {
            None
        }
        fn control_port_mut(
            &mut self,
            _index: usize,
        ) -> Option<&mut crate::traits::port::Port<T, BUF_SIZE>> {
            None
        }

        fn state(&self) -> &NodeState<T, BUF_SIZE> {
            unimplemented!()
        }

        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
            unimplemented!()
        }
    }

    #[test]
    fn test_node_id() {
        let id = NodeId::new(42);
        assert_eq!(id.inner(), 42);
        assert_eq!(format!("{}", id), "Node(42)");
    }

    #[test]
    fn test_node_category() {
        assert_eq!(NodeCategory::Source.name(), "source");
        assert_eq!(NodeCategory::Processor.name(), "processor");
        assert_eq!(NodeCategory::Sink.name(), "sink");
        assert_eq!(NodeCategory::Utility.name(), "utility");
    }

    #[test]
    fn test_node_metadata_new() {
        let metadata = NodeMetadata::new("Test", NodeCategory::Source);
        assert_eq!(metadata.name, "Test");
        assert_eq!(metadata.category, NodeCategory::Source);
    }

    #[test]
    fn test_node_state() {
        let mut state = NodeState::<f32, 64>::new(44100.0);
        assert_eq!(state.sample_pos, 0);
        assert_eq!(state.sample_rate, 44100.0);

        state.advance();
        assert_eq!(state.sample_pos, 64);
        assert_eq!(state.blocks_processed, 1);

        state.reset();
        assert_eq!(state.sample_pos, 0);
        assert_eq!(state.blocks_processed, 0);
    }
}
