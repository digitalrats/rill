#[cfg(feature = "graph")]
use rill_core::traits::{Node, NodeId, NodeVariant, Params};
#[cfg(feature = "graph")]
use rill_graph::{node_ctor, NodeFactory};

#[cfg(feature = "graph")]
pub fn register_graph_nodes<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use crate::{Delay, Distortion, DistortionType, Limiter, ReadHead, WriteHead};

    node_ctor!(factory, "rill/delay", |id: NodeId, params: &Params| {
        let mut n = Delay::<f32, BUF_SIZE>::with_params(
            params.sample_rate,
            params.get_f32("time", 0.3),
            params.get_f32("feedback", 0.4),
            params.get_f32("mix", 0.5),
        );
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(factory, "rill/distortion", |id: NodeId, params: &Params| {
        let mut n = Distortion::<f32, BUF_SIZE>::with_params(
            params.sample_rate,
            DistortionType::SoftClip,
            params.get_f32("drive", 1.0),
            1.0,
        );
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(factory, "rill/limiter", |id: NodeId, params: &Params| {
        let mut n = Limiter::<f32, BUF_SIZE>::new(
            params.sample_rate,
            params.get_f32("threshold", -6.0),
            params.get_f32("attack", 1.0),
            params.get_f32("release", 50.0),
            0.0,
        );
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(factory, "rill/write_head", |id: NodeId, params: &Params| {
        use rill_core::traits::ParamValue;
        let resource = params
            .get("tape")
            .and_then(|v| v.as_str())
            .unwrap_or("tape_0");
        let mut n = WriteHead::<f32, BUF_SIZE>::with_resource(params.sample_rate, resource);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        if let Some(v) = params.get("delay_time").and_then(|v| v.as_f32()) {
            let _ = n.set_parameter(
                &rill_core::traits::ParameterId::new("delay_time").unwrap(),
                ParamValue::Float(v),
            );
        }
        if let Some(v) = params.get("feedback").and_then(|v| v.as_f32()) {
            let _ = n.set_parameter(
                &rill_core::traits::ParameterId::new("feedback").unwrap(),
                ParamValue::Float(v),
            );
        }
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(factory, "rill/read_head", |id: NodeId, params: &Params| {
        use rill_core::traits::ParamValue;
        let resource = params
            .get("tape")
            .and_then(|v| v.as_str())
            .unwrap_or("tape_0");
        let mut n = ReadHead::<f32, BUF_SIZE>::with_resource(resource);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        if let Some(v) = params.get("delay").and_then(|v| v.as_f32()) {
            let _ = n.set_parameter(
                &rill_core::traits::ParameterId::new("delay").unwrap(),
                ParamValue::Float(v),
            );
        }
        NodeVariant::Source(Box::new(n))
    });
}
