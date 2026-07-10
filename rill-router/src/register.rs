#[cfg(feature = "graph")]
use rill_core::traits::{Node, NodeId, NodeVariant, Params};
#[cfg(feature = "graph")]
use rill_graph::{node_ctor, NodeFactory};

#[cfg(feature = "graph")]
pub fn register_graph_nodes<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use crate::eq::{GraphicEqProcessor, ParametricEqProcessor};
    use crate::{DryWetMix, MixerNode};

    node_ctor!(
        factory,
        "rill/parametric_eq",
        |id: NodeId, params: &Params| {
            let bands = params.get_f32("bands", 10.0) as usize;
            let mut n = ParametricEqProcessor::<f32, BUF_SIZE>::new(params.sample_rate, bands);
            Node::set_id(&mut n, id);
            if let Some(v) = params.get("output_gain").and_then(|v| v.as_f32()) {
                let _ = n.set_parameter(
                    &rill_core::traits::ParameterId::new("output_gain").unwrap(),
                    rill_core::traits::ParamValue::Float(v),
                );
            }
            Node::init(&mut n, params.sample_rate);
            NodeVariant::Processor(Box::new(n))
        }
    );

    node_ctor!(factory, "rill/graphic_eq", |id: NodeId, params: &Params| {
        let mut n = GraphicEqProcessor::<f32, BUF_SIZE>::new_third_octave(params.sample_rate);
        Node::set_id(&mut n, id);
        if let Some(v) = params.get("output_gain").and_then(|v| v.as_f32()) {
            let _ = n.set_parameter(
                &rill_core::traits::ParameterId::new("output_gain").unwrap(),
                rill_core::traits::ParamValue::Float(v),
            );
        }
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(
        factory,
        "rill/dry_wet_mix",
        |id: NodeId, params: &Params| {
            let mut n = DryWetMix::<f32, BUF_SIZE>::new();
            Node::set_id(&mut n, id);
            Node::init(&mut n, params.sample_rate);
            NodeVariant::Processor(Box::new(n))
        }
    );

    node_ctor!(factory, "rill/mixer", |id: NodeId, params: &Params| {
        let mut n = MixerNode::<BUF_SIZE>::new(4, 0);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Router(Box::new(n))
    });
}
