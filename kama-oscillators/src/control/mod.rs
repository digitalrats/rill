//! Control frequency oscillators (0.01Hz - 100Hz)
//!
//! These are used for modulation and automation, not for direct audio generation.

mod envelope;
mod lfo;
mod random;
mod sample_hold;

pub use envelope::{Envelope, EnvelopeStage};
pub use lfo::{Lfo, LfoWaveform};
pub use random::RandomWalk;
pub use sample_hold::SampleAndHold;

/// Common trait for all control oscillators
pub trait ControlOscillator: kama_core_traits::AudioNode {
    /// Generate next value
    fn generate(&mut self) -> f64;

    /// Peek current value without advancing
    fn peek(&self) -> f64;

    /// Reset internal state
    fn reset(&mut self);
}
