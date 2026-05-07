//! Low-frequency oscillators for modulation
//!
//! LFOs are used for modulating sound parameters:
//! vibrato (frequency), tremolo (amplitude), filter sweep (cutoff),
//! and other effects.

use super::basic::{BasicOscillator, Waveform};
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use crate::generators::{Generator, SyncableGenerator};
use crate::vector::prelude::*;
use rill_core::traits::{ActionContext, ProcessResult};
use rill_core::Transcendental;

/// LFO (Low Frequency Oscillator)
///
/// Generates low-frequency signals for parameter modulation.
/// Frequency range: 0.01 Hz - 100 Hz.
///
/// # Operating modes
///
/// - **Bipolar**: output in range [-1, 1]
/// - **Unipolar**: output in range [0, 1]
///
/// # Example
/// ```
/// use rill_core::time::ClockTick;
/// use rill_core::traits::ActionContext;
/// use rill_core_dsp::generators::*;
/// use rill_core_dsp::Algorithm;
///
/// let tick = ClockTick::default();
/// let ctx = ActionContext::new(&tick);
///
/// // Create LFO for filter frequency modulation
/// let mut lfo = LFO::<f32>::new(
///     5.0,              // 5 Hz
///     Waveform::Sine,
///     true              // bipolar mode (-1..1)
/// );
/// lfo.init(44100.0);
///
/// // Generate modulation signal
/// let mut output = [0.0_f32];
/// lfo.process(None, &mut output, &ctx).unwrap();
/// let modulation = output[0];
/// ```
#[derive(Clone, Copy)]
pub struct LFO<T: Transcendental> {
    /// Internal oscillator
    osc: BasicOscillator<T>,
    /// Bipolar mode (-1..1) or unipolar (0..1)
    bipolar: bool,
    /// Phase offset (for synchronization)
    phase_offset: ScalarVector1<T>,
}

impl<T: Transcendental> LFO<T> {
    /// Create a new LFO
    ///
    /// # Arguments
    /// * `frequency` - frequency in Hz (0.01 - 100)
    /// * `waveform` - waveform shape
    /// * `bipolar` - true for bipolar (-1..1), false for unipolar (0..1)
    pub fn new(frequency: f32, waveform: Waveform, bipolar: bool) -> Self {
        let one = T::from_f32(1.0);
        Self {
            osc: BasicOscillator::new(waveform, frequency, one),
            bipolar,
            phase_offset: ScalarVector1::splat(T::ZERO),
        }
    }

    /// Create an LFO with phase offset
    pub fn with_phase_offset(mut self, offset: T) -> Self {
        self.set_phase_offset(offset);
        self
    }

    /// Set bipolar mode
    ///
    /// # Arguments
    /// * `bipolar` - true: output in [-1, 1], false: output in [0, 1]
    pub fn set_bipolar(&mut self, bipolar: bool) {
        self.bipolar = bipolar;
    }

    /// Set phase offset (0..1)
    ///
    /// Shifts the LFO phase relative to the reference point.
    /// Useful for stereo effects or synchronizing multiple LFOs.
    pub fn set_phase_offset(&mut self, offset: T) {
        let one = T::from_f32(1.0);
        let zero = T::ZERO;
        let clamped = if offset > one {
            one
        } else if offset < zero {
            zero
        } else {
            offset
        };
        self.phase_offset = ScalarVector1::splat(clamped);
    }

    /// Get current phase offset
    pub fn phase_offset(&self) -> T {
        self.phase_offset.extract(0)
    }

    /// Check if LFO is in bipolar mode
    pub fn is_bipolar(&self) -> bool {
        self.bipolar
    }

    /// Sync with external clock
    ///
    /// # Arguments
    /// * `reset` - if true, reset phase to phase_offset value
    pub fn sync(&mut self, reset: bool) {
        if reset {
            self.osc.set_phase(self.phase_offset.extract(0));
        }
    }

    /// Get modulation value (current sample)
    pub fn modulate(&mut self) -> T {
        let raw = self.osc.generate().extract(0);

        if self.bipolar {
            raw // already -1..1
        } else {
            // Convert from -1..1 to 0..1
            raw.mul(T::from_f32(0.5)).add(T::from_f32(0.5))
        }
    }

    /// Reset LFO to initial state
    pub fn reset(&mut self) {
        self.osc.reset();
        self.osc.set_phase(self.phase_offset.extract(0));
    }
}

// ==================== Algorithm trait implementation ====================

impl<T: Transcendental> Algorithm<T> for LFO<T> {
    fn init(&mut self, sample_rate: f32) {
        self.osc.init(sample_rate);
        self.osc.set_phase(self.phase_offset.extract(0));
    }

    fn reset(&mut self) {
        self.osc.reset();
        self.osc.set_phase(self.phase_offset.extract(0));
    }

    fn process(
        &mut self,
        _input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        for out in output.iter_mut() {
            *out = self.modulate();
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        // Get waveform name from LFO's internal waveform
        // No direct access to waveform, so use description from BasicOscillator
        AlgorithmMetadata {
            name: "LFO",
            category: AlgorithmCategory::Generator,
            description: format!(
                "{} wave LFO ({}polar)",
                match self.osc.frequency() {
                    _ if self.osc.frequency() < 1.0 => "Very low frequency",
                    _ if self.osc.frequency() < 10.0 => "Low frequency",
                    _ => "Signal rate",
                },
                if self.bipolar { "bi" } else { "uni" }
            )
            .leak(),
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

// ==================== Generator trait implementation ====================

impl<T: Transcendental> Generator<T> for LFO<T> {
    fn phase(&self) -> T {
        self.osc.phase()
    }

    fn set_phase(&mut self, phase: T) {
        self.osc.set_phase(phase);
    }

    fn frequency(&self) -> f32 {
        self.osc.frequency()
    }

    fn set_frequency(&mut self, freq: f32) {
        self.osc.set_frequency(freq);
    }

    fn amplitude(&self) -> T {
        self.osc.amplitude()
    }

    fn set_amplitude(&mut self, amp: T) {
        self.osc.set_amplitude(amp);
    }
}

// ==================== SyncableGenerator trait implementation ====================

impl<T: Transcendental> SyncableGenerator<T> for LFO<T> {
    fn sync(&mut self, reset: bool) {
        if reset {
            self.osc.set_phase(self.phase_offset.extract(0));
        }
    }

    fn periods(&self) -> u32 {
        self.osc.periods()
    }
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;
    use rill_core::time::ClockTick;
    use rill_core::traits::ActionContext;

    #[test]
    fn test_lfo_creation() {
        let lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        assert_eq!(lfo.frequency(), 5.0);
        assert!(lfo.is_bipolar());
        assert_eq!(lfo.phase_offset(), 0.0);
    }

    #[test]
    fn test_lfo_bipolar_mode() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        lfo.init(44100.0);

        // In bipolar mode, values should be in [-1, 1]
        let mut output = [0.0f32; 1];
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);
        for _ in 0..100 {
            lfo.process(None, &mut output, &ctx).unwrap();
            let val = output[0];
            assert!(
                val >= -1.0 && val <= 1.0,
                "Value {} out of range [-1,1]",
                val
            );
        }
    }

    #[test]
    fn test_lfo_unipolar_mode() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, false);
        lfo.init(44100.0);

        // In unipolar mode, values should be in [0, 1]
        let mut output = [0.0f32; 1];
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);
        for _ in 0..100 {
            lfo.process(None, &mut output, &ctx).unwrap();
            let val = output[0];
            assert!(val >= 0.0 && val <= 1.0, "Value {} out of range [0,1]", val);
        }
    }

    #[test]
    fn test_lfo_phase_offset() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        lfo.set_phase_offset(0.25);
        lfo.init(44100.0);

        // Verify phase is set correctly
        assert!(approx_eq!(f32, lfo.phase(), 0.25, epsilon = 0.01));
    }

    #[test]
    fn test_lfo_sync() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        lfo.set_phase_offset(0.5);
        lfo.init(44100.0);

        // Advance phase
        let mut output = [0.0f32; 1];
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);
        for _ in 0..10 {
            lfo.process(None, &mut output, &ctx).unwrap();
        }

        // Sync with reset
        lfo.sync(true);
        assert!(approx_eq!(f32, lfo.phase(), 0.5, epsilon = 0.01));
    }

    #[test]
    fn test_lfo_waveforms() {
        let waveforms = [
            Waveform::Sine,
            Waveform::Saw,
            Waveform::Square,
            Waveform::Triangle,
        ];

        for &wav in &waveforms {
            let mut lfo = LFO::<f32>::new(5.0, wav, true);
            lfo.init(44100.0);

            let mut output = [0.0f32; 1];
            let tick = ClockTick::default();
            let ctx = ActionContext::new(&tick);
            lfo.process(None, &mut output, &ctx).unwrap();
            let val = output[0];
            assert!(
                val >= -1.0 && val <= 1.0,
                "Waveform {:?} produced {}",
                wav,
                val
            );
        }
    }

    #[test]
    fn test_lfo_generator_trait() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        lfo.init(44100.0);

        // Test methods from Generator trait
        assert_eq!(lfo.frequency(), 5.0);

        lfo.set_frequency(10.0);
        assert_eq!(lfo.frequency(), 10.0);

        lfo.set_amplitude(0.5);
        assert_eq!(lfo.amplitude(), 0.5);

        let phase = lfo.phase();
        assert!(phase >= 0.0 && phase <= 1.0);
    }

    #[test]
    fn test_lfo_syncable_trait() {
        let mut lfo = LFO::<f32>::new(5.0, Waveform::Sine, true);
        lfo.init(44100.0);

        let initial_periods = lfo.periods();
        println!("Initial periods: {}", initial_periods);

        // Compute samples per period
        let samples_per_period = (44100.0 / 5.0) as usize; // 8820 samples
        println!("Samples per period: {}", samples_per_period);

        // Record initial phase
        let initial_phase = lfo.phase();
        println!("Initial phase: {}", initial_phase.to_f32());
        let mut output = [0.0f32; 1];
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);

        // Advance phase through several periods
        for i in 0..samples_per_period * 3 {
            // 3 full periods
            let before_phase = lfo.phase();
            lfo.process(None, &mut output, &ctx).unwrap();
            let after_phase = lfo.phase();

            // Check if phase reset occurred
            if after_phase < before_phase {
                println!(
                    "Phase reset at sample {}: {} -> {}",
                    i,
                    before_phase.to_f32(),
                    after_phase.to_f32()
                );
                println!("Periods count: {}", lfo.periods());
            }

            // Debug output at key points
            if i == samples_per_period - 1 {
                println!(
                    "After 1 period (sample {}): phase={}, periods={}",
                    i,
                    lfo.phase().to_f32(),
                    lfo.periods()
                );
            } else if i == samples_per_period * 2 - 1 {
                println!(
                    "After 2 periods (sample {}): phase={}, periods={}",
                    i,
                    lfo.phase().to_f32(),
                    lfo.periods()
                );
            }
        }

        println!("Final phase: {}", lfo.phase().to_f32());
        println!("Final periods: {}", lfo.periods());

        assert!(
            lfo.periods() > initial_periods,
            "Periods should increase: before={}, after={}",
            initial_periods,
            lfo.periods()
        );

        // Verify phase continues to change
        let mid_phase = lfo.phase();
        assert!(mid_phase != initial_phase, "Phase should change");

        // Sync with reset
        lfo.sync(true);
        assert!(approx_eq!(f32, lfo.phase(), 0.0, epsilon = 0.01));
    }

    #[test]
    fn test_lfo_clone_copy() {
        let lfo1 = LFO::<f32>::new(5.0, Waveform::Sine, true);
        let lfo2 = lfo1; // Copy
        let lfo3 = lfo1.clone(); // Explicit clone

        assert_eq!(lfo1.frequency(), lfo2.frequency());
        assert_eq!(lfo1.frequency(), lfo3.frequency());
        assert_eq!(lfo1.is_bipolar(), lfo2.is_bipolar());
    }
}
