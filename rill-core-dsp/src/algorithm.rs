//! Re-exports `rill_core::Algorithm` and provides DSP-specific extensions.
//!
//! The base `Algorithm` trait lives in `rill_core`. This module re-exports it
//! together with the DSP-specific `ParameterizedAlgorithm` abstraction.

pub use rill_core::traits::algorithm::{
    ActionContext, Algorithm, AlgorithmCategory, AlgorithmMetadata,
};
pub use rill_core::traits::ProcessResult;

use rill_core::traits::ParamValue;
use rill_core::Transcendental;

/// An `Algorithm` with typed, settable parameters.
///
/// Extends the base `Algorithm` trait with the ability to get and set
/// a typed parameter struct (`Params`) and to update individual parameters
/// by name (for automation integration).
pub trait ParameterizedAlgorithm<T: Transcendental>: Algorithm<T> {
    /// The concrete parameter type for this algorithm.
    type Params: Clone + Send + Sync;

    /// Get a reference to the current parameters.
    fn params(&self) -> &Self::Params;

    /// Replace all parameters atomically.
    ///
    /// The implementation should recompute any derived coefficients.
    fn set_params(&mut self, params: Self::Params);

    /// Set a single parameter by name (for automation / scripting).
    ///
    /// Default: returns an error for any unrecognised name.
    fn set_parameter(&mut self, name: &str, _value: ParamValue) -> Result<(), &'static str> {
        Err(format!("Parameter '{}' not supported", name).leak())
    }
}
