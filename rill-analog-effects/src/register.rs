/// Registration functions for rill-graph and rill-lang built-ins.
#[cfg(feature = "graph")]
pub fn register_graph_nodes<const BUF_SIZE: usize>(_factory: &mut ()) {
    // TODO: Port to rill_lang::builtin system
}

#[cfg(feature = "lang")]
pub fn register_lang_builtins<T: rill_core::math::Transcendental>(
    reg: &mut rill_lang::builtin::Registry<T>,
) {
    crate::lang::register_analog_builtins(reg);
}
