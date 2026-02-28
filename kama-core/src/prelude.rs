//! # Kama Core Prelude
//!
//! This module re-exports the most commonly used types and traits from kama-core.
//! Import it with `use kama_core::prelude::*;` to get access to all essential
//! items for working with the Kama Audio ecosystem.

// Re-export core traits
pub use crate::traits::{
    // Core node traits
    Source, Processor, Sink,
    
    // Node identification
    NodeId, NodeMetadata, NodeCategory, NodeTypeId,
    
    // Parameter handling
    ParameterId, ParamValue, ParamType, ParamRange, ParamMetadata,
    
    // Ports
    PortId, PortType,
};

// Re-export error types from crate root (not from traits!)
pub use crate::{
    ProcessResult, ProcessError,
    ParameterError, ParameterResult,
    BufferError, BufferResult,
    QueueError, QueueResult,
};

// Re-export time abstractions
pub use crate::time::{
    Clock, TimeProvider, TickInfo, SystemClock,
};

// Re-export math abstractions
pub use crate::math::AudioNum;

// Re-export buffer types
pub use crate::buffer::{
    AudioBuffer, BufferStats,
    PipeBuffer, FanOutBuffer, FanInBuffer, DelayLine, RingBuffer,
};

// Re-export queue types
pub use crate::queues::{
    CommandQueue, CommandEnum, SetParameter, TelemetryQueue, Telemetry,
    MicroControlObserver, MicroControlPermit, SignalSource,
};

// Re-export common constants
pub use crate::{
    VERSION, MAX_SAMPLE_RATE, MIN_SAMPLE_RATE, DEFAULT_BLOCK_SIZE, DEFAULT_SAMPLE_RATE,
};

/// Common type aliases for convenience
pub mod aliases {
    use super::*;
    
    /// Default sample type (32-bit float)
    pub type Sample = f32;
    
    /// Default block size (64 samples)
    pub const BLOCK_SIZE: usize = 64;
    
    /// Default sample rate (44.1 kHz)
    pub const SAMPLE_RATE: f32 = 44_100.0;
    
    /// Type alias for a PipeBuffer with default sample type
    pub type DefaultPipeBuffer<const N: usize = BLOCK_SIZE> = PipeBuffer<Sample, N>;
    
    /// Type alias for a DelayLine with default sample type
    pub type DefaultDelayLine<const MAX_DELAY: usize> = DelayLine<Sample, MAX_DELAY>;
    
    /// Type alias for a RingBuffer with default sample type
    pub type DefaultRingBuffer<const N: usize> = RingBuffer<Sample, N>;
    
    /// Type alias for SystemClock
    pub type DefaultClock = SystemClock;
}

/// Prelude for working with f32 samples (common case)
pub mod f32_prelude {
    use crate::buffer::PipeBuffer;
    
    // Fixed: PipeBuffer takes 2 generic parameters: type and size
    pub type PipeBufferF32<const N: usize> = PipeBuffer<f32, N>;
    pub type FanOutBufferF32<const N: usize, const CONSUMERS: usize> = crate::buffer::FanOutBuffer<f32, N, CONSUMERS>;
    pub type FanInBufferF32<const N: usize, const PRODUCERS: usize> = crate::buffer::FanInBuffer<f32, N, PRODUCERS>;
    pub type DelayLineF32<const MAX_DELAY: usize> = crate::buffer::DelayLine<f32, MAX_DELAY>;
    pub type RingBufferF32<const N: usize> = crate::buffer::RingBuffer<f32, N>;
    
    pub use crate::time::SystemClock as SystemClockF32;
    pub use crate::traits::{
        Source as SourceF32, Processor as ProcessorF32, Sink as SinkF32,
    };
}

/// Prelude for working with f64 samples (high precision)
pub mod f64_prelude {
    pub use crate::traits::{
        Source as SourceF64, Processor as ProcessorF64, Sink as SinkF64,
    };
    
    pub use crate::buffer::{
        PipeBuffer as PipeBufferF64,
        FanOutBuffer as FanOutBufferF64,
        FanInBuffer as FanInBufferF64,
        DelayLine as DelayLineF64,
        RingBuffer as RingBufferF64,
    };
    
    pub use crate::time::SystemClock as SystemClockF64;
    pub use crate::math::AudioNum;
}

/// Prelude for working with time
pub mod time_prelude {
    pub use crate::time::{
        Clock, TimeProvider, SystemClock, TickInfo,
    };
}

/// Prelude for working with buffers
pub mod buffer_prelude {
    pub use crate::buffer::{
        AudioBuffer, BufferStats,
        PipeBuffer, FanOutBuffer, FanInBuffer, DelayLine, RingBuffer,
    };
}

/// Prelude for working with queues (automation)
pub mod queue_prelude {
    pub use crate::queues::{
        CommandQueue, CommandEnum, SetParameter, TelemetryQueue, Telemetry,
        MicroControlObserver, MicroControlPermit, SignalSource,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_aliases() {
        use aliases::*;
        
        let _pipe = DefaultPipeBuffer::<64>::new();
        let _delay = DefaultDelayLine::<1024>::new(44100.0);
        let _ring = DefaultRingBuffer::<256>::new();
        let _clock = DefaultClock::new(44100.0, 120.0);
    }
    
    #[test]
    fn test_f32_prelude() {
        use f32_prelude::*;
        
        let _pipe = PipeBufferF32::<64>::new();
        let _clock = SystemClockF32::new(44100.0, 120.0);
    }
    
    #[test]
    fn test_error_types() {
        // Verify that error types are accessible
        let _proc_err: ProcessError = ProcessError::processing("test");
        let _param_err: ParameterError = ParameterError::not_found("gain");
        let _buf_err: BufferError = BufferError::Empty;
        let _queue_err: QueueError = QueueError::QueueEmpty;
    }
}