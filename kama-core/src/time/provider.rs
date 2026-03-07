//! TimeProvider trait for extended timing with tempo

use super::clock::Clock;
use super::tick::TickInfo;

/// Extended time provider with tempo and musical timing information
pub trait TimeProvider: Clock {
    /// Get the current tempo in BPM
    fn bpm(&self) -> f64;

    /// Set the tempo
    fn set_bpm(&self, bpm: f64);

    /// Get detailed tick information (bar, beat, sixteenth)
    fn tick_info(&self) -> TickInfo;
}