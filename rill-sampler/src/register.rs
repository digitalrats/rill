#[cfg(feature = "graph")]
use rill_core::traits::{Node, NodeId, NodeVariant, Params};
#[cfg(feature = "graph")]
use rill_graph::{node_ctor, NodeFactory};

#[cfg(feature = "graph")]
pub fn register_graph_nodes<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use crate::player::SamplePlayerNode;

    node_ctor!(factory, "rill/sampler", |id: NodeId, params: &Params| {
        let mut n = SamplePlayerNode::<f32, BUF_SIZE>::new();
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });
}
