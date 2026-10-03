/// Register FFT language builtins (spectral gate, spectral delay, convolver).
#[cfg(feature = "lang")]
pub fn register_lang_builtins<T: rill_core::math::Transcendental>(
    reg: &mut rill_lang::builtin::Registry<T>,
) {
    crate::lang::register_fft_builtins(reg);
}
