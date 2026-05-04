//! Биквадратный фильтр (Biquad)

use super::{FilterParams, FilterType};
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm};
use crate::vector::{ScalarVector1, Vector};
use rill_core::traits::{ActionContext, ProcessResult};
use rill_core::Transcendental;
use std::f32::consts::PI;

/// Биквадратный фильтр
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
        let (b0, b1, b2, a1, a2) = self.coeffs;
        let (mut x1, mut x2, mut y1, mut y2) = self.state;

        for i in 0..len {
            let inp = input[i];
            let out = b0 * inp + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;

            output[i] = out.extract(0);

            // Update state
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
