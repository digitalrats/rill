//! # Kama Core
//!
//! The core of the Kama Audio ecosystem. Provides fundamental traits, types,
//! and utilities for building real-time audio applications.

//! # Kama Core

#![warn(missing_docs)]
#![allow(unsafe_code)]
#![cfg_attr(not(test), deny(unused))]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Core modules
pub mod traits;
pub mod math;
pub mod buffer;
pub mod queues;
pub mod time;

// Macros for node creation
#[macro_use]
pub mod macros;

// Convenience prelude
pub mod prelude;

// Error types
mod error;
pub use error::*;

// Re-export commonly used items
pub use traits::{
    Source, Processor, Sink,
    NodeId, ParameterId, PortId, PortType,
};

pub use math::AudioNum;

pub use buffer::{
    PipeBuffer, FanOutBuffer, FanInBuffer, DelayLine, RingBuffer,
    AudioBuffer, BufferStats,
};

pub use queues::{
    CommandQueue, CommandEnum, SetParameter, TelemetryQueue, Telemetry,
    MicroControlObserver, MicroControlPermit, SignalSource,
};

pub use time::{
    Clock, TimeProvider, SystemClock, TickInfo,
};

/// Current version of kama-core
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Maximum supported sample rate
pub const MAX_SAMPLE_RATE: f32 = 384_000.0;

/// Minimum supported sample rate
pub const MIN_SAMPLE_RATE: f32 = 8_000.0;

/// Default block size for audio processing
pub const DEFAULT_BLOCK_SIZE: usize = 64;

/// Maximum block size
pub const MAX_BLOCK_SIZE: usize = 8192;

/// Minimum block size
pub const MIN_BLOCK_SIZE: usize = 16;

/// Default sample rate (44.1 kHz)
pub const DEFAULT_SAMPLE_RATE: f32 = 44_100.0;

#[cfg(test)]
mod tests {
    use super::*;  // This imports all constants from the parent module

    #[test]
    fn test_constants() {
        assert!(!VERSION.is_empty());
        assert!(MAX_SAMPLE_RATE > MIN_SAMPLE_RATE);
        assert!(MAX_BLOCK_SIZE > MIN_BLOCK_SIZE);
        assert_eq!(DEFAULT_BLOCK_SIZE, 64);
        assert_eq!(DEFAULT_SAMPLE_RATE, 44100.0);
    }
}