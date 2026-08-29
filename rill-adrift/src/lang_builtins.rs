//! rill-lang built-in bindings — thin aggregation over crate-level registries.

use rill_core::math::Transcendental;

/// Build a complete builtin registry: DSP primitives, oscillators, complex
/// arithmetic, mixer, EQ, dry/wet, and optionally analog models and FFT nodes.
pub fn full_registry<T: Transcendental + 'static>() -> rill_core::builtin::Registry<T> {
    let mut reg = rill_core::builtin::Registry::new();

    // Always available
    rill_core_dsp::lang::register::register_lang_builtins(&mut reg);
    rill_lang::register::register_core_builtins(&mut reg);
    rill_router::register::register_lang_builtins(&mut reg);
    rill_digital_effects::register::register_lang_builtins(&mut reg);

    // Feature-gated
    #[cfg(feature = "fft")]
    rill_fft::register::register_lang_builtins(&mut reg);
    #[cfg(feature = "analog")]
    rill_core_model::register::register_lang_builtins(&mut reg);
    #[cfg(feature = "analog")]
    rill_analog_effects::register::register_lang_builtins(&mut reg);
    #[cfg(feature = "sampler")]
    rill_sampler::register::register_lang_builtins(&mut reg);

    reg
}

/// Build a lofi-capable registry (concrete `f32`).
#[cfg(feature = "lofi")]
pub fn full_registry_f32() -> rill_core::builtin::Registry<f32> {
    let mut reg = full_registry::<f32>();
    rill_lofi::register::register_lang_builtins(&mut reg);
    reg
}
