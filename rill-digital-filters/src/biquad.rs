//! Biquad filter implementation using rill-core-dsp
//!
//! This module provides a Processor wrapper around the `Biquad` filter from `rill-core-dsp`
//! for use in signal graphs.

use rill_core::traits::Algorithm;
use rill_core::Transcendental;
use rill_core_dsp::algorithm::ParameterizedAlgorithm;
use rill_core_dsp::filters::{Biquad, FilterParams, FilterType};

/// Biquad processor with configurable filter type and parameters.
pub struct BiquadProcessor<T: Transcendental, const BUF_SIZE: usize> {
    pub cutoff: f32,
    pub q: f32,
    pub gain_db: f32,
    pub filter_type: FilterType,
    pub algorithm: Biquad<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> BiquadProcessor<T, BUF_SIZE> {
    /// Creates a new Biquad processor with default parameters.
    pub fn new(sample_rate: f32) -> Self {
        let params = FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: 1000.0,
            q: 0.707,
            gain_db: 0.0,
        };

        let mut algorithm = Biquad::new(params);
        algorithm.init(sample_rate);

        Self {
            cutoff: 1000.0,
            q: 0.707,
            gain_db: 0.0,
            filter_type: FilterType::LowPass,
            algorithm,
        }
    }

    /// Creates a new Biquad processor with the given filter parameters.
    pub fn from_params(params: FilterParams) -> Self {
        let mut instance = Self::new(44100.0); // sample rate will be updated later
        instance.cutoff = params.cutoff;
        instance.q = params.q;
        instance.gain_db = params.gain_db;
        instance.filter_type = params.filter_type;
        instance.update_algorithm();
        instance
    }

    /// Creates a new Biquad processor with individual parameters (backward compatibility).
    pub fn new_with_params(filter_type: FilterType, cutoff: f32, q: f32, gain_db: f32) -> Self {
        let params = FilterParams {
            filter_type,
            cutoff,
            q,
            gain_db,
        };
        Self::from_params(params)
    }

    /// Returns the current cutoff frequency (Hz).
    pub fn cutoff(&self) -> f32 {
        self.cutoff
    }

    /// Sets the cutoff frequency (Hz) and updates coefficients.
    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.clamp(20.0, 20000.0);
        self.update_algorithm();
    }

    /// Returns the current Q factor.
    pub fn q(&self) -> f32 {
        self.q
    }

    /// Sets the Q factor and updates coefficients.
    pub fn set_q(&mut self, q: f32) {
        self.q = q.clamp(0.1, 20.0);
        self.update_algorithm();
    }

    /// Returns the current gain in dB (for peak/shelving filters).
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    /// Sets the gain in dB and updates coefficients.
    pub fn set_gain_db(&mut self, gain_db: f32) {
        self.gain_db = gain_db.clamp(-24.0, 24.0);
        self.update_algorithm();
    }

    /// Returns the current filter type.
    pub fn filter_type(&self) -> FilterType {
        self.filter_type
    }

    /// Sets the filter type and updates coefficients.
    pub fn set_filter_type(&mut self, filter_type: FilterType) {
        self.filter_type = filter_type;
        self.update_algorithm();
    }

    /// Returns a reference to the inner algorithm.
    pub fn algorithm(&self) -> &Biquad<T> {
        &self.algorithm
    }

    /// Returns a mutable reference to the inner algorithm.
    pub fn algorithm_mut(&mut self) -> &mut Biquad<T> {
        &mut self.algorithm
    }

    /// Updates the inner algorithm with current parameters.
    fn update_algorithm(&mut self) {
        let params = FilterParams {
            filter_type: self.filter_type,
            cutoff: self.cutoff,
            q: self.q,
            gain_db: self.gain_db,
        };
        self.algorithm.set_params(params);
    }
}
/// Re-export of the generic Biquad filter from rill-core-dsp for advanced use.
pub use rill_core_dsp::filters::Biquad as BiquadFilterGeneric;

/// Type alias for backward compatibility (f32 specialization).
/// NOTE: This type does NOT implement Processor; use `BiquadProcessor` for graph integration.
pub type BiquadFilter = BiquadFilterGeneric<f32>;

/// Extension trait providing convenience methods for Biquad filter.
pub trait BiquadExt<T> {
    /// Get cutoff frequency (Hz)
    fn cutoff(&self) -> f32;
    /// Set cutoff frequency (Hz)
    fn set_cutoff(&mut self, cutoff: f32);
    /// Get Q factor
    fn q(&self) -> f32;
    /// Set Q factor
    fn set_q(&mut self, q: f32);
    /// Get gain in dB (for peak/shelving filters)
    fn gain_db(&self) -> f32;
    /// Set gain in dB
    fn set_gain_db(&mut self, gain_db: f32);
    /// Get filter type
    fn filter_type(&self) -> FilterType;
    /// Set filter type
    fn set_filter_type(&mut self, filter_type: FilterType);
}

impl<T: rill_core::Transcendental> BiquadExt<T> for Biquad<T>
where
    Biquad<T>: ParameterizedAlgorithm<T, Params = FilterParams>,
{
    fn cutoff(&self) -> f32 {
        self.params().cutoff
    }

    fn set_cutoff(&mut self, cutoff: f32) {
        let mut params = self.params().clone();
        params.cutoff = cutoff.clamp(20.0, 20000.0);
        self.set_params(params);
    }

    fn q(&self) -> f32 {
        self.params().q
    }

    fn set_q(&mut self, q: f32) {
        let mut params = self.params().clone();
        params.q = q.clamp(0.1, 20.0);
        self.set_params(params);
    }

    fn gain_db(&self) -> f32 {
        self.params().gain_db
    }

    fn set_gain_db(&mut self, gain_db: f32) {
        let mut params = self.params().clone();
        params.gain_db = gain_db.clamp(-24.0, 24.0);
        self.set_params(params);
    }

    fn filter_type(&self) -> FilterType {
        self.params().filter_type
    }

    fn set_filter_type(&mut self, filter_type: FilterType) {
        let mut params = self.params().clone();
        params.filter_type = filter_type;
        self.set_params(params);
    }
}

/// Backward‑compatibility wrapper for `BiquadFilter::new` with four arguments.
pub fn new(filter_type: FilterType, cutoff: f32, q: f32, gain_db: f32) -> BiquadFilter {
    BiquadFilter::new(FilterParams {
        filter_type,
        cutoff,
        q,
        gain_db,
    })
}
