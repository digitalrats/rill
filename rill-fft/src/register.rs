/// Register FFT language builtins (spectral gate, spectral delay, convolver).
#[cfg(feature = "lang")]
pub fn register_lang_builtins<T: rill_core::math::Transcendental>(
    reg: &mut rill_lang::builtin::Registry<T>,
) {
    crate::lang::register_fft_builtins(reg);
}

/// Register the FFT builtins as FFI factories (for `compile_with_ffi`).
#[cfg(feature = "lang")]
pub fn register_foreign_lang_builtins<T: rill_core::math::Transcendental + 'static>(
    ffi: &mut rill_lang::ffi::ForeignRegistry<T>,
) {
    crate::lang::register_foreign_fft(ffi);
}
