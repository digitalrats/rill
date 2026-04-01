//! Generic equalizer implementations for Kama Audio
//!
//! This module provides equalizers that work with any filter implementation
//! that implements the `Filter` trait from `kama-core-dsp`.

mod band;
mod graphic;
mod node;
mod parametric;
mod utils;

pub use band::{BandType, EqBand};
pub use graphic::GraphicEq;
pub use node::{BiquadFactory, GraphicEqProcessor, ParametricEqProcessor};
pub use parametric::ParametricEq;
pub use utils::log_spaced_frequencies;

/// Factory for creating filter instances.
pub trait FilterFactory<F: super::Filter<f32>> {
    /// Create a new filter with given parameters.
    fn create_filter(&self, filter_type: super::FilterType, frequency: f32, q: f32, gain_db: f32) -> F;
}