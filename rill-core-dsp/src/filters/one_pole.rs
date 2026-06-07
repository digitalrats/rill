//! One-pole filter
//!
//! The fastest filter, ideal for:
//! - Parameter smoothing
//! - Simple low-pass/high-pass filters
//! - Envelope followers

use super::{FilterParams, FilterType};
use crate::algorithm::ParameterizedAlgorithm;
use crate::vector::{ScalarVector1, Vector};
use core::f32::consts::PI;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;

/// One-pole filter
///
/// # Formula
/// ```text
/// y[n] = a * x[n] + (1 - a) * y[n-1]
/// ```
pub struct OnePole<T: Transcendental> {
    /// Filter parameters
    params: FilterParams,
    /// Filter coefficient
    alpha: ScalarVector1<T>,
    /// Previous output
    y1: ScalarVector1<T>,
    /// Sample rate
    sample_rate: f32,
}

impl<T: Transcendental> OnePole<T> {
    /// Create a new one-pole filter
    pub fn new(params: FilterParams) -> Self {
        let mut filter = Self {
            params,
            alpha: ScalarVector1::splat(T::ZERO),
            y1: ScalarVector1::splat(T::ZERO),
            sample_rate: 44100.0,
        };
        filter.update_alpha();
        filter
    }

    /// Update alpha coefficient
    fn update_alpha(&mut self) {
        // α = 1 - exp(-2π * cutoff / sample_rate)
        let exp_arg = -2.0 * PI * self.params.cutoff / self.sample_rate;
        self.alpha = ScalarVector1::splat(T::from_f32(1.0 - exp_arg.exp()));
    }

    /// Process a single sample
    pub fn process_sample(&mut self, input: T) -> T {
        let one = ScalarVector1::splat(T::from_f32(1.0));
        let inp = ScalarVector1::splat(input);
        let out = match self.params.filter_type {
            FilterType::LowPass => self.alpha * inp + (one - self.alpha) * self.y1,
            FilterType::HighPass => {
                // For high-pass: y[n] = α * (y[n-1] + x[n] - x[n-1])
                // Simplified via low-pass: x - lowpass(x)
                let lp = self.alpha * inp + (one - self.alpha) * self.y1;
                inp - lp
            }
            _ => inp, // Other types not supported
        };
        self.y1 = out;
        out.extract(0)
    }
}

impl<T: Transcendental> Algorithm<T> for OnePole<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_alpha();
        self.reset();
    }

    fn reset(&mut self) {
        self.y1 = ScalarVector1::splat(T::ZERO);
    }

    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());
        let one = ScalarVector1::splat(T::from_f32(1.0));

        for i in 0..len {
            let inp = input[i];
            let out = match self.params.filter_type {
                FilterType::LowPass => self.alpha * inp + (one - self.alpha) * self.y1,
                FilterType::HighPass => {
                    // For high-pass: y[n] = α * (y[n-1] + x[n] - x[n-1])
                    // Simplified via low-pass: x - lowpass(x)
                    let lp = self.alpha * inp + (one - self.alpha) * self.y1;
                    ScalarVector1::splat(inp) - lp
                }
                _ => ScalarVector1::splat(inp), // Other types not supported
            };

            self.y1 = out;
            output[i] = out.extract(0);
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "One-Pole Filter",
            category: AlgorithmCategory::Filter,
            description: "Fast one-pole filter for smoothing",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental> ParameterizedAlgorithm<T> for OnePole<T> {
    type Params = FilterParams;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.update_alpha();
    }
}
