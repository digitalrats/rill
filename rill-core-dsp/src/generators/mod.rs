//! # Signal generators
//!
//! This module provides various generators for sound synthesis:
//! - Basic oscillators (Sine, Saw, Square, Triangle, Pulse)
//! - Noise generators (White, Pink, Brown, Blue, Violet)
//! - Envelopes (ADSR, AR, ASR)
//! - LFO for modulation
//! - FM synthesis
//!
//! All generators implement the common [`Generator`] trait and are parameterized
//! by the `T: Transcendental` type (f32 or f64).

// Import necessary types and traits
use crate::algorithm::Algorithm;
use rill_core::Transcendental;

// Declare submodules
mod basic;
mod envelope;
mod fm;
mod lfo;
mod noise;
mod reader;
mod resampler;
mod sample_player;
mod wavetable;

// Re-export everything from submodules
pub use basic::*;
pub use envelope::*;
pub use fm::*;
pub use lfo::*;
pub use noise::*;
pub use reader::*;
pub use resampler::*;
pub use sample_player::*;
pub use wavetable::*;

/// Base trait for all generators
///
/// Provides basic generator control methods:
/// - phase control
/// - frequency changes
/// - amplitude changes
pub trait Generator<T: Transcendental>: Algorithm<T> {
    /// Get current phase (0.0 - 1.0)
    fn phase(&self) -> T;

    /// Set phase
    fn set_phase(&mut self, phase: T);

    /// Reset phase to 0
    fn reset_phase(&mut self) {
        self.set_phase(T::ZERO);
    }

    /// Get frequency in Hz
    fn frequency(&self) -> f32;

    /// Set frequency
    fn set_frequency(&mut self, freq: f32);

    /// Get amplitude
    fn amplitude(&self) -> T;

    /// Set amplitude
    fn set_amplitude(&mut self, amp: T);
}

/// Generator with synchronization
///
/// Allows synchronizing multiple generators
/// by phase or clock signal.
pub trait SyncableGenerator<T: Transcendental>: Generator<T> {
    /// Sync with external clock signal
    ///
    /// # Arguments
    /// * `reset` - if true, reset phase to 0
    fn sync(&mut self, reset: bool);

    /// Get number of periods since last reset
    fn periods(&self) -> u32;
}

/// Generator with frequency modulation
///
/// Supports frequency modulation (FM) for creating
/// complex timbres.
pub trait ModulatableGenerator<T: Transcendental>: Generator<T> {
    /// Apply frequency modulation
    ///
    /// # Arguments
    /// * `amount` - modulation amount
    fn modulate_frequency(&mut self, amount: T);

    /// Modulation index (current FM amount)
    fn modulation_index(&self) -> T;

    /// Set modulation index
    fn set_modulation_index(&mut self, index: T);
}

// =============================================================================
// Generator comparison
// =============================================================================

/// Generator characteristics summary
#[derive(Debug)]
pub struct GeneratorComparison;

impl GeneratorComparison {
    /// Harmonic content comparison
    pub fn harmonic_content() -> &'static str {
        "Harmonic content:\n\
         ┌────────────┬─────────────────────────────────┐\n\
         │ Generator  │ Spectrum                          │\n\
         ├────────────┼─────────────────────────────────┤\n\
         │ Sine       │ Single harmonic (pure tone)     │\n\
         │ Triangle   │ Odd harmonics, fast roll-off     │\n\
         │ Saw        │ All harmonics (1/n)             │\n\
         │ Square     │ Odd harmonics (1/n)        │\n\
         │ Pulse      │ Depends on pulse width      │\n\
         │ White      │ Uniform across all frequencies    │\n\
         │ Pink       │ 3dB/octave roll-off (1/f)           │\n\
         │ Brown      │ 6dB/octave roll-off (1/f²)          │\n\
         └────────────┴─────────────────────────────────┘"
    }

    /// Usage recommendations
    pub fn usage_guide() -> &'static str {
        "How to choose a generator:\n\n\
         🎵 **Subtractive synthesis**:\n\
         → Saw, Square, Pulse - rich spectrum for filtering\n\n\
         🎵 **FM synthesis**:\n\
         → Sine - pure tone for modulation\n\n\
         🎵 **Additive synthesis**:\n\
         → Sine (multiple) - building complex timbres\n\n\
         🎵 **Noise effects**:\n\
         → White - wind, snare drum\n\
         → Pink - natural phenomena\n\
         → Brown - thunder, rumble\n\n\
         🎵 **Envelopes**:\n\
         → ADSR - amplitude envelopes\n\
         → AR - percussion\n\
         → ASR - organ sounds\n\n\
         🎵 **Modulation**:\n\
         → LFO - vibrato, tremolo, filter sweep"
    }

    /// Performance characteristics
    pub fn performance_guide() -> &'static str {
        "Performance (relative):\n\
         ⚡ **Sine** - 1x (fastest)\n\
         ⚡⚡ **Triangle, Square** - 1.5x\n\
         ⚡⚡⚡ **Saw** - 2x (with anti-aliasing)\n\
         ⚡⚡⚡ **Noise** - 2x\n\
         ⚡⚡⚡⚡ **Envelope** - 3x\n\
         ⚡⚡⚡⚡ **FM Synth** - depends on operator count"
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generator_trait_bounds() {
        // Verify all generators implement required traits
        fn assert_generator<T: Transcendental, G: Generator<T>>() {}
        fn assert_syncable<T: Transcendental, G: SyncableGenerator<T>>() {}
        fn assert_modulatable<T: Transcendental, G: ModulatableGenerator<T>>() {}

        assert_generator::<f32, BasicOscillator<f32>>();
        assert_generator::<f32, NoiseGenerator<f32>>();
        assert_generator::<f32, EnvelopeGenerator<f32>>();
        assert_generator::<f32, LFO<f32>>();
        assert_generator::<f32, SimpleFmSynth<f32>>();

        assert_syncable::<f32, BasicOscillator<f32>>();
        assert_modulatable::<f32, BasicOscillator<f32>>();
    }

    #[test]
    fn test_comparison_guides() {
        assert!(!GeneratorComparison::harmonic_content().is_empty());
        assert!(!GeneratorComparison::usage_guide().is_empty());
        assert!(!GeneratorComparison::performance_guide().is_empty());
    }
}
