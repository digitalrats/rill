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
//!
//! # Example with automation
//! ```
//! use kama_eq::{ParametricEq, GraphicEq};
//! use kama_digital_filters::{BiquadFilter, BiquadFactory};
//! use kama_automation::{AutomationManager, Servo, FunctionAutomaton};
//!
//! // Create equalizer
//! let mut eq = ParametricEq::new(BiquadFactory, 5, 44100.0);
//!
//! // Parameters can be automated via kama-automation
//! // eq.set_param("band_0_gain", ParamValue::Float(3.0))?;
//! ```

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
pub use kama_dsp_common::filter::{Filter, FilterFactory, FilterType};
