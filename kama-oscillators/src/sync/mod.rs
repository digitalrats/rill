//! Synchronization and clock generators
//!
//! These generators provide timing and synchronization signals
//! for modular systems and sequencers.

mod clock;
mod pulse;
mod trigger;

pub use clock::{Clock, ClockDivision};
pub use pulse::PulseGenerator;
pub use trigger::{Trigger, TriggerMode};

/// Common trait for sync generators
pub trait SyncGenerator: kama_core_traits::AudioNode {
    /// Get current tempo in BPM
    fn tempo(&self) -> f32;

    /// Set tempo in BPM
    fn set_tempo(&mut self, bpm: f32);

    /// Reset to start of cycle
    fn reset(&mut self);

    /// Check if a new pulse occurred at current sample
    fn is_triggered(&self) -> bool;
}
