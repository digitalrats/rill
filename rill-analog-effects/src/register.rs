#![allow(deprecated)]
/// Registration functions for rill-graph and rill-lang built-ins.
#[cfg(feature = "graph")]
use rill_core::traits::{Node, NodeId, NodeVariant, Params};
#[cfg(feature = "graph")]
use rill_graph::{node_ctor, NodeFactory};

#[cfg(feature = "graph")]
pub fn register_graph_nodes<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use crate::CassetteDeckProcessor;

    node_ctor!(
        factory,
        "rill/cassette_deck",
        |id: NodeId, params: &Params| {
            let mut n = CassetteDeckProcessor::<f32, BUF_SIZE>::new(params.sample_rate);
            Node::set_id(&mut n, id);
            n.tape_speed = params.get_f32("tape_speed", 4.76);
            n.bias_level = params.get_f32("bias_level", 0.8);
            n.noise_floor = params.get_f32("noise_floor", 0.0001);
            n.wow_flutter = params.get_f32("wow_flutter", 0.002);
            Node::init(&mut n, params.sample_rate);
            NodeVariant::Processor(Box::new(n))
        }
    );
}

#[cfg(feature = "lang")]
pub fn register_lang_builtins<T: rill_core::math::Transcendental>(
    reg: &mut rill_lang::builtin::Registry<T>,
) {
    crate::lang::register_analog_builtins(reg);
}
