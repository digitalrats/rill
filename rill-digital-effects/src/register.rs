/// Registration functions for rill-graph and rill-lang built-ins.
#[cfg(feature = "graph")]
pub fn register_graph_nodes<const BUF_SIZE: usize>(_factory: &mut ()) {
    // TODO: Port to rill_lang::builtin system
}

/// Register rill-lang builtins for digital effects (delay, distortion, limiter).
pub fn register_lang_builtins<T: rill_core::math::Transcendental>(
    reg: &mut rill_core::builtin::Registry<T>,
) {
    crate::lang::register::register_lang_builtins(reg);
}
