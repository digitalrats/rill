//! # Core Traits for Rill
//!
//! This module defines the fundamental traits that form the backbone
//! of the Rill ecosystem.

pub mod action;
pub mod algorithm;
mod error;
pub mod node;
pub mod param;
pub mod port;
pub mod processable;

// Re-export all public items
pub use action::*;
pub use algorithm::*;
pub use error::*;
pub use node::*;
pub use param::*;
pub use port::*;
pub use processable::*;

// ============================================================================
// Common Type Aliases
// ============================================================================

/// Default block size for signal processing
pub const DEFAULT_BLOCK_SIZE: usize = 64;

/// Type alias for a mono signal block
pub type MonoBlock<T, const BUF_SIZE: usize> = [T; BUF_SIZE];

/// Type alias for a stereo signal block (left, right)
pub type StereoBlock<T, const BUF_SIZE: usize> = [MonoBlock<T, BUF_SIZE>; 2];

/// Type alias for a control signal value
pub type ControlValue<T> = T;

// ============================================================================
// Prelude - Convenient imports for common use
// ============================================================================

/// Prelude module for convenient importing of common traits and types
pub mod prelude {
    // Re-export from parent modules
    pub use super::{
        // Core traits
        SignalNode,
        NodeCategory,
        // Node types
        NodeId,
        NodeMetadata,
        NodeState,

        NodeTypeId,
        ParamMetadata,

        ParamRange,
        ParamType,
        ParamValue,
        ParameterError,

        // Parameter handling
        ParameterId,
        ParameterResult,
        Port,

        PortDirection,
        // Ports
        PortId,
        PortType,
        ProcessError,
        // Error types
        ProcessResult,
        Processor,
        Sink,

        Source,
        // Constants
        DEFAULT_BLOCK_SIZE,
    };

    // Re-export Transcendental from math module for convenience
    pub use crate::math::Transcendental;
}

// ============================================================================
// Common Helper Traits
// ============================================================================

/// Trait for types that can be converted to/from `ParamValue`
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
        assert_eq!(audio_in.port_type(), PortType::Signal);
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
            type_name: None,
            category: NodeCategory::Processor,
            description: "A test node".to_string(),
            author: "Rill".to_string(),
            version: "1.0".to_string(),
            signal_inputs: 2,
            signal_outputs: 2,
            control_inputs: 1,
            control_outputs: 0,
            clock_inputs: 1,
            clock_outputs: 0,
            feedback_ports: 0,
            parameters: vec![],
        };

        assert_eq!(metadata.name, "TestNode");
        assert_eq!(metadata.category, NodeCategory::Processor);
        assert_eq!(metadata.signal_inputs, 2);
    }

    #[test]
    fn test_param_range() {
        let range = ParamRange::new().with_min(0.0).with_max(1.0).with_step(0.1);

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
