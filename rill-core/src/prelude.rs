//! # Rill Core Prelude
//!
//! This module re-exports the most commonly used types and traits from rill-core.
//! Import it with `use rill_core::prelude::*;` to get access to all essential
//! items for working with the Rill ecosystem.
//!
//! ## What's included
//!
//! - Core traits (`SignalNode`, `Source`, `Processor`, `Sink`)
//! - Node identification (`NodeId`, `NodeMetadata`, `NodeCategory`)
//! - Parameter handling (`ParameterId`, `ParamValue`, `ParamType`)
//! - Ports (`PortId`, `PortType`, `PortDirection`)
//! - Time and clock (`ClockTick`, `ClockSource`, `SystemClock`)
//! - Error types (`ProcessResult`, `ProcessError`, etc.)
//! - Buffer types (`PipeBuffer`, `FanOutBuffer`, `DelayLine`, `RingBuffer`)
//! - Atomic types (`AtomicCell`, `AtomicStats`)
//! - Math abstractions (`Scalar`, `Transcendental`)
//! - Constants (`DEFAULT_BLOCK_SIZE`, `MAX_SAMPLE_RATE`, etc.)
//!
//! ## Example
//!
// В rill-core/src/lib.rs - строка 151 (пример prelude)

//! ## Example
//!
//! ```rust
//! use rill_core::prelude::*;
//!
//! fn process_block<T: Transcendental, const BUF_SIZE: usize>(
//!     source: &mut dyn Source<T, BUF_SIZE>,
//!     processor: &mut dyn Processor<T, BUF_SIZE>,
//!     sink: &mut dyn Sink<T, BUF_SIZE>,
//!     clock: &ClockTick,
//! ) -> ProcessResult<()> {
//!     source.generate(clock, &[], &[])?;
//!     processor.process(clock, &[], &[], &[], &[])?;
//!     sink.consume(clock, &[], &[], &[], &[])?;
//!     Ok(())
//! }
//! ```

// ============================================================================
// Core Traits
// ============================================================================

pub use crate::traits::{
    Action,
    ActionContext,

    // Algorithm / Action
    Algorithm,
    AlgorithmCategory,
    AlgorithmMetadata,
    ConnectionError,
    ConnectionResult,
    // Parameter conversion
    IntoParamValue,

    NodeCategory,
    // Node identification
    NodeId,
    NodeMetadata,
    NodeParams,
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
    PortError,
    // Ports
    PortId,
    PortResult,
    PortType,
    ProcessError,
    // Error handling
    ProcessResult,
    Processor,
    // Core node traits
    SignalNode,
    Sink,

    Source,
};

// ============================================================================
// Time and Clock
// ============================================================================

pub use crate::time::{ClockSource, ClockTick, SystemClock, TimeError, TimeResult};

// ============================================================================
// Math Abstractions
// ============================================================================

pub use crate::interpolate::Interpolate;

pub use crate::math::Transcendental;

// ============================================================================
// Vector Types (SIMD abstractions)
// ============================================================================

pub use crate::math::vector::math::{
    abs_slice, clamp_slice, cos_slice, exp_slice, ln_slice, max_slice, min_slice, sin_slice,
    sqrt_slice, tan_slice,
};
pub use crate::math::vector::ops::{
    add_scalar_slice, add_slices, div_slices, mul_scalar_slice, mul_slices, sub_slices,
};
pub use crate::math::vector::scalar::{ScalarVector1, ScalarVector2, ScalarVector4, ScalarVector8};
#[cfg(feature = "simd")]
pub use crate::math::vector::simd::*;
pub use crate::math::vector::traits::{
    Vector, VectorMask, VectorReduce, VectorScalarOps, VectorTranscendental,
};

// ============================================================================
// Buffer Types
// ============================================================================

pub use crate::buffer::{
    // Utility functions
    utils,
    // Atomic types
    AtomicCell,
    AtomicCellError,
    AtomicStats,

    // Port buffer
    Buffer,

    // Error types
    BufferError,
    BufferResult,

    // Statistics
    BufferStats,

    DelayLine,
    FanInBuffer,
    FanOutBuffer,
    // Buffer implementations
    PipeBuffer,
    RingBuffer,
    // Core buffer trait
    SignalBuffer,
};

// ============================================================================
// Queue Types (from rill-patchbay integration)
// ============================================================================

pub use crate::queues::{QueueError, QueueResult, TelemetryBlock};

// ============================================================================
// Constants
// ============================================================================

pub use crate::{
    // Cache line alignment
    CACHE_LINE_SIZE,
    // Block sizes
    DEFAULT_BLOCK_SIZE,
    // Buffer sizes
    DEFAULT_BUFFER_SIZE,
    DEFAULT_SAMPLE_RATE,

    MAX_BLOCK_SIZE,
    MAX_BUFFER_SIZE,
    // Sample rates
    MAX_SAMPLE_RATE,
    MIN_BLOCK_SIZE,

    MIN_BUFFER_SIZE,

    MIN_SAMPLE_RATE,
    // Version
    VERSION,
};

// ============================================================================
// Common Type Aliases
// ============================================================================

/// Default sample type (32-bit float)
pub type Sample = f32;

/// Mono signal block type
pub type MonoBlock<T, const N: usize> = [T; N];

/// Stereo signal block type (left, right)
pub type StereoBlock<T, const N: usize> = [MonoBlock<T, N>; 2];

/// Control signal value type
pub type ControlValue<T> = T;

/// Default pipe buffer with f32 samples
pub type DefaultPipeBuffer<const N: usize = DEFAULT_BLOCK_SIZE> = PipeBuffer<Sample, N>;

/// Default delay line with f32 samples
pub type DefaultDelayLine<const MAX_DELAY: usize> = DelayLine<Sample, MAX_DELAY>;

/// Default ring buffer with f32 samples
pub type DefaultRingBuffer<const N: usize> = RingBuffer<Sample, N>;

/// Default system clock
pub type DefaultClock = SystemClock;

// ============================================================================
// Specialized Preludes for Different Use Cases
// ============================================================================

/// Prelude for working with f32 samples (common case)
pub mod f32_prelude {
    use crate::buffer::{DelayLine, FanInBuffer, FanOutBuffer, PipeBuffer, RingBuffer};

    /// Pipe buffer with f32 samples
    pub type PipeBufferF32<const N: usize> = PipeBuffer<f32, N>;

    /// Fan-out buffer with f32 samples
    pub type FanOutBufferF32<const N: usize, const CONSUMERS: usize> =
        FanOutBuffer<f32, N, CONSUMERS>;

    /// Fan-in buffer with f32 samples
    pub type FanInBufferF32<const N: usize, const PRODUCERS: usize> =
        FanInBuffer<f32, N, PRODUCERS>;

    /// Delay line with f32 samples
    pub type DelayLineF32<const MAX_DELAY: usize> = DelayLine<f32, MAX_DELAY>;

    /// Ring buffer with f32 samples
    pub type RingBufferF32<const N: usize> = RingBuffer<f32, N>;

    /// System clock for f32 (same as default)
    pub type SystemClockF32 = crate::time::SystemClock;

    // Re-export traits
    pub use crate::traits::{Processor as ProcessorF32, Sink as SinkF32, Source as SourceF32};

    pub use crate::math::Transcendental;
}

/// Prelude for working with f64 samples (high precision)
pub mod f64_prelude {
    use crate::buffer::{DelayLine, FanInBuffer, FanOutBuffer, PipeBuffer, RingBuffer};

    /// Pipe buffer with f64 samples
    pub type PipeBufferF64<const N: usize> = PipeBuffer<f64, N>;

    /// Fan-out buffer with f64 samples
    pub type FanOutBufferF64<const N: usize, const CONSUMERS: usize> =
        FanOutBuffer<f64, N, CONSUMERS>;

    /// Fan-in buffer with f64 samples
    pub type FanInBufferF64<const N: usize, const PRODUCERS: usize> =
        FanInBuffer<f64, N, PRODUCERS>;

    /// Delay line with f64 samples
    pub type DelayLineF64<const MAX_DELAY: usize> = DelayLine<f64, MAX_DELAY>;

    /// Ring buffer with f64 samples
    pub type RingBufferF64<const N: usize> = RingBuffer<f64, N>;

    /// System clock for f64 (same as default)
    pub type SystemClockF64 = crate::time::SystemClock;

    // Re-export traits
    pub use crate::traits::{Processor as ProcessorF64, Sink as SinkF64, Source as SourceF64};

    pub use crate::math::Transcendental;
}

/// Prelude for working with time
pub mod time_prelude {
    pub use crate::time::{ClockSource, ClockTick, SystemClock, TimeError, TimeResult};
}

/// Prelude for working with buffers
pub mod buffer_prelude {
    pub use crate::buffer::{
        utils, AtomicCell, AtomicStats, BufferError, BufferResult, BufferStats, DelayLine,
        FanInBuffer, FanOutBuffer, PipeBuffer, RingBuffer, SignalBuffer,
    };
}

/// Prelude for working with queues (automation)
pub mod queue_prelude {
    pub use crate::queues::{QueueError, QueueResult};
}

/// Prelude for working with parameters
pub mod param_prelude {
    pub use crate::traits::{
        IntoParamValue, ParamMetadata, ParamRange, ParamType, ParamValue, ParameterError,
        ParameterId, ParameterResult,
    };
}

/// Prelude for working with ports
pub mod port_prelude {
    pub use crate::traits::{PortDirection, PortError, PortId, PortResult, PortType};
}

/// Prelude for working with nodes
pub mod node_prelude {
    pub use crate::traits::{
        NodeCategory, NodeId, NodeMetadata, NodeTypeId, Processor, SignalNode, Sink, Source,
    };
}

// ============================================================================
// Re-export of commonly used items from other crates
// ============================================================================

/// Common third-party types that are frequently used with Rill
pub mod external {
    pub use std::f32::consts::PI;
    pub use std::f64::consts::PI as PI_F64;
}

// ============================================================================
// Helper macros for common operations
// ============================================================================

/// Macro for creating a mono block from a slice
///
/// # Example
/// ```
/// use rill_core::mono_block;
///
/// let data = vec![1.0, 2.0, 3.0];
/// let block = mono_block!(data, 64);
/// ```
#[macro_export]
macro_rules! mono_block {
    ($data:expr, $size:expr) => {{
        let mut block = [0.0; $size];
        let len = $data.len().min($size);
        block[..len].copy_from_slice(&$data[..len]);
        block
    }};
}

/// Macro for creating a stereo block from slices
///
/// # Example
/// ```
/// use rill_core::stereo_block;
///
/// let left = vec![1.0; 64];
/// let right = vec![2.0; 64];
/// let block = stereo_block!(left, right, 64);
/// ```
#[macro_export]
macro_rules! stereo_block {
    ($left:expr, $right:expr, $size:expr) => {{
        let mut left_block = [0.0; $size];
        let mut right_block = [0.0; $size];

        let left_len = $left.len().min($size);
        left_block[..left_len].copy_from_slice(&$left[..left_len]);

        let right_len = $right.len().min($size);
        right_block[..right_len].copy_from_slice(&$right[..right_len]);

        [left_block, right_block]
    }};
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prelude_imports() {
        // Verify that all expected types are accessible
        let _node_id = NodeId(0);
        let _port_id = PortId::signal_in(_node_id, 0);
        let _param_id = ParameterId::new("test").unwrap();
        let _clock = SystemClock::with_sample_rate(44100.0);
        let _tick = ClockTick::new(0, 64, 44100.0);

        // Test buffer creation
        let _pipe = PipeBuffer::<f32, 64>::new();
        let _delay = DelayLine::<f32, 1024>::new(44100.0);
        let _ring = RingBuffer::<f32, 256>::new();

        // Test atomic cell
        let _cell = AtomicCell::new(42);
    }

    #[test]
    fn test_type_aliases() {
        let _pipe = DefaultPipeBuffer::<64>::new();
        let _delay = DefaultDelayLine::<1024>::new(44100.0);
        let _ring = DefaultRingBuffer::<256>::new();
        let _clock = DefaultClock::with_sample_rate(44100.0);
    }

    #[test]
    fn test_f32_prelude() {
        use f32_prelude::*;

        let _pipe = PipeBufferF32::<64>::new();
        let _fan_out = FanOutBufferF32::<64, 4>::new();
        let _fan_in = FanInBufferF32::<64, 2>::new();
        let _delay = DelayLineF32::<1024>::new(44100.0);
        let _ring = RingBufferF32::<256>::new();
        let _clock = SystemClockF32::with_sample_rate(44100.0);
    }

    #[test]
    fn test_f64_prelude() {
        use f64_prelude::*;

        let _pipe = PipeBufferF64::<64>::new();
        let _fan_out = FanOutBufferF64::<64, 4>::new();
        let _fan_in = FanInBufferF64::<64, 2>::new();
        let _delay = DelayLineF64::<1024>::new(44100.0);
        let _ring = RingBufferF64::<256>::new();
        let _clock = SystemClockF64::with_sample_rate(44100.0);
    }

    #[test]
    fn test_time_prelude() {
        use time_prelude::*;

        let mut clock = SystemClock::with_sample_rate(44100.0);
        let tick = clock.next_tick(64);
        let _pos = tick.absolute_seconds();
    }

    #[test]
    fn test_buffer_prelude() {
        use buffer_prelude::*;

        let buffer = PipeBuffer::<f32, 64>::new();
        let stats = buffer.stats();
        let _fill = stats.fill_level;
    }

    #[test]
    fn test_param_prelude() {
        use param_prelude::*;

        let _id = ParameterId::new("gain").unwrap();
        let value = ParamValue::Float(0.5);
        let _type = value.param_type();
    }

    #[test]
    fn test_port_prelude() {
        use port_prelude::*;

        let port = PortId::signal_in(NodeId(0), 0);
        assert!(port.is_signal());
        assert!(port.is_input());
    }

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_BLOCK_SIZE, 64);
        assert_eq!(MAX_SAMPLE_RATE, 384_000.0);
        assert_eq!(MIN_SAMPLE_RATE, 8_000.0);
        assert_eq!(CACHE_LINE_SIZE, 64);
    }

    #[test]
    fn test_macros() {
        let data = vec![1.0, 2.0, 3.0];
        let block = mono_block!(data, 4);
        assert_eq!(block, [1.0, 2.0, 3.0, 0.0]);

        let left = vec![1.0; 4];
        let right = vec![2.0; 4];
        let stereo = stereo_block!(left, right, 4);
        assert_eq!(stereo[0], [1.0; 4]);
        assert_eq!(stereo[1], [2.0; 4]);
    }

    #[test]
    fn test_into_param_value() {
        let f: f32 = 42.0;
        let pv = f.into_param_value();
        assert_eq!(pv.as_f32(), Some(42.0));

        let i: i32 = 42;
        let pv = i.into_param_value();
        assert_eq!(pv.as_i32(), Some(42));

        let b = true;
        let pv = b.into_param_value();
        assert_eq!(pv.as_bool(), Some(true));
    }
}
