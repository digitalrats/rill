//! FM (Frequency Modulation) synthesis
//!
//! This module provides tools for frequency modulation:
//! - Simple 2-operator FM synthesizer
//! - Multi-operator FM synthesizer (like Yamaha DX7)
//! - Support for different waveforms per operator
//! - Flexible modulation routing

use super::basic::{BasicOscillator, Waveform};
use crate::generators::{Generator, ModulatableGenerator};
use crate::vector::prelude::*;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;

// =============================================================================
// Simple 2-operator FM synthesizer
// =============================================================================

/// Simple 2-operator FM synthesizer
///
/// Basic FM architecture: one modulator modulates one carrier.
/// Ideal for:
/// - Creating metallic timbres
/// - Bell-like sounds
/// - Complex harmonic structures
///
/// # Example
/// ```
/// use rill_core::time::ClockTick;
/// use rill_core::traits::ActionContext;
/// use rill_core_dsp::generators::*;
/// use rill_core::traits::algorithm::Algorithm;
///
/// let tick = ClockTick::default();
/// let ctx = ActionContext::new(&tick);
///
/// // Create FM synthesizer with 2:1 frequency ratio
/// let mut fm = SimpleFmSynth::<f32>::new(
///     440.0,  // carrier frequency (A4)
///     2.0,    // modulator one octave higher
///     1.5     // modulation index
/// );
/// fm.init(44100.0);
///
/// // Generate sample
/// let mut output = [0.0_f32];
/// fm.process(None, &mut output).unwrap();
/// let sample = output[0];
/// ```
#[derive(Clone, Copy)]
pub struct SimpleFmSynth<T: Transcendental> {
    /// Carrier oscillator - produces the output signal
    carrier: BasicOscillator<T>,
    /// Modulator oscillator - modulates carrier frequency
    modulator: BasicOscillator<T>,
    /// Modulation index (modulation depth)
    modulation_index: ScalarVector1<T>,
    /// Modulator-to-carrier frequency ratio
    ratio: f32,
}

impl<T: Transcendental> SimpleFmSynth<T> {
    /// Create a new FM synthesizer
    ///
    /// # Arguments
    /// * `carrier_freq` - carrier frequency in Hz
    /// * `modulator_ratio` - frequency ratio (modulator/carrier)
    /// * `modulation_index` - modulation index (0.0 - 10.0)
    pub fn new(carrier_freq: f32, modulator_ratio: f32, modulation_index: T) -> Self {
        let one = T::from_f32(1.0);
        Self {
            carrier: BasicOscillator::new(Waveform::Sine, carrier_freq, one),
            modulator: BasicOscillator::new(Waveform::Sine, carrier_freq * modulator_ratio, one),
            modulation_index: ScalarVector1::splat(modulation_index),
            ratio: modulator_ratio,
        }
    }

    /// Set carrier waveform
    ///
    /// Default is sine wave
    pub fn with_carrier_waveform(mut self, waveform: Waveform) -> Self {
        let freq = self.carrier.frequency();
        self.carrier = BasicOscillator::new(waveform, freq, T::from_f32(1.0));
        self
    }

    /// Set modulator waveform
    ///
    /// Default is sine wave
    pub fn with_modulator_waveform(mut self, waveform: Waveform) -> Self {
        let freq = self.modulator.frequency();
        self.modulator = BasicOscillator::new(waveform, freq, T::from_f32(1.0));
        self
    }

    /// Set carrier frequency
    pub fn set_carrier_frequency(&mut self, freq: f32) {
        self.carrier.set_frequency(freq);
        self.modulator.set_frequency(freq * self.ratio);
    }

    /// Set modulation index
    ///
    /// # Arguments
    /// * `index` - modulation index (0.0 - 10.0)
    pub fn set_modulation_index(&mut self, index: T) {
        self.modulation_index = ScalarVector1::splat(index);
        self.carrier.set_modulation_index(index);
    }

    /// Set frequency ratio
    ///
    /// # Arguments
    /// * `ratio` - ratio (modulator/carrier), typically 0.1 - 10.0
    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio;
        self.modulator
            .set_frequency(self.carrier.frequency() * ratio);
    }

    /// Get current modulation index
    pub fn modulation_index(&self) -> T {
        self.modulation_index.extract(0)
    }

    /// Get current frequency ratio
    pub fn ratio(&self) -> f32 {
        self.ratio
    }
}

impl<T: Transcendental> Algorithm<T> for SimpleFmSynth<T> {
    fn init(&mut self, sample_rate: f32) {
        self.carrier.init(sample_rate);
        self.modulator.init(sample_rate);
    }

    fn reset(&mut self) {
        self.carrier.reset();
        self.modulator.reset();
    }

    fn process(&mut self, _input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        for out in output.iter_mut() {
            // Get modulator signal
            let mod_signal = self.modulator.generate().extract(0);

            // Modulate carrier frequency
            self.carrier
                .modulate_frequency(mod_signal * self.modulation_index.extract(0));

            // Return carrier signal
            *out = self.carrier.generate().extract(0);
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Simple FM Synth",
            category: AlgorithmCategory::Generator,
            description: "2-operator FM synthesizer",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

// ==================== Generator trait implementation for SimpleFmSynth ====================

impl<T: Transcendental> Generator<T> for SimpleFmSynth<T> {
    fn phase(&self) -> T {
        self.carrier.phase()
    }

    fn set_phase(&mut self, phase: T) {
        self.carrier.set_phase(phase);
        self.modulator.set_phase(phase);
    }

    fn frequency(&self) -> f32 {
        self.carrier.frequency()
    }

    fn set_frequency(&mut self, freq: f32) {
        self.set_carrier_frequency(freq);
    }

    fn amplitude(&self) -> T {
        self.carrier.amplitude()
    }

    fn set_amplitude(&mut self, amp: T) {
        self.carrier.set_amplitude(amp);
        self.modulator.set_amplitude(amp);
    }
}

// ==================== ModulatableGenerator trait implementation for SimpleFmSynth ====================

impl<T: Transcendental> ModulatableGenerator<T> for SimpleFmSynth<T> {
    fn modulate_frequency(&mut self, amount: T) {
        self.carrier.modulate_frequency(amount);
        // Also update modulation_index accordingly
        self.modulation_index = ScalarVector1::splat(amount);
    }

    fn modulation_index(&self) -> T {
        SimpleFmSynth::modulation_index(self)
    }

    fn set_modulation_index(&mut self, index: T) {
        SimpleFmSynth::set_modulation_index(self, index);
    }
}

// =============================================================================
// Multi-operator FM synthesizer (like Yamaha DX7)
// =============================================================================

/// Multi-operator FM synthesizer
///
/// Implements architecture similar to the Yamaha DX7:
/// - N operators (typically 4 or 6)
/// - Each operator can be a carrier or modulator
/// - Flexible modulation routing matrix
/// - Individual modulation indices
///
/// # Example
/// ```
/// use rill_core_dsp::generators::*;
/// use rill_core::traits::algorithm::Algorithm;
///
/// // 6-operator FM (like DX7)
/// let frequencies = [440.0, 880.0, 1320.0, 1760.0, 2200.0, 2640.0];
/// let algorithm = [
///     [false, true,  false, false, false, false],
///     [false, false, true,  false, false, false],
///     [false, false, false, true,  false, false],
///     [false, false, false, false, true,  false],
///     [false, false, false, false, false, true],
///     [false, false, false, false, false, false],
/// ];
///
/// let mut fm = FmSynth::<f32, 6>::new(frequencies, algorithm);
/// fm.init(44100.0);
/// ```
pub struct FmSynth<T: Transcendental, const N: usize> {
    /// Operators (all use BasicOscillator)
    operators: [BasicOscillator<T>; N],
    /// Connection algorithm (routing matrix)
    /// matrix[i][j] = true means operator j modulates operator i
    algorithm: [[bool; N]; N],
    /// Modulation indices for each operator
    modulation_indices: [ScalarVector1<T>; N],
}

impl<T: Transcendental, const N: usize> FmSynth<T, N> {
    /// Create a new FM synthesizer
    ///
    /// # Arguments
    /// * `frequencies` - array of frequencies for each operator
    /// * `algorithm` - N x N modulation routing matrix
    pub fn new(frequencies: [f32; N], algorithm: [[bool; N]; N]) -> Self {
        let one = T::from_f32(1.0);
        let mut operators = [BasicOscillator::new(Waveform::Sine, 440.0, one); N];
        for i in 0..N {
            operators[i].set_frequency(frequencies[i]);
        }

        Self {
            operators,
            algorithm,
            modulation_indices: [ScalarVector1::splat(T::ZERO); N],
        }
    }

    /// Create a new FM synthesizer with all operators at the same frequency
    pub fn new_with_freq(frequency: f32, algorithm: [[bool; N]; N]) -> Self {
        let one = T::from_f32(1.0);
        let operators = [BasicOscillator::new(Waveform::Sine, frequency, one); N];

        Self {
            operators,
            algorithm,
            modulation_indices: [ScalarVector1::splat(T::ZERO); N],
        }
    }

    /// Set operator waveform
    pub fn set_waveform(&mut self, index: usize, waveform: Waveform) {
        if index < N {
            let freq = self.operators[index].frequency();
            self.operators[index] = BasicOscillator::new(waveform, freq, T::from_f32(1.0));
        }
    }

    /// Set operator frequency
    pub fn set_frequency(&mut self, index: usize, freq: f32) {
        if index < N {
            self.operators[index].set_frequency(freq);
        }
    }

    /// Set modulation index for operator
    pub fn set_modulation_index(&mut self, index: usize, idx: T) {
        if index < N {
            self.modulation_indices[index] = ScalarVector1::splat(idx);
        }
    }

    /// Get current operator value (without processing)
    pub fn peek_operator(&self, index: usize) -> T {
        if index < N {
            self.operators[index].phase()
        } else {
            T::ZERO
        }
    }

    /// Reset all operators
    pub fn reset_all(&mut self) {
        for op in &mut self.operators {
            op.reset();
        }
    }
}

impl<T: Transcendental, const N: usize> Algorithm<T> for FmSynth<T, N> {
    fn init(&mut self, sample_rate: f32) {
        for op in &mut self.operators {
            op.init(sample_rate);
        }
    }

    fn reset(&mut self) {
        self.reset_all();
    }

    fn process(&mut self, _input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        for out in output.iter_mut() {
            // Store current values of all operators
            let values: [_; N] = core::array::from_fn(|i| self.operators[i].generate().extract(0));

            // Apply modulation according to algorithm
            for (i, op) in self.operators.iter_mut().enumerate() {
                let mut mod_sum = T::ZERO;

                // Sum all modulations for this operator
                for (j, &is_mod) in self.algorithm[i].iter().enumerate() {
                    if is_mod {
                        mod_sum += values[j] * self.modulation_indices[j].extract(0);
                    }
                }

                // Apply modulation if present
                if mod_sum != T::ZERO {
                    op.modulate_frequency(mod_sum);
                }
            }

            // Last operator produces the output signal
            // (in classic FM architecture)
            *out = values[N - 1];
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        // Create static strings for different sizes
        match N {
            2 => AlgorithmMetadata {
                name: "2-operator FM Synth",
                category: AlgorithmCategory::Generator,
                description: "2-operator FM synthesizer",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
            3 => AlgorithmMetadata {
                name: "3-operator FM Synth",
                category: AlgorithmCategory::Generator,
                description: "3-operator FM synthesizer",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
            4 => AlgorithmMetadata {
                name: "4-operator FM Synth",
                category: AlgorithmCategory::Generator,
                description: "4-operator FM synthesizer (DX7 style)",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
            5 => AlgorithmMetadata {
                name: "5-operator FM Synth",
                category: AlgorithmCategory::Generator,
                description: "5-operator FM synthesizer",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
            6 => AlgorithmMetadata {
                name: "6-operator FM Synth",
                category: AlgorithmCategory::Generator,
                description: "6-operator FM synthesizer (DX7 style)",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
            _ => AlgorithmMetadata {
                name: "FM Synth",
                category: AlgorithmCategory::Generator,
                description: "Multi-operator FM synthesizer",
                author: "Rill",
                version: env!("CARGO_PKG_VERSION"),
            },
        }
    }
}

// =============================================================================
// Helper functions and constants
// =============================================================================

/// Preset algorithms for 4-operator FM
pub mod algorithms_4op {
    /// Algorithm 1: all operators in series
    pub const ALGORITHM_1: [[bool; 4]; 4] = [
        [false, true, false, false],
        [false, false, true, false],
        [false, false, false, true],
        [false, false, false, false],
    ];

    /// Algorithm 2: two parallel cascades
    pub const ALGORITHM_2: [[bool; 4]; 4] = [
        [false, true, false, false],
        [false, false, false, false],
        [false, false, false, true],
        [false, false, false, false],
    ];

    /// Algorithm 3: operators 1 and 2 modulate 3 and 4
    pub const ALGORITHM_3: [[bool; 4]; 4] = [
        [false, false, false, false],
        [false, false, false, false],
        [true, true, false, false],
        [false, false, false, false],
    ];
}

/// Preset algorithms for 6-operator FM (DX7 style)
pub mod algorithms_6op {
    /// Algorithm 1: serial chain
    pub const ALGORITHM_1: [[bool; 6]; 6] = [
        [false, true, false, false, false, false],
        [false, false, true, false, false, false],
        [false, false, false, true, false, false],
        [false, false, false, false, true, false],
        [false, false, false, false, false, true],
        [false, false, false, false, false, false],
    ];

    /// Algorithm 2: two parallel cascades of 3
    pub const ALGORITHM_2: [[bool; 6]; 6] = [
        [false, true, false, false, false, false],
        [false, false, true, false, false, false],
        [false, false, false, false, false, false],
        [false, false, false, false, true, false],
        [false, false, false, false, false, true],
        [false, false, false, false, false, false],
    ];

    /// Algorithm 3: complex structure with feedback
    pub const ALGORITHM_3: [[bool; 6]; 6] = [
        [false, true, false, false, false, false],
        [true, false, true, false, false, false],
        [false, false, false, true, false, false],
        [false, false, false, false, true, false],
        [false, false, false, false, false, true],
        [false, false, false, false, false, false],
    ];
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_fm_synth() {
        let mut fm = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5);
        fm.init(44100.0);

        let mut output = [0.0f32; 1];
        fm.process(None, &mut output).unwrap();
        let sample = output[0];
        assert!((-1.0..=1.0).contains(&sample));
    }

    #[test]
    fn test_simple_fm_with_different_waveforms() {
        let mut fm = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5)
            .with_carrier_waveform(Waveform::Saw)
            .with_modulator_waveform(Waveform::Square);
        fm.init(44100.0);

        let mut output = [0.0f32; 1];
        fm.process(None, &mut output).unwrap();
        let sample = output[0];
        assert!((-1.0..=1.0).contains(&sample));
    }

    #[test]
    fn test_simple_fm_parameters() {
        let mut fm = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5);
        fm.init(44100.0);

        assert_eq!(fm.frequency(), 440.0);
        assert_eq!(fm.ratio(), 2.0);
        assert_eq!(fm.modulation_index(), 1.5);

        fm.set_carrier_frequency(880.0);
        assert_eq!(fm.frequency(), 880.0);

        fm.set_ratio(3.0);
        assert_eq!(fm.ratio(), 3.0);

        fm.set_modulation_index(2.0);
        assert_eq!(fm.modulation_index(), 2.0);
    }

    #[test]
    fn test_fm_synth_4op() {
        let frequencies = [440.0, 880.0, 1320.0, 1760.0];
        let mut fm = FmSynth::<f32, 4>::new(frequencies, algorithms_4op::ALGORITHM_1);
        fm.init(44100.0);

        let mut output = [0.0f32; 1];
        fm.process(None, &mut output).unwrap();
        let sample = output[0];
        assert!((-1.0..=1.0).contains(&sample));
    }

    #[test]
    fn test_fm_synth_6op() {
        let frequencies = [440.0, 880.0, 1320.0, 1760.0, 2200.0, 2640.0];
        let mut fm = FmSynth::<f32, 6>::new(frequencies, algorithms_6op::ALGORITHM_1);
        fm.init(44100.0);

        let mut output = [0.0f32; 1];
        fm.process(None, &mut output).unwrap();
        let sample = output[0];
        assert!((-1.0..=1.0).contains(&sample));
    }

    #[test]
    fn test_fm_synth_set_waveform() {
        let frequencies = [440.0, 880.0];
        let algorithm = [[false, true], [false, false]];

        let mut fm = FmSynth::<f32, 2>::new(frequencies, algorithm);
        fm.init(44100.0);

        fm.set_waveform(0, Waveform::Saw);
        fm.set_waveform(1, Waveform::Square);

        let mut output = [0.0f32; 1];
        fm.process(None, &mut output).unwrap();
        let sample = output[0];
        assert!((-1.0..=1.0).contains(&sample));
    }

    #[test]
    fn test_generator_trait() {
        use crate::generators::Generator;

        let mut fm = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5);
        fm.init(44100.0);

        assert_eq!(fm.frequency(), 440.0);
        fm.set_frequency(880.0);
        assert_eq!(fm.frequency(), 880.0);

        fm.set_amplitude(0.5);
        assert_eq!(fm.amplitude(), 0.5);

        let phase = fm.phase();
        assert!((0.0..=1.0).contains(&phase));
    }

    #[test]
    fn test_modulatable_trait() {
        use crate::generators::ModulatableGenerator;

        let mut fm = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5);
        fm.init(44100.0);

        // Check initial value
        assert_eq!(fm.modulation_index(), 1.5);

        // Modulate frequency
        fm.modulate_frequency(0.3);
        assert_eq!(
            fm.modulation_index(),
            0.3,
            "modulation_index should be updated to 0.3"
        );

        // Set new modulation index
        fm.set_modulation_index(0.8);
        assert_eq!(
            fm.modulation_index(),
            0.8,
            "modulation_index should be updated to 0.8"
        );
    }

    #[test]
    fn test_clone_copy() {
        let fm1 = SimpleFmSynth::<f32>::new(440.0, 2.0, 1.5);
        let fm2 = fm1; // Copy
        let fm3 = Clone::clone(&fm1); // Explicit clone

        assert_eq!(fm1.frequency(), fm2.frequency());
        assert_eq!(fm1.frequency(), fm3.frequency());
        assert_eq!(fm1.ratio(), fm2.ratio());
    }
}
