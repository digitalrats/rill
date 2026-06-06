//! Noise generators (White, Pink, Brown, Blue, Violet)

use super::Generator;
use crate::filters::{FilterParams, FilterType, OnePole};
use crate::vector::prelude::*;
use rill_core::math::vector::scalar::ScalarVector4;
use rill_core::math::vector::traits::Vector as VecTrait;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
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

    /// Generate white noise — batched xorshift + SIMD amplitude
    fn generate_white_block(&mut self, out: &mut [T]) {
        let chunks = out.len() / 4;
        let amp = ScalarVector4::splat(self.amplitude.extract(0));

        for chunk in 0..chunks {
            let offset = chunk * 4;
            // Run xorshift 4 times in a batch
            let w0 = self.xorshift();
            let w1 = self.xorshift();
            let w2 = self.xorshift();
            let w3 = self.xorshift();

            let v = ScalarVector4::load(&[w0, w1, w2, w3]);
            v.mul(&amp).store(&mut out[offset..offset + 4]);
        }

        // Scalar remainder
        for out in out[chunks * 4..].iter_mut() {
            *out = self.generate_white().extract(0);
        }
    }

    /// Generate white noise
    #[inline(always)]
    fn generate_white(&mut self) -> ScalarVector1<T> {
        ScalarVector1::splat(self.xorshift()) * self.amplitude
    }

    /// Generate pink noise (1/f) — scalar path
    fn generate_pink_scalar(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();

        // 6-band filter for 1/f approximation
        let mut output = T::ZERO;
        for filter in &mut self.pink_filters {
            output = output.add(filter.process_sample(white));
        }

        ScalarVector1::splat(output) * self.amplitude / ScalarVector1::splat(T::from_f32(3.0))
        // normalization
    }

    /// Generate brown noise — batched integrator with SIMD amplitude
    fn generate_brown_block(&mut self, out: &mut [T]) {
        let chunks = out.len() / 4;
        let amp = self.amplitude.extract(0);
        let factor = T::from_f32(0.1);
        let one = T::ONE;
        let neg_one = -T::ONE;
        let mut state = self.brown_state.extract(0);

        for chunk in 0..chunks {
            let offset = chunk * 4;
            let w0 = self.xorshift();
            let w1 = self.xorshift();
            let w2 = self.xorshift();
            let w3 = self.xorshift();

            // Unrolled integrator with per-step clamping
            state = (state + w0 * factor).clamp(neg_one, one);
            out[offset] = state * amp;

            state = (state + w1 * factor).clamp(neg_one, one);
            out[offset + 1] = state * amp;

            state = (state + w2 * factor).clamp(neg_one, one);
            out[offset + 2] = state * amp;

            state = (state + w3 * factor).clamp(neg_one, one);
            out[offset + 3] = state * amp;
        }

        self.brown_state = ScalarVector1::splat(state);

        for out in out[chunks * 4..].iter_mut() {
            *out = self.generate_brown_scalar().extract(0);
        }
    }

    /// Generate brown noise — scalar path
    fn generate_brown_scalar(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();
        // Integrator with clipping
        self.brown_state =
            self.brown_state + ScalarVector1::splat(white) * ScalarVector1::splat(T::from_f32(0.1));
        let one_vec = ScalarVector1::splat(T::ONE);
        let neg_one_vec = ScalarVector1::splat(-T::ONE);
        self.brown_state = self.brown_state.clamp(&neg_one_vec, &one_vec);
        self.brown_state * self.amplitude
    }

    /// Generate blue noise — SIMD batch of 4
    fn generate_blue_block(&mut self, out: &mut [T]) {
        let chunks = out.len() / 4;
        let amp = self.amplitude.extract(0);
        let mut last = self.last_white.extract(0);

        for chunk in 0..chunks {
            let offset = chunk * 4;
            let w0 = self.xorshift();
            let w1 = self.xorshift();
            let w2 = self.xorshift();
            let w3 = self.xorshift();

            let white_v = ScalarVector4::load(&[w0, w1, w2, w3]);
            let shifted_v = ScalarVector4::load(&[last, w0, w1, w2]);
            let diff = white_v.sub(&shifted_v);
            diff.mul(&ScalarVector4::splat(amp))
                .store(&mut out[offset..offset + 4]);
            last = w3;
        }

        self.last_white = ScalarVector1::splat(last);

        // Scalar remainder
        for out in out[chunks * 4..].iter_mut() {
            *out = self.generate_blue_scalar().extract(0);
        }
    }

    /// Generate violet noise — SIMD batch of 4
    fn generate_violet_block(&mut self, out: &mut [T]) {
        let chunks = out.len() / 4;
        let amp = self.amplitude.extract(0);
        let mut l1 = self.last_white1.extract(0);
        let mut l2 = self.last_white2.extract(0);

        for chunk in 0..chunks {
            let offset = chunk * 4;
            let w0 = self.xorshift();
            let w1 = self.xorshift();
            let w2 = self.xorshift();
            let w3 = self.xorshift();

            // First differentiator: diff1_v = [w0-l1, w1-w0, w2-w1, w3-w2]
            let white_v = ScalarVector4::load(&[w0, w1, w2, w3]);
            let s1_v = ScalarVector4::load(&[l1, w0, w1, w2]);
            let diff1 = white_v.sub(&s1_v);

            // Second differentiator: diff2_v = [d1_0-l2, d1_1-d1_0, d1_2-d1_1, d1_3-d1_2]
            let s2_v =
                ScalarVector4::load(&[l2, diff1.extract(0), diff1.extract(1), diff1.extract(2)]);
            let diff2 = diff1.sub(&s2_v);

            diff2
                .mul(&ScalarVector4::splat(amp))
                .store(&mut out[offset..offset + 4]);
            l1 = w3;
            l2 = diff1.extract(3);
        }

        self.last_white1 = ScalarVector1::splat(l1);
        self.last_white2 = ScalarVector1::splat(l2);

        for out in out[chunks * 4..].iter_mut() {
            *out = self.generate_violet_scalar().extract(0);
        }
    }

    /// Generate blue noise — scalar path
    fn generate_blue_scalar(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();
        let white_vec = ScalarVector1::splat(white);
        let diff = white_vec - self.last_white;
        self.last_white = white_vec;
        diff * self.amplitude
    }

    /// Generate violet noise — scalar path
    fn generate_violet_scalar(&mut self) -> ScalarVector1<T> {
        let white = self.xorshift();
        let white_vec = ScalarVector1::splat(white);
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

    fn process(&mut self, _input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match self.noise_type {
            NoiseType::White => self.generate_white_block(output),
            NoiseType::Brown => self.generate_brown_block(output),
            NoiseType::Blue => self.generate_blue_block(output),
            NoiseType::Violet => self.generate_violet_block(output),
            _ => {
                for out in output.iter_mut() {
                    *out = match self.noise_type {
                        NoiseType::Pink => self.generate_pink_scalar().extract(0),
                        _ => unreachable!(),
                    };
                }
            }
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
