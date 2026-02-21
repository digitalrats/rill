//! Audio frequency oscillators (20Hz - 20kHz)

mod sine;
mod saw;
mod square;
mod triangle;
mod noise;
mod fm;

pub use sine::SineOsc;
pub use saw::SawOsc;
pub use square::SquareOsc;
pub use triangle::TriangleOsc;
pub use noise::NoiseOsc;
pub use fm::FmOsc;

/// Common trait for all audio oscillators
pub trait AudioOscillator: kama_core_traits::AudioNode {
    /// Set oscillator frequency in Hz
    fn set_frequency(&mut self, freq: f32);
    
    /// Get current frequency
    fn frequency(&self) -> f32;
    
    /// Set output amplitude (0.0 - 1.0)
    fn set_amplitude(&mut self, amp: f32);
    
    /// Get current amplitude
    fn amplitude(&self) -> f32;
    
    /// Reset phase to zero
    fn reset_phase(&mut self);
}