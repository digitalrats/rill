//! # Core Traits for Rill
//!
//! This module defines the fundamental traits that form the backbone
//! of the Rill ecosystem.
/// Action trait and types for node‑level commands.
pub mod action;
/// Algorithm trait and action contexts.
pub mod algorithm;
/// Bridge backend trait for duplex execution boundary.
pub mod bridge;
/// BufferView trait for backend-specific ring buffer access.
pub mod buffer_view;
mod error;
/// MultichannelAlgorithm trait for multi-IO processing (N inputs, M outputs).
pub mod multichannel_algorithm;
/// NodeId type for identifying nodes.
pub mod node;
/// Parameter types and IDs (`ParameterId`, `ParamValue`, `ParamType`, etc.).
pub mod param;
/// ParameterWrite trait — polymorphic control interface for DSP engines.
pub mod parameter_write;
/// Rack archetype — modular processing unit (Eurorack case).
pub mod rack;
// Re-export all public items
pub use action::*;
pub use algorithm::*;
pub use buffer_view::*;
pub use error::*;
pub use multichannel_algorithm::*;
pub use node::*;
pub use param::*;
pub use parameter_write::*;
pub use rack::*;
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
        BufferView,
        Eurorack,
        ParamMetadata,
        ParamRange,
        ParamType,
        ParamValue,
        ParameterError,
        // Parameter handling
        ParameterId,
        ParameterResult,
        ParameterWrite,
        ProcessError,
        // Error types
        ProcessResult, // Constants
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
        assert!(ParameterId::new("gain.value").is_ok());
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
