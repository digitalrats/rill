//! Node traits for the Kama Audio ecosystem

use crate::traits::param::{ParameterId, ParamValue, ParamMetadata};
use crate::ProcessResult;
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
    
    /// Control nodes (MIDI, CV)
    Control,
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
            Self::Control => "control",
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
    
    /// Parameters exposed by the node
    pub parameters: Vec<ParamMetadata>,
}

// ============================================================================
// Source Trait
// ============================================================================

/// Active source of audio signals
///
/// Sources generate audio from internal state or external input.
/// They have no audio inputs, only audio outputs, but can have control inputs.
pub trait Source<T, const BUF_SIZE: usize>: Send + Sync
where
    T: crate::math::AudioNum + Send + Sync,
{
    /// Generate the next block of audio
    ///
    /// # Arguments
    /// * `outputs` - Array of output buffers to fill (one per channel)
    /// * `control` - Array of control input values (updated by graph)
    fn generate(
        &mut self,
        outputs: &mut [&mut [T; BUF_SIZE]],
        control: &[f32],
    ) -> ProcessResult<()>;

    /// Number of audio output channels
    fn num_audio_outputs(&self) -> usize;

    /// Number of control inputs
    fn num_control_inputs(&self) -> usize {
        0
    }

    /// Get the current value of a parameter
    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue>;

    /// Set a parameter value
    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()>;

    /// Initialize the source with a sample rate
    fn init(&mut self, sample_rate: f32);

    /// Reset the source to its initial state
    fn reset(&mut self);
}

// ============================================================================
// Processor Trait
// ============================================================================

/// Passive processor of audio signals
///
/// Processors transform input signals into output signals.
/// They have both audio inputs and outputs, and can have control inputs.
pub trait Processor<T, const BUF_SIZE: usize>: Send + Sync
where
    T: crate::math::AudioNum + Send + Sync,
{
    /// Process a block of audio
    ///
    /// # Arguments
    /// * `inputs` - Array of input buffers (one per channel)
    /// * `outputs` - Array of output buffers to fill (one per channel)
    /// * `control` - Array of control input values (updated by graph)
    fn process(
        &mut self,
        inputs: &[&[T; BUF_SIZE]],
        outputs: &mut [&mut [T; BUF_SIZE]],
        control: &[f32],
    ) -> ProcessResult<()>;

    /// Number of audio input channels
    fn num_audio_inputs(&self) -> usize;

    /// Number of audio output channels
    fn num_audio_outputs(&self) -> usize;

    /// Number of control inputs
    fn num_control_inputs(&self) -> usize {
        0
    }

    /// Get the current value of a parameter
    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue>;

    /// Set a parameter value
    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()>;

    /// Initialize the processor with a sample rate
    fn init(&mut self, sample_rate: f32);

    /// Reset the processor to its initial state
    fn reset(&mut self);
}

// ============================================================================
// Sink Trait
// ============================================================================

/// Active sink of audio signals
///
/// Sinks consume audio signals and send them to external destinations
/// (sound cards, files, network). They have only audio inputs, and can have control inputs.
pub trait Sink<T, const BUF_SIZE: usize>: Send + Sync
where
    T: crate::math::AudioNum + Send + Sync,
{
    /// Process a block of audio (consumes it)
    ///
    /// # Arguments
    /// * `inputs` - Array of input buffers to consume (one per channel)
    /// * `control` - Array of control input values (updated by graph)
    fn process(
        &mut self,
        inputs: &[&[T; BUF_SIZE]],
        control: &[f32],
    ) -> ProcessResult<()>;

    /// Number of audio input channels
    fn num_audio_inputs(&self) -> usize;

    /// Number of control inputs
    fn num_control_inputs(&self) -> usize {
        0
    }

    /// Get the current value of a parameter
    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue>;

    /// Set a parameter value
    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()>;

    /// Initialize the sink with a sample rate
    fn init(&mut self, sample_rate: f32);

    /// Reset the sink to its initial state
    fn reset(&mut self);
}

// ============================================================================
// Type Erasure Helpers (для использования в графе)
// ============================================================================

/// Type-erased source for storage in collections
pub struct BoxedSource(pub Box<dyn DynSource>);

/// Type-erased processor for storage in collections
pub struct BoxedProcessor(pub Box<dyn DynProcessor>);

/// Type-erased sink for storage in collections
pub struct BoxedSink(pub Box<dyn DynSink>);

/// Dynamic dispatch trait for Source
pub trait DynSource: Send + Sync {
    /// Generate with dynamic dispatch
    fn dyn_generate(
        &mut self,
        outputs: &mut [&mut [f32]],
        control: &[f32],
    ) -> ProcessResult<()>;
    
    fn dyn_num_audio_outputs(&self) -> usize;
    fn dyn_num_control_inputs(&self) -> usize;
    fn dyn_get_parameter(&self, id: &ParameterId) -> Option<ParamValue>;
    fn dyn_set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()>;
    fn dyn_init(&mut self, sample_rate: f32);
    fn dyn_reset(&mut self);
}

/// Dynamic dispatch trait for Processor
pub trait DynProcessor: Send + Sync {
    /// Process with dynamic dispatch
    fn dyn_process(
        &mut self,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        control: &[f32],
    ) -> ProcessResult<()>;
    
    fn dyn_num_audio_inputs(&self) -> usize;
    fn dyn_num_audio_outputs(&self) -> usize;
    fn dyn_num_control_inputs(&self) -> usize;
    fn dyn_get_parameter(&self, id: &ParameterId) -> Option<ParamValue>;
    fn dyn_set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()>;
    fn dyn_init(&mut self, sample_rate: f32);
    fn dyn_reset(&mut self);
}

/// Dynamic dispatch trait for Sink
pub trait DynSink: Send + Sync {
    /// Process with dynamic dispatch
    fn dyn_process(
        &mut self,
        inputs: &[&[f32]],
        control: &[f32],
    ) -> ProcessResult<()>;
    
    fn dyn_num_audio_inputs(&self) -> usize;
    fn dyn_num_control_inputs(&self) -> usize;
    fn dyn_get_parameter(&self, id: &ParameterId) -> Option<ParamValue>;
    fn dyn_set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()>;
    fn dyn_init(&mut self, sample_rate: f32);
    fn dyn_reset(&mut self);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    struct TestSource;
    
    impl<T: crate::math::AudioNum, const BUF_SIZE: usize> Source<T, BUF_SIZE> for TestSource {
        fn generate(
            &mut self,
            outputs: &mut [&mut [T; BUF_SIZE]],
            _control: &[f32],
        ) -> ProcessResult<()> {
            for output in outputs {
                for i in 0..BUF_SIZE {
                    output[i] = T::ZERO;
                }
            }
            Ok(())
        }
        
        fn num_audio_outputs(&self) -> usize { 1 }
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> { None }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> { Ok(()) }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
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
    }
}