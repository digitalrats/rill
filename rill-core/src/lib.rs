//! # Rill Core
//!
//! The core of the Rill ecosystem. Provides fundamental traits, types,
//! and utilities for building real-time signal processing applications.
//!
//! ## Architecture Overview
//!
//! ```text
//! rill-core/
//! ├── traits/           # Core traits (Node, Source, Processor, Sink, etc.)
//! ├── math/             # Mathematical abstractions (Scalar, Transcendental, Vector)
//! │   └── vector/       # Vector types, SIMD abstractions, slice operations
//! ├── buffer/           # Lock-free signal buffers with AtomicCell safety
//! ├── queues/           # Real-time safe command queues
//! ├── time/             # Time and clock abstractions (ClockTick, SystemClock)
//! ├── io/               # Generic I/O backend trait (IoBackend)
//! ├── macros/           # Node creation macros (source_node!, processor_node!, etc.)
//! ├── prelude           # Convenience prelude for common imports
//! ├── interpolate       # Fractional-index interpolation trait
//! └── executor/         # Graph executor for driving signal processing
//! ```
//!
//! ## Key Concepts
//!
//! - **Scalar**: Base numeric trait for any type (floats and integers)
//! - **Transcendental**: Float numeric abstraction with sin/cos/sqrt
//! - **AtomicCell**: Safe atomic wrapper for lock-free data structures
//! - **Node**: Base trait for all nodes in the signal graph
//! - **Source**: Active generators (oscillators, file readers)
//! - **Processor**: Passive processors (filters, effects)
//! - **Sink**: Active outputs (I/O devices, file writers)
//! - **PipeBuffer**: Zero-copy connections between nodes
//! - **CommandQueue**: Real-time safe parameter automation
//! - **ClockTick**: Sample-accurate timing for synchronization
//!
//! ## Example
//!
//! ```rust
//! use rill_core::prelude::*;
//! use rill_core::Port;
//! use rill_core::traits::node;
//!
//! // Create a simple sine source
//! struct MySine<T: Transcendental, const BUF_SIZE: usize> {
//!     frequency: T,
//!     amplitude: T,
//!     phase: T,
//!     sample_rate: T,
//! }
//!
//! impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for MySine<T, BUF_SIZE> {
//!     fn metadata(&self) -> NodeMetadata {
//!         NodeMetadata {
//!             name: "Sine".to_string(),
//!             type_name: None,
//!             category: NodeCategory::Source,
//!             description: "Sine wave oscillator".to_string(),
//!             author: "Rill".to_string(),
//!             version: env!("CARGO_PKG_VERSION").to_string(),
//!             signal_inputs: 0,
//!             signal_outputs: 1,
//!             control_inputs: 0,
//!             control_outputs: 0,
//!             clock_inputs: 1,
//!             clock_outputs: 0,
//!             feedback_ports: 0,
//!             parameters: vec![],
//!         }
//!     }
//!     
//!     fn init(&mut self, sample_rate: f32) {
//!         self.sample_rate = T::from_f32(sample_rate);
//!     }
//!     
//!     fn reset(&mut self) {
//!         self.phase = T::ZERO;
//!     }
//!     
//!     fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
//!         None
//!     }
//!     
//!     fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
//!         Ok(())
//!     }
//!     
//!     fn id(&self) -> NodeId { NodeId(0) }
//!     fn set_id(&mut self, _id: NodeId) {}
//!     
//!     fn input_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
//!     fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
//!     fn output_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
//!     fn output_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
//!     fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
//!     fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
//!     
//!     fn state(&self) -> &node::NodeState<T,BUF_SIZE> {
//!         unimplemented!()
//!     }
//!     
//!     fn state_mut(&mut self) -> &mut node::NodeState<T,BUF_SIZE> {
//!         unimplemented!()
//!     }
//! }
//!
//! impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE> for MySine<T, BUF_SIZE> {
//!     fn generate(
//!         &mut self,
//!         clock: &ClockTick,
//!         _control_inputs: &[T],
//!         _clock_inputs: &[ClockTick],
//!     ) -> ProcessResult<()> {
//!         let two_pi = T::from_f32(2.0 * std::f32::consts::PI);
//!         let phase_inc = self.frequency / T::from_f32(clock.sample_rate);
//!         let amp = self.amplitude;
//!         
//!         let mut temp = [T::ZERO; BUF_SIZE];
//!         for i in 0..BUF_SIZE {
//!             let phase_rad = self.phase * two_pi;
//!             temp[i] = phase_rad.sin() * amp;
//!             self.phase = self.phase + phase_inc;
//!             if self.phase >= T::from_f32(1.0) {
//!                 self.phase = self.phase - T::from_f32(1.0);
//!             }
//!         }
//!         *self.output_port_mut(0).unwrap().buffer.as_mut_array() = temp;
//!         Ok(())
//!     }
//!     
//!     fn num_signal_outputs(&self) -> usize { 1 }
//!     fn num_control_inputs(&self) -> usize { 0 }
//!     fn num_clock_inputs(&self) -> usize { 1 }
//! }
//! ```

#![warn(missing_docs)]
#![allow(clippy::doc_lazy_continuation)]
#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(unused))]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(deprecated)]

// ============================================================================
// Core Modules
// ============================================================================

/// Core traits for the Rill ecosystem
pub mod traits;

/// Mathematical abstractions for signal processing
pub mod math;

/// Lock-free, real-time safe signal buffers
pub mod buffer;

/// Real-time safe command queues for automation
pub mod queues;

/// Time and clock abstractions for synchronization
pub mod time;

#[doc(hidden)]
pub use math::vector;

/// Macros for node creation and boilerplate reduction
#[macro_use]
pub mod macros;

/// Convenience prelude for importing common types
pub mod prelude;

/// Fractional-index interpolation trait for slice-like types
pub mod interpolate;

/// Generic multi-channel signal I/O abstraction
pub mod io;

/// Graph executor for driving signal processing
pub mod executor;

// ============================================================================
// Error Types
// ============================================================================

/// Core error types for the Rill ecosystem
mod error;
pub use error::*;

// ============================================================================
// Re-exports for Convenience
// ============================================================================

// Re-export core traits
pub use traits::{
    ClockError, ClockResult, ConnectionError, ConnectionResult, Eurorack, Node, NodeCategory,
    NodeId, NodeMetadata, NodeState, NodeTypeId, ParamMetadata, ParamRange, ParamType, ParamValue,
    ParameterError, ParameterId, Params, Port, PortDirection, PortError, PortId, PortResult,
    PortType, ProcessError, ProcessResult, Processor, Sink, Source,
};

// Re-export math abstractions
pub use math::{Scalar, Transcendental};

// Re-export buffer types with AtomicCell safety
pub use buffer::{
    AtomicCell, AtomicCellError, AtomicStats, Buffer, BufferError, BufferResult, BufferStats,
    DelayLine, FanInBuffer, FanOutBuffer, PipeBuffer, RingBuffer,
};

// Re-export queue types (from rill-patchbay integration)
pub use queues::{QueueError, QueueResult};

// Re-export time abstractions
pub use time::{ClockSource, ClockTick, SystemClock};

// ============================================================================
// Constants
// ============================================================================

/// Current version of rill-core
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Maximum supported sample rate
pub const MAX_SAMPLE_RATE: f32 = 384_000.0;

/// Minimum supported sample rate
pub const MIN_SAMPLE_RATE: f32 = 8_000.0;

/// Default sample rate (44.1 kHz)
pub const DEFAULT_SAMPLE_RATE: f32 = 44_100.0;

/// Default block size for signal processing
pub const DEFAULT_BLOCK_SIZE: usize = 64;

/// Maximum block size
pub const MAX_BLOCK_SIZE: usize = 8192;

/// Minimum block size
pub const MIN_BLOCK_SIZE: usize = 16;

/// Default buffer size for most use cases
pub const DEFAULT_BUFFER_SIZE: usize = 1024;

/// Maximum buffer size (2^16 = 65536 samples)
pub const MAX_BUFFER_SIZE: usize = 65536;

/// Minimum buffer size
pub const MIN_BUFFER_SIZE: usize = 16;

/// Cache line size for alignment (64 bytes on x86_64)
pub const CACHE_LINE_SIZE: usize = 64;

// ============================================================================
// Utility Functions
// ============================================================================

/// Utility functions for common operations
pub mod utils {
    use crate::math::Transcendental;

    /// Convert seconds to samples
    #[inline(always)]
    pub fn seconds_to_samples(seconds: f32, sample_rate: f32) -> usize {
        (seconds * sample_rate) as usize
    }

    /// Convert samples to seconds
    #[inline(always)]
    pub fn samples_to_seconds(samples: usize, sample_rate: f32) -> f32 {
        samples as f32 / sample_rate
    }

    /// Convert dB to linear gain
    #[inline(always)]
    pub fn db_to_linear<T: Transcendental>(db: T) -> T {
        T::from_f32(10.0_f32.powf(db.to_f32() / 20.0))
    }

    /// Convert linear gain to dB
    #[inline(always)]
    pub fn linear_to_db<T: Transcendental>(linear: T) -> T {
        T::from_f32(20.0 * linear.to_f32().log10())
    }

    /// Check if a value is a power of two
    #[inline(always)]
    pub const fn is_power_of_two(x: usize) -> bool {
        x != 0 && (x & (x - 1)) == 0
    }

    /// Round up to the next power of two
    #[inline(always)]
    pub const fn next_power_of_two(x: usize) -> usize {
        let mut n = x - 1;
        n |= n >> 1;
        n |= n >> 2;
        n |= n >> 4;
        n |= n >> 8;
        n |= n >> 16;
        n + 1
    }
}

// ============================================================================
// Version Information
// ============================================================================

/// Get detailed version information
pub fn version_info() -> VersionInfo {
    VersionInfo {
        version: VERSION,
        crate_name: env!("CARGO_PKG_NAME"),
        authors: env!("CARGO_PKG_AUTHORS"),
        description: env!("CARGO_PKG_DESCRIPTION"),
        repository: env!("CARGO_PKG_REPOSITORY"),
    }
}

/// Detailed version information for the rill-core crate.
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// Crate version string (from `CARGO_PKG_VERSION`).
    pub version: &'static str,
    /// Crate name (from `CARGO_PKG_NAME`).
    pub crate_name: &'static str,
    /// Author list (from `CARGO_PKG_AUTHORS`).
    pub authors: &'static str,
    /// Crate description (from `CARGO_PKG_DESCRIPTION`).
    pub description: &'static str,
    /// Repository URL (from `CARGO_PKG_REPOSITORY`).
    pub repository: &'static str,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::prelude::*;
    use super::utils;

    #[test]
    fn test_constants() {
        assert!(!VERSION.is_empty());
        assert!(MAX_SAMPLE_RATE > MIN_SAMPLE_RATE);
        assert!(MAX_BLOCK_SIZE > MIN_BLOCK_SIZE);
        assert_eq!(DEFAULT_BLOCK_SIZE, 64);
        assert_eq!(DEFAULT_SAMPLE_RATE, 44100.0);
        assert_eq!(CACHE_LINE_SIZE, 64);
    }

    #[test]
    fn test_utils() {
        assert_eq!(utils::seconds_to_samples(1.0, 44100.0), 44100);
        assert!((utils::samples_to_seconds(44100, 44100.0) - 1.0).abs() < 1e-6);

        let linear = utils::db_to_linear(0.0f32);
        assert!((linear - 1.0).abs() < 1e-6);

        let db = utils::linear_to_db(1.0f32);
        assert!((db - 0.0).abs() < 1e-6);

        assert!(utils::is_power_of_two(64));
        assert!(!utils::is_power_of_two(63));
        assert_eq!(utils::next_power_of_two(63), 64);
    }

    #[test]
    fn test_atomic_cell() {
        let cell = AtomicCell::new(42);
        assert_eq!(cell.load(), 42);
        cell.store(100);
        assert_eq!(cell.load(), 100);
    }
}

// ============================================================================
// Documentation Tests
// ============================================================================

#[cfg(doctest)]
mod doctests {
    //! This module exists only to host documentation tests
}
