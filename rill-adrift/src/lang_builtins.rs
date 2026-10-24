//! rill-lang built-in bindings — thin aggregation over crate-level registries.

use rill_core::math::Transcendental;

/// Build a complete FFI factory registry: the DSP/effect/EQ builtins plus the
/// feature-gated fft/sampler factories. Hand it to
/// [`compile`](rill_lang::compile) (it becomes the world's registry) or to the
/// `WorldConfig` of a world driven by [`compile_eval`](rill_lang::compile_eval)
/// + [`test_eval`](rill_lang::test_eval).
///
/// Signatures come from the catalog; this registers only the Rust factories.
pub fn full_ffi<T: Transcendental + 'static>() -> rill_lang::ffi::ForeignRegistry<T> {
    let mut ffi = rill_lang::ffi::ForeignRegistry::new();

    // Always available
    rill_lang::register::register_foreign_generators(&mut ffi);
    rill_lang::register::register_foreign_filters(&mut ffi);
    rill_lang::register::register_foreign_digital_effects(&mut ffi);
    rill_lang::register::register_foreign_eq(&mut ffi);

    // Feature-gated
    #[cfg(feature = "fft")]
    rill_fft::register::register_foreign_lang_builtins(&mut ffi);
    #[cfg(feature = "sampler")]
    rill_sampler::register::register_foreign_lang_builtins(&mut ffi);
    #[cfg(feature = "analog")]
    rill_lang::register::register_foreign_model(&mut ffi);

    ffi
}

/// Build a lofi-capable FFI registry (concrete `f32`).
#[cfg(feature = "lofi")]
pub fn full_ffi_f32() -> rill_lang::ffi::ForeignRegistry<f32> {
    let mut ffi = full_ffi::<f32>();
    rill_lofi::register::register_foreign_lang_builtins(&mut ffi);
    ffi
}
