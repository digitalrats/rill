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
