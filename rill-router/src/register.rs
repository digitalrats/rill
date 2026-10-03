/// Register router language builtins (legacy `Registry`).
#[cfg(feature = "lang")]
pub fn register_lang_builtins<T: rill_core::math::Transcendental>(
    reg: &mut rill_lang::builtin::Registry<T>,
) {
    crate::lang::register_router_builtins(reg);
}

/// Register the router builtins (graphic_eq/mono_to_stereo/mixer/eq_parametric/
/// dry_wet) into a [`rill_lang::ffi::ForeignRegistry`] — the SP-3b FFI path.
#[cfg(feature = "lang")]
pub fn register_foreign_router<T: rill_core::math::Transcendental + 'static>(
    ffi: &mut rill_lang::ffi::ForeignRegistry<T>,
) {
    crate::lang::register_foreign_router(ffi);
}
