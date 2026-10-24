/// Register the lo-fi + chip builtins as FFI factories (for `compile`).
#[cfg(feature = "lang")]
pub fn register_foreign_lang_builtins(ffi: &mut rill_lang::ffi::ForeignRegistry<f32>) {
    crate::lang_helpers::register_foreign_lofi_builtins(ffi);
}
