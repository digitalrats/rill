//! Clock trait for basic timekeeping

use std::fmt;

/// Basic clock trait for timekeeping in audio systems
pub trait Clock: Send + Sync + fmt::Debug {
    /// Get the current sample rate
    fn sample_rate(&self) -> f64;

    /// Get the current position in samples
    fn position_samples(&self) -> u64;

    /// Get the current position in seconds
    fn position_seconds(&self) -> f64 {
        self.position_samples() as f64 / self.sample_rate()
    }

    /// Advance the clock by the given number of samples
    fn advance(&self, samples: u64) -> u64;

    /// Reset the clock to zero
    fn reset(&self);
}