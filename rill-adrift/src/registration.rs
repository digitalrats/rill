//! Centralized registration of all built-in node types and backends.
//!
//! Provides `register_all_nodes` and `register_all_backends` for
//! populating factories before creating a [`GraphBuilder`].

use rill_core::traits::{Node, NodeId, NodeVariant, ParamValue, Params};
use rill_graph::backend_factory::BackendFactory;
use rill_graph::{node_ctor, NodeFactory};
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "io")]
/// Register every built-in node type on a [`GraphBuilder`].
///
/// Typically called once at application startup before loading graph presets.
///
/// # Type parameters
///
/// - `BUF_SIZE` — block size, must match the target graph.
///
/// Register every built-in node type into a [`NodeFactory`].
pub fn register_all_nodes<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    register_oscillators(factory);
    register_digital_filters(factory);
    register_digital_effects(factory);
    register_io(factory);
    #[cfg(feature = "sampler")]
    register_sampler::<BUF_SIZE>(factory);
}

#[cfg(feature = "io")]
fn register_io<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    node_ctor!(factory, "rill/output", |id: NodeId, params: &Params| {
        let ch = params.get_f32("channels", 2.0) as usize;
        let mut n = crate::io::output::Output::<f32, BUF_SIZE>::with_channels(ch);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Sink(Box::new(n))
    });

    node_ctor!(factory, "rill/input", |id: NodeId, params: &Params| {
        let ch = params.get_f32("channels", 2.0) as usize;
        let mut n = crate::io::input::Input::<f32, BUF_SIZE>::with_channels(ch);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });
}

#[cfg(not(feature = "io"))]
fn register_io<const BUF_SIZE: usize>(_factory: &mut NodeFactory<f32, BUF_SIZE>) {
    // No I/O nodes available without "io" feature.
}

// ============================================================================
// Rill Sampler
// ============================================================================

#[cfg(feature = "sampler")]
fn register_sampler<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_sampler::player::SamplePlayerNode;
    use rill_sampler::wav::load_wav;

    node_ctor!(factory, "rill/sampler", |id: NodeId, params: &Params| {
        let mut n = SamplePlayerNode::<f32, BUF_SIZE>::new();
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        if let Some(path) = params.get("file").and_then(|v| v.as_str()) {
            if let Ok(sample) = load_wav(path) {
                n.load(sample);
                n.play();
            } else {
                eprintln!("SamplePlayer: could not load {path}");
            }
        }
        NodeVariant::Source(Box::new(n))
    });
}

// ============================================================================
// Rill Oscillators
// ============================================================================

fn register_oscillators<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_oscillators::audio::{NoiseOsc, NoiseType, SawOsc, SineOsc};

    node_ctor!(factory, "rill/sine", |id: NodeId, params: &Params| {
        let mut n = SineOsc::<f32, BUF_SIZE>::new()
            .with_frequency(params.get_f32("freq", 440.0))
            .with_amplitude(params.get_f32("amp", 0.5));
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });

    node_ctor!(factory, "rill/saw", |id: NodeId, params: &Params| {
        let mut n = SawOsc::<f32, BUF_SIZE>::new()
            .with_frequency(params.get_f32("freq", 440.0))
            .with_amplitude(params.get_f32("amp", 0.5));
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });

    node_ctor!(factory, "rill/noise", |id: NodeId, params: &Params| {
        let t = match params.get("type").and_then(|v| v.as_f32()) {
            Some(2.0) => NoiseType::Brown,
            Some(1.0) => NoiseType::Pink,
            _ => NoiseType::White,
        };
        let mut n = NoiseOsc::<BUF_SIZE>::new().with_type(t);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });
}

// ============================================================================
// Rill Digital Filters
// ============================================================================

fn register_digital_filters<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_core_dsp::filters::FilterType;
    use rill_digital_filters::biquad::BiquadProcessor;

    node_ctor!(factory, "rill/biquad", |id: NodeId, params: &Params| {
        let ft = match params.get("filter").and_then(|v| v.as_f32()) {
            Some(1.0) => FilterType::LowPass,
            Some(2.0) => FilterType::HighPass,
            Some(3.0) => FilterType::BandPass,
            _ => FilterType::LowPass,
        };
        let mut n = BiquadProcessor::<f32, BUF_SIZE>::new_with_params(
            ft,
            params.get_f32("cutoff", 1000.0),
            params.get_f32("q", 0.707),
            0.0,
        );
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });
}

// ============================================================================
// Rill Digital Effects
// ============================================================================

fn register_digital_effects<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_digital_effects::{Delay, Distortion, DistortionType, Limiter, ReadHead, WriteHead};
    use rill_router::{DryWetMix, MixerNode};

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

    node_ctor!(factory, "rill/write_head", |id: NodeId, params: &Params| {
        let resource = params
            .get("tape")
            .and_then(|v| v.as_str())
            .unwrap_or("tape_0");
        let mut n = WriteHead::<f32, BUF_SIZE>::with_resource(params.sample_rate, resource);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(factory, "rill/read_head", |id: NodeId, params: &Params| {
        let resource = params
            .get("tape")
            .and_then(|v| v.as_str())
            .unwrap_or("tape_0");
        let mut n = ReadHead::<f32, BUF_SIZE>::with_resource(resource);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });

    node_ctor!(factory, "rill/mixer", |id: NodeId, params: &Params| {
        let mut n = MixerNode::<BUF_SIZE>::new(4, 0);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Router(Box::new(n))
    });
}

/// Create a [`GraphBuilder`] pre-populated with the global node factory.
///
/// The factory is initialized once with all built-in node types from all
/// rill crates. Multiple calls return independent builders sharing the
/// same factory via `Arc`.
pub fn create_builder<const B: usize>() -> rill_graph::GraphBuilder<f32, B> {
    let mut node_factory = NodeFactory::new();
    let mut backend_factory = BackendFactory::new();
    register_all_nodes(&mut node_factory);
    register_backends(&mut backend_factory);
    rill_graph::GraphBuilder::new(Arc::new(node_factory), Arc::new(backend_factory))
}

/// Load a [`GraphDef`](rill_graph::serialization::GraphDef) into a
/// [`GraphBuilder`] using the global factory.
#[cfg(feature = "serialization")]
pub fn load_graph_document<const B: usize>(
    doc: rill_graph::serialization::GraphDef,
) -> Result<rill_graph::GraphBuilder<f32, B>, rill_graph::serialization::SerializationError> {
    let mut builder = create_builder::<B>();
    doc.populate(&mut builder)?;
    Ok(builder)
}

/// Deserialise a JSON graph string into a [`GraphBuilder`] using the global
/// factory. Convenience wrapper around [`from_json`].
#[cfg(feature = "serialization")]
pub fn load_graph_json<const B: usize>(
    json: &str,
) -> Result<rill_graph::GraphBuilder<f32, B>, rill_graph::serialization::SerializationError> {
    let doc = rill_graph::serialization::from_json(json)?;
    let mut builder = create_builder::<B>();
    doc.populate(&mut builder)?;
    Ok(builder)
}

/// Register all built‑in backends into a [`BackendFactory<f32>`](rill_graph::backend_factory::BackendFactory).
#[cfg(feature = "io")]
pub fn register_backends(factory: &mut rill_graph::backend_factory::BackendFactory<f32>) {
    factory.register("null", |p| {
        Ok(Box::new(crate::io::backends::NullBackend::new(
            cfg_from_params(p),
        )))
    });

    #[cfg(feature = "alsa")]
    factory.register("alsa", |p| {
        let b = crate::io::backends::AlsaBackend::new(cfg_from_params(p))
            .map_err(|e| format!("alsa: {e}"))?;
        Ok(Box::new(b))
    });

    #[cfg(feature = "cpal")]
    factory.register("cpal", |p| {
        let b = crate::io::backends::CpalBackend::new(cfg_from_params(p))
            .map_err(|e| format!("cpal: {e}"))?;
        Ok(Box::new(b))
    });

    #[cfg(feature = "pipewire")]
    factory.register("pipewire", |p| {
        let b = crate::io::backends::PipewireBackend::new(cfg_from_params(p))
            .map_err(|e| format!("pipewire: {e}"))?;
        Ok(Box::new(b))
    });

    #[cfg(feature = "jack")]
    factory.register("jack", |p| {
        let b = crate::io::backends::JackBackend::new(cfg_from_params(p))
            .map_err(|e| format!("jack: {e}"))?;
        Ok(Box::new(b))
    });
}

fn cfg_from_params(p: &HashMap<String, ParamValue>) -> crate::io::AudioConfig {
    let sr = p
        .get("sample_rate")
        .and_then(|v| v.as_i32())
        .unwrap_or(44100) as u32;
    let bs = p.get("buffer_size").and_then(|v| v.as_i32()).unwrap_or(256) as u32;
    let ch = p.get("channels").and_then(|v| v.as_i32()).unwrap_or(2) as u32;
    crate::io::AudioConfig::new()
        .with_sample_rate(sr)
        .with_buffer_size(bs)
        .with_channels(ch)
}
