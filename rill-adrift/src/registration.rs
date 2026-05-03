//! Centralized node type registration for the Rill ecosystem.
//!
//! Provides a single entry point to register all built-in node types
//! from every rill crate into a [`NodeRegistry`].
//!
//! # Usage
//!
//! ```rust
//! use rill_adrift::registration::register_all;
//!
//! let mut registry = rill_adrift::rill_graph::NodeRegistry::<f32, 64>::new();
//! register_all(&mut registry);
//! ```

use std::sync::Mutex;

use rill_core::traits::{SignalNode, NodeId, NodeParams, NodeVariant, Source, Processor};
use rill_graph::{node_ctor, NodeRegistry};

// Global registries, one per block size. Lazily initialized on first access.
static REGISTRY_64: Mutex<Option<NodeRegistry<f32, 64>>> = Mutex::new(None);
static REGISTRY_128: Mutex<Option<NodeRegistry<f32, 128>>> = Mutex::new(None);
static REGISTRY_256: Mutex<Option<NodeRegistry<f32, 256>>> = Mutex::new(None);
static REGISTRY_512: Mutex<Option<NodeRegistry<f32, 512>>> = Mutex::new(None);

/// Return a lazily-initialized global registry for the given block size.
///
/// The registry is initialised once on the first call and reused thereafter.
/// This allows [`rill_graph::serialization::from_json`] and similar functions
/// to work without the caller providing a registry.
///
/// # Panics
/// Panics if `BUF_SIZE` is not one of: 64, 128, 256, 512.
pub fn registry<const BUF_SIZE: usize>() -> &'static NodeRegistry<f32, BUF_SIZE> {
    // Static registries per block size. Initialized lazily on first call.
    static R64: Mutex<Option<NodeRegistry<f32, 64>>> = Mutex::new(None);
    static R128: Mutex<Option<NodeRegistry<f32, 128>>> = Mutex::new(None);
    static R256: Mutex<Option<NodeRegistry<f32, 256>>> = Mutex::new(None);
    static R512: Mutex<Option<NodeRegistry<f32, 512>>> = Mutex::new(None);

    let guard: &Mutex<Option<NodeRegistry<f32, BUF_SIZE>>> = match BUF_SIZE {
        64 => unsafe { std::mem::transmute(&R64) },
        128 => unsafe { std::mem::transmute(&R128) },
        256 => unsafe { std::mem::transmute(&R256) },
        512 => unsafe { std::mem::transmute(&R512) },
        _ => panic!("unsupported block size {BUF_SIZE}"),
    };
    let mut lock = guard.lock().unwrap();
    if lock.is_none() {
        let mut reg = NodeRegistry::new();
        register_all(&mut reg);
        *lock = Some(reg);
    }
    let inner: &NodeRegistry<f32, BUF_SIZE> = lock.as_ref().unwrap();
    // Safety: registry is never modified after initialisation. The
    // Option is set once and never taken out, so the &'static is sound.
    unsafe { std::mem::transmute::<&NodeRegistry<f32, BUF_SIZE>, &'static NodeRegistry<f32, BUF_SIZE>>(inner) }
}

/// Register every built-in node type from all rill crates.
///
/// Typically called once at application startup before loading graph presets.
///
/// # Type parameters
///
/// - `BUF_SIZE` — block size, must match the target graph.
pub fn register_all<const BUF_SIZE: usize>(registry: &mut NodeRegistry<f32, BUF_SIZE>) {
    register_oscillators(registry);
    register_digital_filters(registry);
    register_digital_effects(registry);
}

// ============================================================================
// Rill Oscillators
// ============================================================================

fn register_oscillators<const BUF_SIZE: usize>(registry: &mut NodeRegistry<f32, BUF_SIZE>) {
    use rill_oscillators::audio::{SineOsc, SawOsc, NoiseOsc, NoiseType};

    node_ctor!(registry, "rill/sine", |id: NodeId, params: &NodeParams| {
        let mut n = SineOsc::<f32, BUF_SIZE>::new()
            .with_frequency(params.get_f32("freq", 440.0))
            .with_amplitude(params.get_f32("amp", 0.5));
        SignalNode::set_id(&mut n, id);
        SignalNode::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });

    node_ctor!(registry, "rill/saw", |id: NodeId, params: &NodeParams| {
        let mut n = SawOsc::<f32, BUF_SIZE>::new()
            .with_frequency(params.get_f32("freq", 440.0))
            .with_amplitude(params.get_f32("amp", 0.5));
        SignalNode::set_id(&mut n, id);
        SignalNode::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });

    node_ctor!(registry, "rill/noise", |id: NodeId, params: &NodeParams| {
        let t = match params.get("type").and_then(|v| v.as_f32()) {
            Some(2.0) => NoiseType::Brown,
            Some(1.0) => NoiseType::Pink,
            _ => NoiseType::White,
        };
        let mut n = NoiseOsc::<BUF_SIZE>::new().with_type(t);
        SignalNode::set_id(&mut n, id);
        SignalNode::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });
}

// ============================================================================
// Rill Digital Filters
// ============================================================================

fn register_digital_filters<const BUF_SIZE: usize>(registry: &mut NodeRegistry<f32, BUF_SIZE>) {
    use rill_core_dsp::filters::FilterType;
    use rill_digital_filters::biquad::BiquadProcessor;

    node_ctor!(registry, "rill/biquad", |id: NodeId, params: &NodeParams| {
        let ft = match params.get("filter").and_then(|v| v.as_f32()) {
            Some(1.0) => FilterType::LowPass,
            Some(2.0) => FilterType::HighPass,
            Some(3.0) => FilterType::BandPass,
            _ => FilterType::LowPass,
        };
        let mut n = BiquadProcessor::<f32, BUF_SIZE>::new_with_params(
            ft, params.get_f32("cutoff", 1000.0), params.get_f32("q", 0.707), 0.0,
        );
        SignalNode::set_id(&mut n, id);
        SignalNode::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });
}

// ============================================================================
// Rill Digital Effects
// ============================================================================

fn register_digital_effects<const BUF_SIZE: usize>(registry: &mut NodeRegistry<f32, BUF_SIZE>) {
    use rill_digital_effects::{
        Delay, Distortion, DistortionType, Limiter, DryWetMix,
        WriteHead, ReadHead,
    };
    use rill_router::MixerNode;

    node_ctor!(registry, "rill/delay", |id: NodeId, params: &NodeParams| {
        let mut n = Delay::<f32, BUF_SIZE>::with_params(
            params.sample_rate,
            params.get_f32("time", 0.3),
            params.get_f32("feedback", 0.4),
            params.get_f32("mix", 0.5),
        );
        SignalNode::set_id(&mut n, id);
        SignalNode::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(registry, "rill/distortion", |id: NodeId, params: &NodeParams| {
        let mut n = Distortion::<f32, BUF_SIZE>::with_params(
            params.sample_rate,
            DistortionType::SoftClip,
            params.get_f32("drive", 1.0),
            1.0,
        );
        SignalNode::set_id(&mut n, id);
        SignalNode::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(registry, "rill/limiter", |id: NodeId, params: &NodeParams| {
        let mut n = Limiter::<f32, BUF_SIZE>::new(
            params.sample_rate,
            params.get_f32("threshold", -6.0),
            params.get_f32("attack", 1.0),
            params.get_f32("release", 50.0),
            0.0,
        );
        SignalNode::set_id(&mut n, id);
        SignalNode::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(registry, "rill/dry_wet_mix", |id: NodeId, params: &NodeParams| {
        let mut n = DryWetMix::<f32, BUF_SIZE>::new();
        SignalNode::set_id(&mut n, id);
        SignalNode::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(registry, "rill/write_head", |id: NodeId, params: &NodeParams| {
        let mut n = WriteHead::<f32, BUF_SIZE>::new(params.sample_rate);
        SignalNode::set_id(&mut n, id);
        SignalNode::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(registry, "rill/read_head", |id: NodeId, params: &NodeParams| {
        let mut n = ReadHead::<f32, BUF_SIZE>::new();
        SignalNode::set_id(&mut n, id);
        SignalNode::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });

    node_ctor!(registry, "rill/mixer", |id: NodeId, params: &NodeParams| {
        let mut n = MixerNode::<BUF_SIZE>::new(4, 0);
        SignalNode::set_id(&mut n, id);
        SignalNode::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });
}

/// Load a [`GraphDocument`](rill_graph::serialization::GraphDocument) into a
/// [`GraphBuilder`] using the global registry. No external registry needed.
#[cfg(feature = "serialization")]
pub fn load_graph_document<const B: usize>(
    doc: rill_graph::serialization::GraphDocument,
) -> Result<rill_graph::GraphBuilder<f32, B>, rill_graph::serialization::SerializationError> {
    doc.into_builder(registry::<B>())
}

/// Deserialise a JSON graph string into a [`GraphBuilder`] using the global
/// registry. Convenience wrapper around [`from_json`] that doesn't require
/// a registry parameter.
#[cfg(feature = "serialization")]
pub fn load_graph_json<const B: usize>(
    json: &str,
) -> Result<rill_graph::GraphBuilder<f32, B>, rill_graph::serialization::SerializationError> {
    rill_graph::serialization::from_json(json, registry::<B>())
}
