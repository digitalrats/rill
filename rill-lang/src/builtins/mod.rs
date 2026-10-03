//! Built-in signal processing algorithms for the rill-lang DSL.
//!
//! These are standalone RT-safe structs that implement signal processing for
//! the rill-lang catalog's FFI factories. The mixer/eq/dry-wet builtins moved
//! to `rill-router` (SP-3b Task 7); this module holds the DSP-crate wrappers.

#[cfg(feature = "dsp")]
/// Builtin wrappers for the rill-core-dsp filters (biquad/onepole/moog).
pub mod filters;
#[cfg(feature = "dsp")]
/// Builtin wrappers for the rill-core-dsp generators and integrators.
pub mod generators;
#[cfg(feature = "model")]
/// Builtin wrapper for the rill-core-model WDF Moog ladder (analog_moog).
pub mod model;

/// Convert a `ParamValue` to an `f32` for `set_param` routing. Moved with the
/// wrappers from `rill-core-dsp/src/lang/mod.rs` (SP-3b Task 6); shared by the
/// dsp and model wrapper modules.
#[cfg(any(feature = "dsp", feature = "model"))]
pub(crate) fn pv_f32(value: &rill_core::traits::ParamValue) -> f32 {
    match value {
        rill_core::traits::ParamValue::Float(f) => *f,
        rill_core::traits::ParamValue::Int(i) => *i as f32,
        _ => 0.0,
    }
}
