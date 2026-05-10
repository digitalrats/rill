//! Biquad filter

use super::{FilterParams, FilterType};
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm};
use crate::vector::{ScalarVector1, ScalarVector4, Vector};
use rill_core::math::vector::traits::Vector as VecTrait;
use rill_core::traits::{ActionContext, ProcessResult};
use rill_core::Transcendental;
use std::f32::consts::PI;

/// Pre-computed 4-sample block coefficients for SIMD biquad processing.
pub(crate) struct BiquadBlock<T: Transcendental> {
    /// Feedforward 4×4 matrix: y[i] = sum_j ff[i*4+j] * x[j], row-major
    ff: [T; 16],
    /// Feedback from old y1 state
    fb_y1: [T; 4],
    /// Feedback from old y2 state
    fb_y2: [T; 4],
    /// Feedback from old x1 state
    fb_x1: [T; 4],
    /// Feedback from old x2 state
    fb_x2: [T; 4],
}

impl<T: Transcendental> BiquadBlock<T> {
    /// Create empty block coefficients.
    fn empty() -> Self {
        Self {
            ff: [T::ZERO; 16],
            fb_y1: [T::ZERO; 4],
            fb_y2: [T::ZERO; 4],
            fb_x1: [T::ZERO; 4],
            fb_x2: [T::ZERO; 4],
        }
    }

    /// Pre-compute block coefficients from biquad coefficients.
    fn compute(&mut self, coeffs: &(T, T, T, T, T)) {
        let (b0, b1, b2, a1, a2) = coeffs;
        let neg_a1 = -(*a1);
        let neg_a2 = -(*a2);

        // Feedforward: run filter for 4 steps with impulse at each input position
        for j in 0..4 {
            let (mut x1, mut x2, mut y1, mut y2) = (T::ZERO, T::ZERO, T::ZERO, T::ZERO);
            for i in 0..4 {
                let x = if i == j { T::ONE } else { T::ZERO };
                let y = *b0 * x + *b1 * x1 + *b2 * x2 + neg_a1 * y1 + neg_a2 * y2;
                self.ff[i * 4 + j] = y;
                x2 = x1;
                x1 = x;
                y2 = y1;
                y1 = y;
            }
        }

        // Feedback from old y1 state (y1=1, others=0, no input)
        {
            let (mut x1, mut x2, mut y1, mut y2) = (T::ZERO, T::ZERO, T::ONE, T::ZERO);
            for i in 0..4 {
                let y = *b1 * x1 + *b2 * x2 + neg_a1 * y1 + neg_a2 * y2;
                self.fb_y1[i] = y;
                x2 = x1;
                x1 = T::ZERO;
                y2 = y1;
                y1 = y;
            }
        }

        // Feedback from old y2 state
        {
            let (mut x1, mut x2, mut y1, mut y2) = (T::ZERO, T::ZERO, T::ZERO, T::ONE);
            for i in 0..4 {
                let y = *b1 * x1 + *b2 * x2 + neg_a1 * y1 + neg_a2 * y2;
                self.fb_y2[i] = y;
                x2 = x1;
                x1 = T::ZERO;
                y2 = y1;
                y1 = y;
            }
        }

        // Feedback from old x1 state
        {
            let (mut x1, mut x2, mut y1, mut y2) = (T::ONE, T::ZERO, T::ZERO, T::ZERO);
            for i in 0..4 {
                let y = *b0 * T::ZERO + *b1 * x1 + *b2 * x2 + neg_a1 * y1 + neg_a2 * y2;
                self.fb_x1[i] = y;
                x2 = x1;
                x1 = T::ZERO;
                y2 = y1;
                y1 = y;
            }
        }

        // Feedback from old x2 state
        {
            let (mut x1, mut x2, mut y1, mut y2) = (T::ZERO, T::ONE, T::ZERO, T::ZERO);
            for i in 0..4 {
                let y = *b1 * x1 + *b2 * x2 + neg_a1 * y1 + neg_a2 * y2;
                self.fb_x2[i] = y;
                x2 = x1;
                x1 = T::ZERO;
                y2 = y1;
                y1 = y;
            }
        }
    }
}
#[allow(clippy::type_complexity)]
pub struct Biquad<T: Transcendental> {
    params: FilterParams,
    coeffs: (
        ScalarVector1<T>,
        ScalarVector1<T>,
        ScalarVector1<T>,
        ScalarVector1<T>,
        ScalarVector1<T>,
    ),
    state: (
        ScalarVector1<T>,
        ScalarVector1<T>,
        ScalarVector1<T>,
        ScalarVector1<T>,
    ),
    sample_rate: f32,
    block: BiquadBlock<T>,
}

impl<T: Transcendental> Biquad<T> {
    /// Create a new `Biquad` filter with the given parameters.
    ///
    /// Coefficients are computed immediately based on the filter type, cutoff,
    /// Q factor, and gain.
    pub fn new(params: FilterParams) -> Self {
        let mut filter = Self {
            params,
            coeffs: (
                ScalarVector1::splat(T::from_f32(1.0)),
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
            ),
            state: (
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
                ScalarVector1::splat(T::ZERO),
            ),
            sample_rate: 44100.0,
            block: BiquadBlock::empty(),
        };
        filter.update_coeffs();
        filter
    }

    fn update_coeffs(&mut self) {
        let omega = 2.0 * PI * self.params.cutoff / self.sample_rate;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let alpha = sin_omega / (2.0 * self.params.q);

        match self.params.filter_type {
            FilterType::LowPass => {
                let b0 = (1.0 - cos_omega) / 2.0;
                let b1 = 1.0 - cos_omega;
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;

                self.coeffs = (
                    ScalarVector1::splat(T::from_f32(b0 / a0)),
                    ScalarVector1::splat(T::from_f32(b1 / a0)),
                    ScalarVector1::splat(T::from_f32(b2 / a0)),
                    ScalarVector1::splat(T::from_f32(a1 / a0)),
                    ScalarVector1::splat(T::from_f32(a2 / a0)),
                );
            }

            FilterType::HighPass => {
                let b0 = (1.0 + cos_omega) / 2.0;
                let b1 = -(1.0 + cos_omega);
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;

                self.coeffs = (
                    ScalarVector1::splat(T::from_f32(b0 / a0)),
                    ScalarVector1::splat(T::from_f32(b1 / a0)),
                    ScalarVector1::splat(T::from_f32(b2 / a0)),
                    ScalarVector1::splat(T::from_f32(a1 / a0)),
                    ScalarVector1::splat(T::from_f32(a2 / a0)),
                );
            }

            FilterType::BandPass => {
                // Constant skirt gain (peak gain = Q)
                let b0 = sin_omega / 2.0;
                let b1 = 0.0;
                let b2 = -b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;

                self.coeffs = (
                    ScalarVector1::splat(T::from_f32(b0 / a0)),
                    ScalarVector1::splat(T::from_f32(b1 / a0)),
                    ScalarVector1::splat(T::from_f32(b2 / a0)),
                    ScalarVector1::splat(T::from_f32(a1 / a0)),
                    ScalarVector1::splat(T::from_f32(a2 / a0)),
                );
            }

            FilterType::Notch => {
                let b0 = 1.0;
                let b1 = -2.0 * cos_omega;
                let b2 = 1.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;

                self.coeffs = (
                    ScalarVector1::splat(T::from_f32(b0 / a0)),
                    ScalarVector1::splat(T::from_f32(b1 / a0)),
                    ScalarVector1::splat(T::from_f32(b2 / a0)),
                    ScalarVector1::splat(T::from_f32(a1 / a0)),
                    ScalarVector1::splat(T::from_f32(a2 / a0)),
                );
            }

            FilterType::Peak => {
                let a = 10.0_f32.powf(self.params.gain_db / 40.0);
                let b0 = 1.0 + alpha * a;
                let b1 = -2.0 * cos_omega;
                let b2 = 1.0 - alpha * a;
                let a0 = 1.0 + alpha / a;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha / a;

                self.coeffs = (
                    ScalarVector1::splat(T::from_f32(b0 / a0)),
                    ScalarVector1::splat(T::from_f32(b1 / a0)),
                    ScalarVector1::splat(T::from_f32(b2 / a0)),
                    ScalarVector1::splat(T::from_f32(a1 / a0)),
                    ScalarVector1::splat(T::from_f32(a2 / a0)),
                );
            }

            FilterType::LowShelf => {
                let a = 10.0_f32.powf(self.params.gain_db / 40.0);
                let sqrt_a = a.sqrt();
                let b0 = a * ((a + 1.0) - (a - 1.0) * cos_omega + 2.0 * sqrt_a * alpha);
                let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_omega);
                let b2 = a * ((a + 1.0) - (a - 1.0) * cos_omega - 2.0 * sqrt_a * alpha);
                let a0 = (a + 1.0) + (a - 1.0) * cos_omega + 2.0 * sqrt_a * alpha;
                let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_omega);
                let a2 = (a + 1.0) + (a - 1.0) * cos_omega - 2.0 * sqrt_a * alpha;

                self.coeffs = (
                    ScalarVector1::splat(T::from_f32(b0 / a0)),
                    ScalarVector1::splat(T::from_f32(b1 / a0)),
                    ScalarVector1::splat(T::from_f32(b2 / a0)),
                    ScalarVector1::splat(T::from_f32(a1 / a0)),
                    ScalarVector1::splat(T::from_f32(a2 / a0)),
                );
            }

            FilterType::HighShelf => {
                let a = 10.0_f32.powf(self.params.gain_db / 40.0);
                let sqrt_a = a.sqrt();
                let b0 = a * ((a + 1.0) + (a - 1.0) * cos_omega + 2.0 * sqrt_a * alpha);
                let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_omega);
                let b2 = a * ((a + 1.0) + (a - 1.0) * cos_omega - 2.0 * sqrt_a * alpha);
                let a0 = (a + 1.0) - (a - 1.0) * cos_omega + 2.0 * sqrt_a * alpha;
                let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_omega);
                let a2 = (a + 1.0) - (a - 1.0) * cos_omega - 2.0 * sqrt_a * alpha;

                self.coeffs = (
                    ScalarVector1::splat(T::from_f32(b0 / a0)),
                    ScalarVector1::splat(T::from_f32(b1 / a0)),
                    ScalarVector1::splat(T::from_f32(b2 / a0)),
                    ScalarVector1::splat(T::from_f32(a1 / a0)),
                    ScalarVector1::splat(T::from_f32(a2 / a0)),
                );
            }

            FilterType::AllPass => {
                let b0 = 1.0 - alpha;
                let b1 = -2.0 * cos_omega;
                let b2 = 1.0 + alpha;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_omega;
                let a2 = 1.0 - alpha;

                self.coeffs = (
                    ScalarVector1::splat(T::from_f32(b0 / a0)),
                    ScalarVector1::splat(T::from_f32(b1 / a0)),
                    ScalarVector1::splat(T::from_f32(b2 / a0)),
                    ScalarVector1::splat(T::from_f32(a1 / a0)),
                    ScalarVector1::splat(T::from_f32(a2 / a0)),
                );
            }
        }
        self.recompute_block();
    }

    fn recompute_block(&mut self) {
        let (b0, b1, b2, a1, a2) = self.coeffs;
        self.block.compute(&(
            b0.extract(0),
            b1.extract(0),
            b2.extract(0),
            a1.extract(0),
            a2.extract(0),
        ));
    }
}

impl<T: Transcendental> Algorithm<T> for Biquad<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_coeffs();
        self.reset();
    }

    fn reset(&mut self) {
        self.state = (
            ScalarVector1::splat(T::ZERO),
            ScalarVector1::splat(T::ZERO),
            ScalarVector1::splat(T::ZERO),
            ScalarVector1::splat(T::ZERO),
        );
    }

    fn process(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());
        let chunks = len / 4;
        let (mut x1, mut x2, mut y1, mut y2) = self.state;

        // SIMD block path: process 4 samples at once
        for chunk in 0..chunks {
            let offset = chunk * 4;
            let x = ScalarVector4::load(&input[offset..offset + 4]);
            let mut y = [T::ZERO; 4];

            // Feedforward: y[i] = sum_j ff[i*4+j] * x[j]
            for i in 0..4 {
                y[i] = self.block.ff[i * 4] * x.extract(0)
                    + self.block.ff[i * 4 + 1] * x.extract(1)
                    + self.block.ff[i * 4 + 2] * x.extract(2)
                    + self.block.ff[i * 4 + 3] * x.extract(3);
            }

            // Add state feedback
            let x1_t = x1.extract(0);
            let x2_t = x2.extract(0);
            let y1_t = y1.extract(0);
            let y2_t = y2.extract(0);
            for i in 0..4 {
                y[i] += self.block.fb_x1[i] * x1_t
                    + self.block.fb_x2[i] * x2_t
                    + self.block.fb_y1[i] * y1_t
                    + self.block.fb_y2[i] * y2_t;
            }

            output[offset..offset + 4].copy_from_slice(&y);

            // State for next block: x = last 2 inputs, y = last 2 outputs
            x2 = ScalarVector1::splat(x.extract(2));
            x1 = ScalarVector1::splat(x.extract(3));
            y2 = ScalarVector1::splat(y[2]);
            y1 = ScalarVector1::splat(y[3]);
        }

        // Scalar remainder
        let (b0, b1, b2, a1, a2) = self.coeffs;
        for i in chunks * 4..len {
            let inp = input[i];
            let out = b0 * inp + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            output[i] = out.extract(0);
            x2 = x1;
            x1 = ScalarVector1::splat(inp);
            y2 = y1;
            y1 = out;
        }

        self.state = (x1, x2, y1, y2);
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Biquad Filter",
            category: AlgorithmCategory::Filter,
            description: "Universal biquad filter",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental> ParameterizedAlgorithm<T> for Biquad<T> {
    type Params = FilterParams;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.update_coeffs();
    }
}
