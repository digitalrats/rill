//! Built-in signal processing algorithms for the rill-lang DSL.
//!
//! These are standalone RT-safe structs that implement multi-channel
//! signal processing — mixer, EQ, and dry/wet blend.

/// Dry/wet signal blend (crossfade between two signals).
pub mod dry_wet;
/// Biquad filter cascade (parametric EQ) with RBJ cookbook coefficients.
pub mod eq;
/// Multi-channel mixer with per-channel pan, volume, muting, and aux sends.
pub mod mixer;

#[cfg(feature = "dsp")]
/// Builtin wrappers for the rill-core-dsp filters (biquad/onepole/moog).
pub mod filters;
#[cfg(feature = "dsp")]
/// Builtin wrappers for the rill-core-dsp generators and integrators.
pub mod generators;

/// Convert a `ParamValue` to an `f32` for `set_param` routing. Moved with the
/// wrappers from `rill-core-dsp/src/lang/mod.rs` (SP-3b Task 6).
#[cfg(feature = "dsp")]
pub(crate) fn pv_f32(value: &rill_core::traits::ParamValue) -> f32 {
    match value {
        rill_core::traits::ParamValue::Float(f) => *f,
        rill_core::traits::ParamValue::Int(i) => *i as f32,
        _ => 0.0,
    }
}
