//! Traits for signal analyzers

use rill_core::traits::algorithm::Algorithm;
use rill_core::Transcendental;

/// Base trait for analyzers
pub trait Analyzer<T: Transcendental>: Algorithm<T> {
    /// Analysis result type
    type Output;

    /// Get analysis result
    fn result(&self) -> &Self::Output;

    /// Reset accumulated data
    fn reset_analysis(&mut self);
}

/// Peak detector (VU meter)
pub trait PeakMeter<T: Transcendental>: Analyzer<T, Output = T> {
    /// Decay rate (0.0-1.0)
    fn decay(&self) -> f32;

    /// Set decay rate
    fn set_decay(&mut self, decay: f32);

    /// Current peak (for display)
    fn peak(&self) -> T {
        *self.result()
    }
}

/// Envelope follower
pub trait EnvelopeFollower<T: Transcendental>: Analyzer<T, Output = T> {
    /// Attack time in seconds
    fn attack(&self) -> f32;

    /// Release time in seconds
    fn release(&self) -> f32;

    /// Set times
    fn set_attack_release(&mut self, attack: f32, release: f32);

    /// Current envelope
    fn envelope(&self) -> T {
        *self.result()
    }
}

/// Frequency detector (for tuners)
pub trait FrequencyDetector<T: Transcendental>: Analyzer<T, Output = f32> {
    /// Minimum detection frequency
    fn min_freq(&self) -> f32;

    /// Maximum detection frequency
    fn max_freq(&self) -> f32;

    /// Current frequency
    fn frequency(&self) -> f32 {
        *self.result()
    }

    /// Closest MIDI note
    fn closest_midi_note(&self) -> u8 {
        let freq = self.frequency();
        if freq <= 0.0 {
            return 0;
        }
        let note = 69.0 + 12.0 * (freq / 440.0).log2();
        note.round() as u8
    }
}

/// Spectrum analyzer (FFT)
pub trait SpectrumAnalyzer<T: Transcendental>: Analyzer<T, Output = Vec<f32>> {
    /// FFT size
    fn fft_size(&self) -> usize;

    /// Get spectrum
    fn spectrum(&self) -> &[f32] {
        self.result()
    }

    /// Get amplitude at specific frequency
    fn amplitude_at(&self, freq: f32, sample_rate: f32) -> f32 {
        let bin = (freq * self.fft_size() as f32 / sample_rate) as usize;
        self.result().get(bin).copied().unwrap_or(0.0)
    }
}
