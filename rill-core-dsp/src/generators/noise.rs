//! Noise generators (White, Pink, Brown, Blue, Violet)

use super::Generator;
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use crate::filters::{FilterParams, FilterType, OnePole};
use crate::vector::prelude::*;
use rill_core::traits::{ActionContext, ProcessResult};
use rill_core::Transcendental;

/// Noise colour / spectral shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoiseType {
    /// Equal energy per Hz (flat spectrum).
    White,
    /// Equal energy per octave (3 dB/oct roll-off, 1/f).
    Pink,
    /// Brownian motion (6 dB/oct roll-off, 1/f²).
    Brown,
    /// Increasing with frequency (3 dB/oct rise).
    Blue,
    /// Strongly increasing (6 dB/oct rise).
    Violet,
}

impl NoiseType {
    /// Human-readable name of the noise type.
    pub fn name(&self) -> &'static str {
        match self {
            NoiseType::White => "White Noise",
            NoiseType::Pink => "Pink Noise",
            NoiseType::Brown => "Brown Noise",
            NoiseType::Blue => "Blue Noise",
            NoiseType::Violet => "Violet Noise",
        }
    }

    /// Short description of the noise type's spectral characteristic.
    pub fn description(&self) -> &'static str {
        match self {
            NoiseType::White => "Equal energy per Hz",
            NoiseType::Pink => "Equal energy per octave (1/f)",
            NoiseType::Brown => "Brownian motion (1/f²)",
            NoiseType::Blue => "Increasing with frequency (+3dB/oct)",
            NoiseType::Violet => "Strongly increasing (+6dB/oct)",
        }
    }
}

/// Coloured noise generator (white, pink, brown, blue, violet).
///
/// Uses a Xorshift RNG for the white noise source and applies
/// filtering / integration for the coloured variants.
pub struct NoiseGenerator<T: Transcendental> {
    noise_type: NoiseType,
    amplitude: ScalarVector1<T>,
    state: u32,
    pink_filters: [OnePole<T>; 6],
    brown_state: ScalarVector1<T>,
    sample_rate: f32,
    last_white: ScalarVector1<T>,
    last_white1: ScalarVector1<T>,
    last_white2: ScalarVector1<T>,
}

impl<T: Transcendental> NoiseGenerator<T> {
    /// Create a new noise generator with the given colour and amplitude.
    pub fn new(noise_type: NoiseType, amplitude: T) -> Self {
        // Create OnePole filters via new with correct parameters
        let filter_params = FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: 1.0,
            q: 0.707,
            gain_db: 0.0,
        };

        Self {
            noise_type,
            amplitude: ScalarVector1::splat(amplitude),
            state: 123456789,
            pink_filters: [
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params.clone()),
                OnePole::new(filter_params),
            ],
            brown_state: ScalarVector1::splat(T::ZERO),
            sample_rate: 44100.0,
            last_white: ScalarVector1::splat(T::ZERO),
            last_white1: ScalarVector1::splat(T::ZERO),
            last_white2: ScalarVector1::splat(T::ZERO),
        }
    }

    /// Xorshift RNG (operates on u32, returns f32 via Transcendental)
    #[inline(always)]
    fn xorshift(&mut self) -> T {
        let mut x = self.state;

        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;

        self.state = x;

        // Convert u32 to f32 in range [-1, 1]
        // Use upper 24 bits for uniform distribution
        let float_val = (x as f32 / 2147483648.0) - 1.0; // 2^31
        T::from_f32(float_val)
    }

    /// Generate white noise
    #[inline(always)]
    fn generate_white(&mut self) -> ScalarVector1<T> {
        ScalarVector1::splat(self.xorshift()) * self.amplitude
    }

    /// Generate pink noise (1/f)
    /// Paul Kellett's method
    fn generate_pink(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();

        // 6-band filter for 1/f approximation
        let mut output = T::ZERO;
        for filter in &mut self.pink_filters {
            output = output.add(filter.process_sample(white));
        }

        ScalarVector1::splat(output) * self.amplitude / ScalarVector1::splat(T::from_f32(3.0))
        // normalization
    }

    /// Generate brown noise (1/f²)
    fn generate_brown(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();

        // Integrator with clipping
        self.brown_state =
            self.brown_state + ScalarVector1::splat(white) * ScalarVector1::splat(T::from_f32(0.1));
        // Clipping
        let one_vec = ScalarVector1::splat(T::from_f32(1.0));
        let neg_one_vec = ScalarVector1::splat(T::from_f32(-1.0));
        self.brown_state = self.brown_state.clamp(&neg_one_vec, &one_vec);

        self.brown_state * self.amplitude
    }

    /// Generate blue noise (+3dB/octave)
    fn generate_blue(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();
        let white_vec = ScalarVector1::splat(white);

        // Differentiator (high-pass)
        let diff = white_vec - self.last_white;
        self.last_white = white_vec;

        diff * self.amplitude
    }

    /// Generate violet noise (+6dB/octave)
    fn generate_violet(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();
        let white_vec = ScalarVector1::splat(white);

        // Double differentiator
        let diff1 = white_vec - self.last_white1;
        let diff2 = diff1 - self.last_white2;
        self.last_white2 = diff1;
        self.last_white1 = white_vec;

        diff2 * self.amplitude
    }
}

impl<T: Transcendental> Algorithm<T> for NoiseGenerator<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;

        // Configure filters for pink noise
        let freqs = [5.0, 15.0, 45.0, 135.0, 405.0, 1215.0];
        for (i, &freq) in freqs.iter().enumerate() {
            // Update filter parameters via set_cutoff
            // Import the Filter trait for this
            use crate::filters::Filter;
            self.pink_filters[i].set_cutoff(freq);
        }

        self.reset();
    }

    fn reset(&mut self) {
        self.state = 123456789;
        self.brown_state = ScalarVector1::splat(T::ZERO);
        self.last_white = ScalarVector1::splat(T::ZERO);
        self.last_white1 = ScalarVector1::splat(T::ZERO);
        self.last_white2 = ScalarVector1::splat(T::ZERO);
        for filter in &mut self.pink_filters {
            filter.reset();
        }
    }

    fn process(
        &mut self,
        _input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        for out in output.iter_mut() {
            *out = match self.noise_type {
                NoiseType::White => self.generate_white().extract(0),
                NoiseType::Pink => self.generate_pink().extract(0),
                NoiseType::Brown => self.generate_brown().extract(0),
                NoiseType::Blue => self.generate_blue().extract(0),
                NoiseType::Violet => self.generate_violet().extract(0),
            };
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: self.noise_type.name(),
            category: AlgorithmCategory::Generator,
            description: self.noise_type.description(),
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental> Generator<T> for NoiseGenerator<T> {
    fn phase(&self) -> T {
        T::ZERO
    } // Noise has no phase

    fn set_phase(&mut self, _phase: T) {}

    fn frequency(&self) -> f32 {
        0.0
    }

    fn set_frequency(&mut self, _freq: f32) {}

    fn amplitude(&self) -> T {
        self.amplitude.extract(0)
    }

    fn set_amplitude(&mut self, amp: T) {
        let one = T::from_f32(1.0);
        let clamped = if amp > one {
            one
        } else if amp < T::ZERO {
            T::ZERO
        } else {
            amp
        };
        self.amplitude = ScalarVector1::splat(clamped);
    }
}
