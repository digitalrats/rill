//! DSL-facing builtin wrappers for rill-router's mixer/EQ/dry-wet algorithms.
//!
//! These were the `rill-lang/src/builtins` duplicates (SP-3b Task 7): the
//! underlying states (`MixerState`/`EqState`/`DryWetState`) + their
//! `BlockBuiltin`/`MultichannelBlockBuiltin` wrappers moved here so rill-router
//! owns both the real implementations (`src/mixer`, `src/eq`) and the DSL
//! surface. The `lang` feature gates the module (only rill-lang-dependent crates
//! enable it).

/// Dry/wet blend (crossfade between two signals).
pub mod dry_wet;
/// Parametric EQ: cascaded biquad bands (RBJ cookbook coefficients).
pub mod eq;
/// Multi-channel mixer: per-channel pan/volume/mute, aux sends, master volume.
pub mod mixer;
