/// Register sampler language builtins.
#[cfg(feature = "lang")]
pub fn register_lang_builtins<T: rill_core::math::Transcendental>(
    reg: &mut rill_lang::builtin::Registry<T>,
) {
    crate::lang::register_sampler_builtins(reg);
    crate::tape::lang::register_tape_builtins(reg);
}
