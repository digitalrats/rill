/// Registration functions for rill-graph and rill-lang built-ins.
#[cfg(feature = "graph")]
pub fn register_graph_nodes<const BUF_SIZE: usize>(_factory: &mut ()) {
    // TODO: Port to rill_lang::builtin system
}

#[cfg(feature = "lang")]
pub fn register_lang_builtins(reg: &mut rill_lang::builtin::Registry<f32>) {
    crate::lang_helpers::register_lofi_builtins(reg);
    crate::lang_helpers::register_chip_builtins(reg);
}
