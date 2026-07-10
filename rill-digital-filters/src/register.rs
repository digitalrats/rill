#[cfg(feature = "graph")]
use rill_core::traits::{Node, NodeId, NodeVariant, Params};
#[cfg(feature = "graph")]
use rill_graph::{node_ctor, NodeFactory};

#[cfg(feature = "graph")]
pub fn register_graph_nodes<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_core_dsp::filters::FilterType;

    node_ctor!(factory, "rill/biquad", |id: NodeId, params: &Params| {
        let ft = match params.get("filter").and_then(|v| v.as_f32()) {
            Some(1.0) => FilterType::LowPass,
            Some(2.0) => FilterType::HighPass,
            Some(3.0) => FilterType::BandPass,
            _ => FilterType::LowPass,
        };
        let mut n = crate::biquad::BiquadProcessor::<f32, BUF_SIZE>::new_with_params(
            ft,
            params.get_f32("cutoff", 1000.0),
            params.get_f32("q", 0.707),
            0.0,
        );
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(
        factory,
        "rill/moog_ladder",
        |id: NodeId, params: &Params| {
            let mut n =
                crate::moog_ladder::MoogLadderProcessor::<f32, BUF_SIZE>::new(params.sample_rate);
            Node::set_id(&mut n, id);
            n.cutoff = params.get_f32("cutoff", 1000.0);
            n.resonance = params.get_f32("resonance", 0.0);
            Node::init(&mut n, params.sample_rate);
            NodeVariant::Processor(Box::new(n))
        }
    );
}
