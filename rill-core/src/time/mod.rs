//! # Time and clock abstractions for Rill
//!
//! This module provides timing and synchronization primitives for real-time
//! signal processing. It includes clock sources, tick information, and
//! utilities for sample-accurate timing.
//!
//! ## Key Components
//!
//! - [`ClockTick`]: A single tick of the signal clock with timing information
//! - [`ClockSource`]: Trait for objects that can provide clock ticks
//! - [`SystemClock`]: Software-based clock using system time
//! - `Clock`: Legacy clock trait (deprecated, use `ClockSource`)
//!
//! ## Example
//!
//! ```rust
//! use rill_core::time::{SystemClock, ClockSource};
//!
//! let mut clock = SystemClock::with_sample_rate(44100.0);
//! let tick = clock.next_tick(64);
//!
//! println!("Sample position: {}", tick.sample_pos);
//! println!("Time since last tick: {} seconds", tick.delta_seconds());
//! ```

mod clock;
mod error;
mod render;
mod source;
mod tick;

pub use clock::SystemClock;
pub use error::TimeError;
pub use render::{RenderContext, TransportState};
pub use source::ClockSource;
pub use tick::ClockTick;

/// Result type for time operations
pub type TimeResult<T> = Result<T, TimeError>;

/// Prelude for convenient imports
pub mod prelude {
    pub use super::{
        ClockSource, ClockTick, RenderContext, SystemClock, TimeError, TimeResult, TransportState,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        let _clock = SystemClock::with_sample_rate(44100.0);
        let _tick = ClockTick::new(0, 64, 44100.0);
    }
}
