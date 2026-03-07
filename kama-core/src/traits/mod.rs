//! # Core Traits for Kama Audio
//!
//! This module defines the fundamental traits that form the backbone
//! of the Kama Audio ecosystem. These traits provide a common interface
//! for all audio nodes, parameters, ports, and time handling.
//!
//! ## Architecture
//!
//! ```text
//!                      AudioNode (base trait)
//!                      /        |        \
//!                     /         |         \
//!               Source     Processor     Sink
//!           (generators)  (processors)  (consumers)
//! ```
//!
//! ## Key Concepts
//!
//! - **AudioNode**: Base trait for all nodes in the graph
//! - **Source**: Active generator (has outputs, no inputs)
//! - **Processor**: Passive processor (has both inputs and outputs)
//! - **Sink**: Active consumer (has inputs, no outputs)
//! - **PortId**: Unique identifier for ports with type and direction
//! - **ParameterId**: Type-safe parameter identifier
//! - **Clock**: Time source for synchronization

mod error;
mod node;
mod param;
mod port;

// Re-export all public items
pub use error::*;
pub use node::*;
pub use param::*;
pub use port::*;

// ============================================================================
// Common Type Aliases
// ============================================================================

/// Default block size for audio processing
pub const DEFAULT_BLOCK_SIZE: usize = 64;

/// Type alias for a mono audio block
pub type MonoBlock<T, const BUF_SIZE: usize> = [T; BUF_SIZE];

/// Type alias for a stereo audio block (left, right)
pub type StereoBlock<T, const BUF_SIZE: usize> = [MonoBlock<T, BUF_SIZE>; 2];

/// Type alias for a control signal value
pub type ControlValue<T> = T;

// ============================================================================
// Prelude - Convenient imports for common use
// ============================================================================

/// Prelude module for convenient importing of common traits and types
///
/// ## Example
///
/// ```
/// use kama_core::traits::prelude::*;
/// use kama_core::ClockTick;
///
/// fn process<T: AudioNum, const BUF_SIZE: usize>(
///     source: &mut dyn Source<T, BUF_SIZE>,
///     processor: &mut dyn Processor<T, BUF_SIZE>,
///     sink: &mut dyn Sink<T, BUF_SIZE>
/// ) -> ProcessResult<()> {
///     let clock = ClockTick::default();
///     
///     // Создаем отдельные буферы для каждого этапа
///     let mut source_output = [T::ZERO; BUF_SIZE];
///     let mut processor_output = [T::ZERO; BUF_SIZE];
///     
///     // Source генерирует в source_output
///     let mut source_outputs = [&mut source_output];
///     source.generate(&clock, &[], &[], &mut source_outputs)?;
///     
///     // Processor читает из source_output и пишет в processor_output
///     let mut processor_outputs = [&mut processor_output];
///     processor.process(
///         &clock,
///         &[&source_output],  // неизменяемая ссылка на source_output
///         &[], &[], &[],
///         &mut processor_outputs,
///         &mut [], &mut [], &mut []
///     )?;
///     
///     // Sink читает из processor_output
///     sink.consume(
///         &clock,
///         &[&processor_output],  // неизменяемая ссылка на processor_output
///         &[], &[], &[],
///         &mut [], &mut []
///     )?;
///     
///     Ok(())
/// }
/// ```
pub mod prelude {
    // Re-export from parent modules
    pub use super::{
        // Core traits
        AudioNode, Source, Processor, Sink,
        
        // Error types
        ProcessResult, ProcessError,
        ParameterResult, ParameterError,
        
        // Node types
        NodeId, NodeMetadata, NodeCategory, NodeTypeId,
        
        // Parameter handling
        ParameterId, ParamValue, ParamType, ParamRange, ParamMetadata,
        
        // Ports
        PortId, PortType, PortDirection,
        
        // Constants
        DEFAULT_BLOCK_SIZE,
    };
    
    // Re-export AudioNum from math module for convenience
    pub use crate::math::AudioNum;
}

// ============================================================================
// Common Helper Traits
// ============================================================================

/// Trait for types that can be converted to/from `ParamValue`
///
/// This is useful for automatic parameter conversion in nodes.
pub trait IntoParamValue: Sized {
    /// Convert this value into a `ParamValue`
    fn into_param_value(self) -> ParamValue;
    
    /// Try to convert a `ParamValue` back into this type
    fn from_param_value(value: ParamValue) -> Option<Self>;
}

impl IntoParamValue for f32 {
    fn into_param_value(self) -> ParamValue {
        ParamValue::Float(self)
    }
    
    fn from_param_value(value: ParamValue) -> Option<Self> {
        value.as_f32()
    }
}

impl IntoParamValue for i32 {
    fn into_param_value(self) -> ParamValue {
        ParamValue::Int(self)
    }
    
    fn from_param_value(value: ParamValue) -> Option<Self> {
        value.as_i32()
    }
}

impl IntoParamValue for bool {
    fn into_param_value(self) -> ParamValue {
        ParamValue::Bool(self)
    }
    
    fn from_param_value(value: ParamValue) -> Option<Self> {
        value.as_bool()
    }
}

impl IntoParamValue for String {
    fn into_param_value(self) -> ParamValue {
        ParamValue::String(self)
    }
    
    fn from_param_value(value: ParamValue) -> Option<Self> {
        match value {
            ParamValue::String(s) => Some(s),
            ParamValue::Choice(s) => Some(s),
            _ => None,
        }
    }
}

// ============================================================================
// Blanket Implementations
// ============================================================================

// All types that implement AudioNode also implement the base traits
// This allows for polymorphism in the graph

/// Helper trait for downcasting to concrete types
pub trait AsAny: 'static {
    /// Convert to `&dyn std::any::Any`
    fn as_any(&self) -> &dyn std::any::Any;
    
    /// Convert to `&mut dyn std::any::Any`
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

impl<T: 'static> AsAny for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::prelude::*;
    use crate::traits::IntoParamValue;
    
    #[test]
    fn test_prelude_imports() {
        // Verify that all expected types are accessible
        let _node_id = NodeId(0);
        let _port_id = PortId::audio_in(_node_id, 0);
        let _param_id = ParameterId::new("test").unwrap();
        
        // Test IntoParamValue
        let f: f32 = 42.0;
        let pv = f.into_param_value();
        assert_eq!(pv.as_f32(), Some(42.0));
        
        let back = f32::from_param_value(pv);
        assert_eq!(back, Some(42.0));
    }
    
    #[test]
    fn test_param_value_conversions() {
        let f = ParamValue::Float(42.0);
        assert_eq!(f.as_f32(), Some(42.0));
        assert_eq!(f.as_i32(), Some(42));
        assert_eq!(f.as_bool(), Some(true));
        
        let i = ParamValue::Int(0);
        assert_eq!(i.as_f32(), Some(0.0));
        assert_eq!(i.as_i32(), Some(0));
        assert_eq!(i.as_bool(), Some(false));
        
        let b = ParamValue::Bool(true);
        assert_eq!(b.as_f32(), Some(1.0));
        assert_eq!(b.as_i32(), Some(1));
        assert_eq!(b.as_bool(), Some(true));
    }
    
    #[test]
    fn test_parameter_id_validation() {
        assert!(ParameterId::new("gain").is_ok());
        assert!(ParameterId::new("cutoff_freq").is_ok());
        assert!(ParameterId::new("delay_time_2").is_ok());
        
        assert!(ParameterId::new("").is_err());
        assert!(ParameterId::new("1gain").is_err());
        assert!(ParameterId::new("_gain").is_err());
        assert!(ParameterId::new("gain.value").is_err());
    }
    
    #[test]
    fn test_port_id_creation() {
        let node = NodeId(42);
        
        let audio_in = PortId::audio_in(node, 0);
        assert_eq!(audio_in.node_id(), node);
        assert_eq!(audio_in.port_type(), PortType::Audio);
        assert_eq!(audio_in.direction(), PortDirection::Input);
        assert_eq!(audio_in.index(), 0);
        assert!(audio_in.is_input());
        assert!(audio_in.is_audio());
        
        let clock_out = PortId::clock_out(node, 0);
        assert_eq!(clock_out.port_type(), PortType::Clock);
        assert!(clock_out.is_output());
        assert!(clock_out.is_clock());
        
        let feedback_in = PortId::feedback_in(node, 0);
        assert_eq!(feedback_in.port_type(), PortType::Feedback);
        assert!(feedback_in.is_input());
        assert!(feedback_in.is_feedback());
    }
    
    #[test]
    fn test_node_metadata() {
        let metadata = NodeMetadata {
            name: "TestNode".to_string(),
            category: NodeCategory::Processor,
            description: "A test node".to_string(),
            author: "Kama".to_string(),
            version: "1.0".to_string(),
            audio_inputs: 2,
            audio_outputs: 2,
            control_inputs: 1,
            control_outputs: 0,
            clock_inputs: 1,
            clock_outputs: 0,
            feedback_ports: 0,
            parameters: vec![],
        };
        
        assert_eq!(metadata.name, "TestNode");
        assert_eq!(metadata.category, NodeCategory::Processor);
        assert_eq!(metadata.audio_inputs, 2);
    }
    
    #[test]
    fn test_param_range() {
        let range = ParamRange::new()
            .with_min(0.0)
            .with_max(1.0)
            .with_step(0.1);
        
        assert!(range.contains(0.5));
        assert!(!range.contains(1.5));
        assert_eq!(range.clamp(1.5), 1.0);
        assert_eq!(range.clamp(-0.5), 0.0);
    }
    
    #[test]
    fn test_default_block_size() {
        assert_eq!(DEFAULT_BLOCK_SIZE, 64);
    }
}