/// Register lo-fi language builtins (vintage hardware, chips).
#[cfg(feature = "lang")]
pub fn register_lang_builtins(reg: &mut rill_lang::builtin::Registry<f32>) {
    crate::lang_helpers::register_lofi_builtins(reg);
    crate::lang_helpers::register_chip_builtins(reg);
}
