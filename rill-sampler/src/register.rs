/// Register sampler language builtins.
#[cfg(feature = "lang")]
pub fn register_lang_builtins<T: rill_core::math::Transcendental>(
    reg: &mut rill_lang::builtin::Registry<T>,
) {
    crate::lang::register_sampler_builtins(reg);
    crate::tape::lang::register_tape_builtins(reg);
}

/// Register the sampler + tape-head builtins as FFI factories (for
/// `compile_with_ffi`).
#[cfg(feature = "lang")]
pub fn register_foreign_lang_builtins<T: rill_core::math::Transcendental + 'static>(
    ffi: &mut rill_lang::ffi::ForeignRegistry<T>,
) {
    crate::lang::register_foreign_sampler_builtins(ffi);
    crate::tape::lang::register_tape_ffi(ffi);
}
