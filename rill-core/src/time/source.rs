//! # Clock sources for signal timing
//!
//! This module defines the `ClockSource` trait and related types for
//! providing timing information to the signal graph.

use super::error::TimeError;
use super::tick::ClockTick;
use crate::time::SystemClock;
use std::fmt;

/// A source of clock ticks for signal processing
///
/// Implementations can be hardware-based (ALSA, JACK) or software-based
/// (`SystemClock`). The clock source is responsible for providing
/// accurate timing information to the signal graph.
///
/// # Example
///
/// ```
/// use rill_core::time::{ClockSource, SystemClock};
///
/// let mut clock = SystemClock::with_sample_rate(44100.0);
/// let tick = clock.next_tick(64);
/// ```
pub trait ClockSource: Send + Sync + fmt::Debug {
    /// Get the next clock tick
    ///
    /// # Arguments
    /// * `block_size` - Number of samples in the next block
    ///
    /// # Returns
    /// A `ClockTick` containing timing information for the next block
    fn next_tick(&mut self, block_size: usize) -> ClockTick;

    /// Get the sample rate of this clock source
    fn sample_rate(&self) -> f32;

    /// Start the clock
    ///
    /// This is called when the signal graph starts processing.
    /// Default implementation does nothing.
    fn start(&mut self) -> Result<(), TimeError> {
        Ok(())
    }

    /// Stop the clock
    ///
    /// This is called when the signal graph stops processing.
    /// Default implementation does nothing.
    fn stop(&mut self) -> Result<(), TimeError> {
        Ok(())
    }

    /// Check if the clock is running
    fn is_running(&self) -> bool {
        true
    }

    /// Get the current sample position
    ///
    /// Default implementation returns the position from the last tick,
    /// but hardware clocks may provide more accurate information.
    fn current_position(&self) -> u64 {
        0
    }
}

impl ClockSource for SystemClock {
    fn next_tick(&mut self, block_size: usize) -> ClockTick {
        self.next_tick(block_size)
    }

    fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use crate::time::SystemClock;

    #[test]
    fn test_clock_source_trait() {
        let mut clock = SystemClock::with_sample_rate(44100.0);

        assert_eq!(clock.sample_rate, 44100.0);
        //assert!(clock.is_running());

        let tick = clock.next_tick(64);
        assert_eq!(tick.sample_pos, 0);
        assert_eq!(tick.samples_since_last, 64);
    }
}
