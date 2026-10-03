//! rill-lang built-in bindings — thin aggregation over crate-level registries.

use rill_core::math::Transcendental;

/// Build a complete builtin registry: DSP primitives, oscillators, complex
/// arithmetic, mixer, EQ, dry/wet, and optionally FFT and sampler nodes.
pub fn full_registry<T: Transcendental + 'static>() -> rill_lang::builtin::Registry<T> {
    let mut reg = rill_lang::builtin::Registry::new();

    // Always available
    rill_lang::register::register_core_dsp_builtins(&mut reg);
    rill_lang::register::register_core_builtins(&mut reg);
    rill_router::register::register_lang_builtins(&mut reg);

    // Feature-gated
    #[cfg(feature = "fft")]
    rill_fft::register::register_lang_builtins(&mut reg);
    #[cfg(feature = "sampler")]
    rill_sampler::register::register_lang_builtins(&mut reg);

    reg
}

/// Build a lofi-capable registry (concrete `f32`).
#[cfg(feature = "lofi")]
pub fn full_registry_f32() -> rill_lang::builtin::Registry<f32> {
    let mut reg = full_registry::<f32>();
    rill_lofi::register::register_lang_builtins(&mut reg);
    reg
}

/// Build a complete FFI factory registry (for `compile_with_ffi`): the migrated
/// DSP/effect/router builtins plus the feature-gated fft/sampler factories.
/// Signatures come from the catalog; this registers only the Rust factories.
pub fn full_ffi<T: Transcendental + 'static>() -> rill_lang::ffi::ForeignRegistry<T> {
    let mut ffi = rill_lang::ffi::ForeignRegistry::new();

    // Always available
    rill_lang::register::register_foreign_generators(&mut ffi);
    rill_lang::register::register_foreign_filters(&mut ffi);
    rill_lang::register::register_foreign_digital_effects(&mut ffi);
    rill_router::register::register_foreign_router(&mut ffi);

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
