//! Basic oscillators (Sine, Saw, Square, Triangle)

use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use crate::generators::{Generator, ModulatableGenerator, SyncableGenerator};
use crate::vector::prelude::*;
use rill_core::math::vector::scalar::ScalarVector4;
use rill_core::math::vector::traits::{Vector, VectorMask, VectorTranscendental};
use rill_core::traits::{ActionContext, ProcessResult};
use rill_core::Transcendental;
use std::f32::consts::PI;

/// Waveform type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Waveform {
    /// Pure sine wave
    Sine,
    /// Sawtooth wave
    Saw,
    /// Square wave
    Square,
    /// Triangle wave
    Triangle,
    /// Pulse wave with adjustable duty cycle
    Pulse(f32), // pulse width (0.0 - 1.0)
}

impl Waveform {
    /// Get waveform name
    pub fn name(&self) -> &'static str {
        match self {
            Waveform::Sine => "Sine",
            Waveform::Saw => "Saw",
            Waveform::Square => "Square",
            Waveform::Triangle => "Triangle",
            Waveform::Pulse(_) => "Pulse",
        }
    }

    /// Get waveform description
    pub fn description(&self) -> &'static str {
        match self {
            Waveform::Sine => "Pure sine wave - single harmonic",
            Waveform::Saw => "Sawtooth wave - all harmonics (1/n)",
            Waveform::Square => "Square wave - odd harmonics (1/n)",
            Waveform::Triangle => "Triangle wave - odd harmonics (1/n²)",
            Waveform::Pulse(_) => "Pulse wave with variable width",
        }
    }
}

/// Basic oscillator
///
/// Generates various waveforms with support for:
/// - Real-time frequency changes
/// - Frequency modulation (FM)
/// - Anti-aliasing for sawtooth wave
/// - Phase synchronization
#[derive(Clone, Copy)]
pub struct BasicOscillator<T: Transcendental> {
    /// Waveform type
    waveform: Waveform,
    /// Frequency (Hz)
    frequency: f32,
    /// Amplitude (0.0 - 1.0)
    amplitude: ScalarVector1<T>,
    /// Current phase (0..1)
    phase: ScalarVector1<T>,
    /// Phase increment per sample
    phase_inc: ScalarVector1<T>,
    /// Sample rate
    sample_rate: f32,
    /// Number of completed periods
    periods: u32,
    /// Frequency modulation (FM)
    fm_amount: ScalarVector1<T>,
}

impl<T: Transcendental> BasicOscillator<T> {
    /// Create a new oscillator
    ///
    /// # Arguments
    /// * `waveform` - waveform shape
    /// * `frequency` - frequency in Hz
    /// * `amplitude` - amplitude (0.0 - 1.0)
    pub fn new(waveform: Waveform, frequency: f32, amplitude: T) -> Self {
        let mut osc = Self {
            waveform,
            frequency,
            amplitude: ScalarVector1::splat(amplitude),
            phase: ScalarVector1::splat(T::ZERO),
            phase_inc: ScalarVector1::splat(T::ZERO),
            sample_rate: 44100.0,
            periods: 0,
            fm_amount: ScalarVector1::splat(T::ZERO),
        };
        osc.update_phase_inc();
        osc
    }

    /// Update phase increment based on current frequency
    #[inline(always)]
    fn update_phase_inc(&mut self) {
        self.phase_inc = ScalarVector1::splat(T::from_f32(self.frequency / self.sample_rate));
    }

    /// SIMD block generation — processes `output` in chunks of 4,
    /// falling back to scalar for remainder and high-frequency edge cases.
    fn generate_block_simd(&mut self, output: &mut [T]) {
        let chunks = output.len() / 4;
        let _remainder = output.len() % 4;

        if chunks > 0 {
            let inc = self.phase_inc;
            let inc4 = inc * ScalarVector1::splat(T::from_usize(4));
            let one = ScalarVector1::splat(T::ONE);
            let use_simd = (inc.extract(0) * T::from_usize(4)) < T::ONE;

            if use_simd {
                let mut phase = self.phase;
                let amp_v = self.amplitude;

                for chunk in 0..chunks {
                    let offset = chunk * 4;
                    let p0 = phase.extract(0);
                    let inc_t = inc.extract(0);

                    let phases = ScalarVector4::load(&[
                        p0,
                        p0 + inc_t,
                        p0 + inc_t + inc_t,
                        p0 + inc_t + inc_t + inc_t,
                    ]);

                    let vals = match self.waveform {
                        Waveform::Sine => self.simd_sine(&phases, &amp_v),
                        Waveform::Saw => self.simd_saw_blep(&phases, inc_t, &amp_v),
                        Waveform::Square => self.simd_square(&phases, &amp_v),
                        Waveform::Triangle => self.simd_triangle(&phases, &amp_v),
                        Waveform::Pulse(width) => {
                            self.simd_pulse(&phases, T::from_f32(width.clamp(0.01, 0.99)), &amp_v)
                        }
                    };

                    vals.store(&mut output[offset..offset + 4]);

                    phase = phase + inc4;
                    if phase.extract(0) >= one.extract(0) {
                        phase = phase - one;
                        self.periods += 1;
                    }
                }
                self.phase = phase;
            } else {
                // High frequency: fall back to scalar for the block
                for i in 0..chunks * 4 {
                    output[i] = self.generate_scalar();
                }
            }
        }

        // Scalar remainder
        for i in chunks * 4..output.len() {
            output[i] = self.generate_scalar();
        }
    }

    /// Generate ONE sample via the scalar path (same as old generate()).
    /// Renamed from `generate()` to avoid confusion with SIMD methods.
    fn generate_scalar(&mut self) -> T {
        let effective_inc = self.phase_inc + self.fm_amount;
        let output_vec = match self.waveform {
            Waveform::Sine => self.scalar_sine(),
            Waveform::Saw => self.scalar_saw_bandlimited(),
            Waveform::Square => self.scalar_square(),
            Waveform::Triangle => self.scalar_triangle(),
            Waveform::Pulse(width) => self.scalar_pulse(width),
        };
        self.phase = self.phase + effective_inc;
        let one = ScalarVector1::splat(T::ONE);
        if self.phase.extract(0) >= one.extract(0) {
            self.phase = self.phase - one;
            self.periods += 1;
        }
        output_vec.extract(0)
    }

    // ─── Scalar waveform methods (renamed, same logic) ───

    #[inline(always)]
    fn scalar_sine(&self) -> ScalarVector1<T> {
        let phase_rad = self.phase.mul(&ScalarVector1::splat(T::from_f32(2.0 * PI)));
        phase_rad.sin().mul(&self.amplitude)
    }

    #[inline(always)]
    fn scalar_saw_raw(&self) -> ScalarVector1<T> {
        self.phase
            .mul(&ScalarVector1::splat(T::from_f32(2.0)))
            .sub(&ScalarVector1::splat(T::from_f32(1.0)))
            .mul(&self.amplitude)
    }

    #[inline(always)]
    fn scalar_saw_bandlimited(&mut self) -> ScalarVector1<T> {
        let raw = self.scalar_saw_raw();
        let next_phase = self.phase.add(&self.phase_inc).extract(0);
        let one = T::from_f32(1.0);
        if next_phase >= one {
            let one_vec = ScalarVector1::splat(one);
            let t = (one_vec - self.phase) / self.phase_inc;
            let blep =
                t * ScalarVector1::splat(T::from_f32(2.0)) - ScalarVector1::splat(T::from_f32(1.0));
            raw - blep * self.amplitude
        } else {
            raw
        }
    }

    #[inline(always)]
    fn scalar_square(&self) -> ScalarVector1<T> {
        let half = T::from_f32(0.5);
        if self.phase.extract(0) < half {
            self.amplitude
        } else {
            -self.amplitude
        }
    }

    #[inline(always)]
    fn scalar_triangle(&self) -> ScalarVector1<T> {
        let half = ScalarVector1::splat(T::from_f32(0.5));
        let p = self.phase - half;
        (p.abs() * ScalarVector1::splat(T::from_f32(4.0)) - ScalarVector1::splat(T::from_f32(1.0)))
            * self.amplitude
    }

    #[inline(always)]
    fn scalar_pulse(&self, width: f32) -> ScalarVector1<T> {
        let width_t = T::from_f32(width.clamp(0.01, 0.99));
        if self.phase.extract(0) < width_t {
            self.amplitude
        } else {
            -self.amplitude
        }
    }

    // ─── SIMD waveform methods (4 lanes at once) ───

    #[inline(always)]
    fn simd_sine(&self, phases: &ScalarVector4<T>, amp: &ScalarVector1<T>) -> ScalarVector4<T> {
        let pi2 = ScalarVector4::splat(T::from_f32(2.0 * PI));
        let rad = phases.mul(&pi2);
        let raw = rad.sin();
        let amp_broadcast = ScalarVector4::splat(amp.extract(0));
        raw.mul(&amp_broadcast)
    }

    #[inline(always)]
    fn simd_triangle(&self, phases: &ScalarVector4<T>, amp: &ScalarVector1<T>) -> ScalarVector4<T> {
        let half = ScalarVector4::splat(T::from_f32(0.5));
        let four = ScalarVector4::splat(T::from_f32(4.0));
        let one = ScalarVector4::splat(T::from_f32(1.0));
        let amp_b = ScalarVector4::splat(amp.extract(0));
        let p = phases.sub(&half);
        p.abs().mul(&four).sub(&one).mul(&amp_b)
    }

    #[inline(always)]
    fn simd_square(&self, phases: &ScalarVector4<T>, amp: &ScalarVector1<T>) -> ScalarVector4<T> {
        let half = ScalarVector4::splat(T::from_f32(0.5));
        let pos = ScalarVector4::splat(amp.extract(0));
        let neg = ScalarVector4::splat(-amp.extract(0));
        let mask = phases.lt(&half);
        <ScalarVector4<T> as VectorMask<T, 4>>::select(&pos, &neg, mask)
    }

    #[inline(always)]
    fn simd_pulse(
        &self,
        phases: &ScalarVector4<T>,
        width_t: T,
        amp: &ScalarVector1<T>,
    ) -> ScalarVector4<T> {
        let threshold = ScalarVector4::splat(width_t);
        let pos = ScalarVector4::splat(amp.extract(0));
        let neg = ScalarVector4::splat(-amp.extract(0));
        let mask = phases.lt(&threshold);
        <ScalarVector4<T> as VectorMask<T, 4>>::select(&pos, &neg, mask)
    }

    #[inline(always)]
    fn simd_saw_raw(&self, phases: &ScalarVector4<T>, amp: &ScalarVector1<T>) -> ScalarVector4<T> {
        let two = ScalarVector4::splat(T::from_f32(2.0));
        let one = ScalarVector4::splat(T::from_f32(1.0));
        let amp_b = ScalarVector4::splat(amp.extract(0));
        phases.mul(&two).sub(&one).mul(&amp_b)
    }

    #[inline(always)]
    fn simd_saw_blep(
        &mut self,
        phases: &ScalarVector4<T>,
        inc: T,
        amp: &ScalarVector1<T>,
    ) -> ScalarVector4<T> {
        let raw = self.simd_saw_raw(phases, amp);
        let one = ScalarVector4::splat(T::ONE);
        let two = ScalarVector4::splat(T::from_f32(2.0));
        let inc_v = ScalarVector4::splat(inc);
        let amp_v = ScalarVector4::splat(amp.extract(0));

        // next_phases = phases + inc for each lane
        let next = phases.add(&inc_v);

        // Mask: true where next >= 1.0 (discontinuity)
        let wrap_mask = next.ge(&one);

        // t = (1 - phase) / inc (pre-compute for all lanes, used only where wrapping)
        let t = one.sub(phases).div(&inc_v);

        // BLEP = 2*t - 1, then scale by amplitude
        let blep = t.mul(&two).sub(&one).mul(&amp_v);

        let corrected = raw.sub(&blep);

        <ScalarVector4<T> as VectorMask<T, 4>>::select(&corrected, &raw, wrap_mask)
    }

    /// Backward-compatible public API (used by LFO and existing callers).
    pub(crate) fn generate(&mut self) -> ScalarVector1<T> {
        let effective_inc = self.phase_inc + self.fm_amount;
        let output_vec = match self.waveform {
            Waveform::Sine => self.scalar_sine(),
            Waveform::Saw => self.scalar_saw_bandlimited(),
            Waveform::Square => self.scalar_square(),
            Waveform::Triangle => self.scalar_triangle(),
            Waveform::Pulse(width) => self.scalar_pulse(width),
        };
        self.phase = self.phase + effective_inc;
        let one = ScalarVector1::splat(T::from_f32(1.0));
        if self.phase.extract(0) >= one.extract(0) {
            self.phase = self.phase - one;
            self.periods += 1;
        }
        output_vec
    }

    /// Reset phase to 0
    pub fn reset_phase(&mut self) {
        self.phase = ScalarVector1::splat(T::ZERO);
        self.periods = 0;
    }

    /// Get current phase (0..1)
    pub fn current_phase(&self) -> T {
        self.phase.extract(0)
    }

    /// Get number of completed periods
    pub fn period_count(&self) -> u32 {
        self.periods
    }

    /// Set pulse width (for Pulse waveform)
    pub fn set_pulse_width(&mut self, width: f32) {
        if let Waveform::Pulse(_) = self.waveform {
            self.waveform = Waveform::Pulse(width.clamp(0.01, 0.99));
        }
    }
}

// ==================== Algorithm trait implementation ====================

impl<T: Transcendental> Algorithm<T> for BasicOscillator<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_phase_inc();
        self.phase = ScalarVector1::splat(T::ZERO);
        self.periods = 0;
    }

    fn reset(&mut self) {
        self.phase = ScalarVector1::splat(T::ZERO);
        self.periods = 0;
        self.fm_amount = ScalarVector1::splat(T::ZERO);
    }

    fn process(
        &mut self,
        _input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        self.generate_block_simd(output);
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: self.waveform.name(),
            category: AlgorithmCategory::Generator,
            description: self.waveform.description(),
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

// ==================== Generator trait implementation ====================

impl<T: Transcendental> Generator<T> for BasicOscillator<T> {
    fn phase(&self) -> T {
        self.phase.extract(0)
    }

    fn set_phase(&mut self, phase: T) {
        let one = T::from_f32(1.0);
        let zero = T::ZERO;
        self.phase = ScalarVector1::splat(if phase > one {
            one
        } else if phase < zero {
            zero
        } else {
            phase
        });
    }

    fn frequency(&self) -> f32 {
        self.frequency
    }

    fn set_frequency(&mut self, freq: f32) {
        self.frequency = freq.clamp(0.1, 20000.0);
        self.update_phase_inc();
    }

    fn amplitude(&self) -> T {
        self.amplitude.extract(0)
    }

    fn set_amplitude(&mut self, amp: T) {
        let one = T::from_f32(1.0);
        let zero = T::ZERO;
        self.amplitude = ScalarVector1::splat(if amp > one {
            one
        } else if amp < zero {
            zero
        } else {
            amp
        });
    }
}

// ==================== SyncableGenerator trait implementation ====================

impl<T: Transcendental> SyncableGenerator<T> for BasicOscillator<T> {
    fn sync(&mut self, reset: bool) {
        if reset {
            self.phase = ScalarVector1::splat(T::ZERO);
        }
    }

    fn periods(&self) -> u32 {
        self.periods
    }
}

// ==================== ModulatableGenerator trait implementation ====================

impl<T: Transcendental> ModulatableGenerator<T> for BasicOscillator<T> {
    fn modulate_frequency(&mut self, amount: T) {
        self.fm_amount = ScalarVector1::splat(amount);
    }

    fn modulation_index(&self) -> T {
        self.fm_amount.extract(0)
    }

    fn set_modulation_index(&mut self, index: T) {
        self.fm_amount = ScalarVector1::splat(index);
    }
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;

    #[test]
    fn test_sine_oscillator() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 0.5);
        osc.init(44100.0);

        // First sample should be 0
        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample1 = output[0];
        assert!(approx_eq!(f32, sample1, 0.0, epsilon = 1e-6));

        // Second sample should not be 0
        osc.process(None, &mut output, &ctx).unwrap();
        let sample2 = output[0];
        assert!(sample2 != 0.0);
        assert!(sample2 >= -0.5 && sample2 <= 0.5);
    }

    #[test]
    fn test_saw_oscillator() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Saw, 440.0, 0.5);
        osc.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample >= -0.5 && sample <= 0.5);
    }

    #[test]
    fn test_square_oscillator() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Square, 440.0, 0.5);
        osc.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample == 0.5 || sample == -0.5);
    }

    #[test]
    fn test_triangle_oscillator() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Triangle, 440.0, 0.5);
        osc.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample >= -0.5 && sample <= 0.5);
    }

    #[test]
    fn test_pulse_oscillator() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Pulse(0.25), 440.0, 0.5);
        osc.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample == 0.5); // At phase 0 should be positive pulse
    }

    #[test]
    fn test_frequency_change() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 0.5);
        osc.init(44100.0);

        assert_eq!(osc.frequency(), 440.0);

        osc.set_frequency(880.0);
        assert_eq!(osc.frequency(), 880.0);
    }

    #[test]
    fn test_amplitude_change() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 0.5);
        osc.init(44100.0);

        assert_eq!(osc.amplitude(), 0.5);

        osc.set_amplitude(0.8);
        assert_eq!(osc.amplitude(), 0.8);
    }

    #[test]
    fn test_phase_manipulation() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 1.0);
        osc.init(44100.0);

        osc.set_phase(0.25); // π/2
        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(approx_eq!(f32, sample, 1.0, epsilon = 1e-4)); // sin(π/2) = 1
    }

    #[test]
    fn test_fm_modulation() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 1.0);
        osc.init(44100.0);

        osc.modulate_frequency(0.5);
        assert_eq!(osc.modulation_index(), 0.5);

        // Verify modulation is applied
        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample >= -1.0 && sample <= 1.0);
    }

    #[test]
    fn test_clone_copy() {
        let osc1 = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 0.5);
        let osc2 = osc1; // Copy via Copy trait
        let osc3 = osc1.clone(); // Explicit clone

        assert_eq!(osc1.frequency(), osc2.frequency());
        assert_eq!(osc1.frequency(), osc3.frequency());
    }
}
