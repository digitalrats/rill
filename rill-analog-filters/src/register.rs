#[cfg(feature = "graph")]
use rill_core::traits::{Node, NodeId, NodeVariant, Params};
#[cfg(feature = "graph")]
use rill_graph::{node_ctor, NodeFactory};

#[cfg(feature = "graph")]
pub fn register_graph_nodes<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use crate::WdfMoogLadderProcessor;

    node_ctor!(
        factory,
        "rill/analog_moog_ladder",
        |id: NodeId, params: &Params| {
            let mut n = WdfMoogLadderProcessor::<f32, BUF_SIZE>::new(params.sample_rate);
            Node::set_id(&mut n, id);
            n.cutoff = params.get_f32("cutoff", 1000.0);
            n.resonance = params.get_f32("resonance", 0.0);
            Node::init(&mut n, params.sample_rate);
            NodeVariant::Processor(Box::new(n))
        }
    );
}
