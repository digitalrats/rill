/// Register the FFT builtins as FFI factories (for `compile`).
#[cfg(feature = "lang")]
pub fn register_foreign_lang_builtins<T: rill_core::math::Transcendental + 'static>(
    ffi: &mut rill_lang::ffi::ForeignRegistry<T>,
) {
    crate::lang::register_foreign_fft(ffi);
}
