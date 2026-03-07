//! Generic equalizer implementations for Kama Audio
//!
//! This crate provides equalizers that work with any filter implementation
//! that implements the `Filter` trait from `kama-core-dsp`.
//!
//! # Integration with kama-automation
//!
//! All parameters are exposed via `get_param`/`set_param` and can be
//! automated using `kama-automation`. When the `automation` feature is enabled,
//! the equalizer also sends `ParameterChanged` signals on parameter updates.

#![warn(missing_docs)]

mod band;
mod graphic;
mod parametric;
mod utils;

pub use band::{BandType, EqBand};
pub use graphic::GraphicEq;
pub use parametric::ParametricEq;
pub use utils::log_spaced_frequencies;

// Re-export for convenience
pub use kama_core_dsp::filters::{Filter, FilterType};

/// Factory for creating filter instances.
pub trait FilterFactory<F: Filter<f32>> {
    /// Create a new filter with given parameters.
    fn create_filter(&self, filter_type: FilterType, frequency: f32, q: f32, gain_db: f32) -> F;
}
