//! State Variable Filter (SVF)
//!
//! Advantages:
//! - Simultaneous low-pass, high-pass, and band-pass outputs
//! - Stable at high resonance
//! - Ideal for analog emulation

use super::{FilterParams, FilterType};
use crate::algorithm::ParameterizedAlgorithm;
use crate::vector::{ScalarVector1, Vector};
use core::f32::consts::PI;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;

/// State Variable Filter
///
/// Provides three simultaneous outputs:
/// - lowpass: low frequencies
/// - highpass: high frequencies
/// - bandpass: band-pass
pub struct StateVariableFilter<T: Transcendental> {
    /// Filter parameters
    params: FilterParams,
    /// Coefficients
    f: ScalarVector1<T>, // frequency
    q: ScalarVector1<T>, // resonance
    /// State
    lp: ScalarVector1<T>, // low-pass output
    hp: ScalarVector1<T>, // high-pass output
    bp: ScalarVector1<T>, // band-pass output
    /// Previous input (for delay)
    x1: ScalarVector1<T>,
    /// Sample rate
    sample_rate: f32,
}

impl<T: Transcendental> StateVariableFilter<T> {
    /// Create a new SVF
    pub fn new(params: FilterParams) -> Self {
        let mut filter = Self {
            params,
            f: ScalarVector1::splat(T::ZERO),
            q: ScalarVector1::splat(T::ZERO),
            lp: ScalarVector1::splat(T::ZERO),
            hp: ScalarVector1::splat(T::ZERO),
            bp: ScalarVector1::splat(T::ZERO),
            x1: ScalarVector1::splat(T::ZERO),
            sample_rate: 44100.0,
        };
        filter.update_coeffs();
        filter
    }

    /// Update coefficients
    fn update_coeffs(&mut self) {
        // f = 2 * sin(π * cutoff / sample_rate)
        self.f = ScalarVector1::splat(T::from_f32(
            2.0 * (PI * self.params.cutoff / self.sample_rate).sin(),
        ));
        self.q = ScalarVector1::splat(T::from_f32(self.params.q));
    }

    /// Get low-pass output
    pub fn lowpass(&self) -> T {
        self.lp.extract(0)
    }

    /// Get high-pass output
    pub fn highpass(&self) -> T {
        self.hp.extract(0)
    }

    /// Get band-pass output
    pub fn bandpass(&self) -> T {
        self.bp.extract(0)
    }
}

impl<T: Transcendental> Algorithm<T> for StateVariableFilter<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_coeffs();
        self.reset();
    }

    fn reset(&mut self) {
        self.lp = ScalarVector1::splat(T::ZERO);
        self.hp = ScalarVector1::splat(T::ZERO);
        self.bp = ScalarVector1::splat(T::ZERO);
        self.x1 = ScalarVector1::splat(T::ZERO);
    }

    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());

        for i in 0..len {
            let input_vec = ScalarVector1::splat(input[i]);
            self.lp += self.f * self.bp;
            self.hp = input_vec - self.lp - self.q * self.bp;
            self.bp = self.f * self.hp + self.bp;

            output[i] = match self.params.filter_type {
                FilterType::LowPass => self.lp.extract(0),
                FilterType::HighPass => self.hp.extract(0),
                FilterType::BandPass => self.bp.extract(0),
                _ => self.lp.extract(0), // default to low-pass
            };
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "State Variable Filter",
            category: AlgorithmCategory::Filter,
            description: "SVF with simultaneous LP/HP/BP outputs",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental> ParameterizedAlgorithm<T> for StateVariableFilter<T> {
    type Params = FilterParams;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.update_coeffs();
    }
}
