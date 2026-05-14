//! Centralized registration of all built-in node types and backends.
//!
//! Provides `register_all_nodes` and `register_all_backends` for
//! populating factories before creating a
//! [rill_graph::GraphBuilder].

use rill_core::traits::{Node, NodeId, NodeVariant, Params};
use rill_graph::{node_ctor, NodeFactory};

#[cfg(feature = "io")]
use rill_core::traits::ParamValue;
#[cfg(feature = "io")]
use std::collections::HashMap;

/// Register every built-in node type on a
/// [rill_graph::GraphBuilder].
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
    register_router(factory);
    #[cfg(feature = "io")]
    register_io(factory);
    #[cfg(feature = "sampler")]
    register_sampler::<BUF_SIZE>(factory);
    #[cfg(feature = "lofi")]
    register_lofi::<BUF_SIZE>(factory);
    #[cfg(feature = "analog")]
    register_analog::<BUF_SIZE>(factory);
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
// Rill Lo-Fi
// ============================================================================

#[cfg(feature = "lofi")]
fn register_lofi<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_lofi::LofiProcessor;

    node_ctor!(factory, "rill/lofi", |id: NodeId, params: &Params| {
        use rill_core::traits::ParamValue;
        let mut n = LofiProcessor::<BUF_SIZE>::new(rill_lofi::LofiConfig::default());
        Node::set_id(&mut n, id);
        if let Some(v) = params.get("dry_wet").and_then(|v| v.as_f32()) {
            let _ = n.set_parameter(
                &rill_core::traits::ParameterId::new("dry_wet").unwrap(),
                ParamValue::Float(v),
            );
        }
        if let Some(v) = params.get("output_gain").and_then(|v| v.as_f32()) {
            let _ = n.set_parameter(
                &rill_core::traits::ParameterId::new("output_gain").unwrap(),
                ParamValue::Float(v),
            );
        }
        if let Some(v) = params.get("bit_depth").and_then(|v| v.as_i32()) {
            let _ = n.set_parameter(
                &rill_core::traits::ParameterId::new("bit_depth").unwrap(),
                ParamValue::Int(v),
            );
        }
        if let Some(v) = params.get("enable_bitcrush").and_then(|v| v.as_bool()) {
            let _ = n.set_parameter(
                &rill_core::traits::ParameterId::new("enable_bitcrush").unwrap(),
                ParamValue::Bool(v),
            );
        }
        if let Some(v) = params.get("enable_sr_reduction").and_then(|v| v.as_bool()) {
            let _ = n.set_parameter(
                &rill_core::traits::ParameterId::new("enable_sr_reduction").unwrap(),
                ParamValue::Bool(v),
            );
        }
        if let Some(v) = params.get("enable_noise").and_then(|v| v.as_bool()) {
            let _ = n.set_parameter(
                &rill_core::traits::ParameterId::new("enable_noise").unwrap(),
                ParamValue::Bool(v),
            );
        }
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(factory, "rill/lofi_input", |id: NodeId, params: &Params| {
        use rill_lofi::{ClassicSystem, LofiConfig, LofiInput};
        let bit_depth = params.get_i32("bit_depth", 8) as u8;
        let nonlinear = params.get_bool("nonlinear", false);
        let noise_floor = params.get_f32("noise_floor", -48.0);
        let config = LofiConfig::for_system(ClassicSystem::Custom {
            bit_depth,
            sample_rate: params.sample_rate,
            nonlinear,
            noise_floor,
        });
        let mut n = LofiInput::<f32, BUF_SIZE>::new(config);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });
}

// ============================================================================
// Rill Analog (filters + effects)
// ============================================================================

#[cfg(feature = "analog")]
fn register_analog<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_analog_effects::CassetteDeckProcessor;
    use rill_analog_filters::WdfMoogLadderProcessor;

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
    use rill_digital_filters::moog_ladder::MoogLadderProcessor;

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

    node_ctor!(
        factory,
        "rill/moog_ladder",
        |id: NodeId, params: &Params| {
            let mut n = MoogLadderProcessor::<f32, BUF_SIZE>::new(params.sample_rate);
            Node::set_id(&mut n, id);
            n.cutoff = params.get_f32("cutoff", 1000.0);
            n.resonance = params.get_f32("resonance", 0.0);
            Node::init(&mut n, params.sample_rate);
            NodeVariant::Processor(Box::new(n))
        }
    );
}

// ============================================================================
// Rill Digital Effects
// ============================================================================

fn register_digital_effects<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_digital_effects::{Delay, Distortion, DistortionType, Limiter, ReadHead, WriteHead};

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
}

// ============================================================================
// Rill Router (EQ + mixer + dry_wet)
// ============================================================================
// Rill Router (EQ + mixer)
// ============================================================================

fn register_router<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_router::eq::{GraphicEqProcessor, ParametricEqProcessor};
    use rill_router::{DryWetMix, MixerNode};

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
} // end register_router

/// Deserialise a JSON graph string into a
/// [rill_graph::serialization::GraphDef].
///
/// Use [`ModularSystem::create_builder`](crate::modular::ModularSystem::create_builder)
/// to build a graph from the definition.
#[cfg(feature = "serialization")]
pub fn load_graph_json(
    json: &str,
) -> Result<rill_graph::serialization::GraphDef, rill_graph::serialization::SerializationError> {
    rill_graph::serialization::from_json(json)
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

    #[cfg(feature = "portaudio")]
    factory.register("portaudio", |p| {
        let b = crate::io::backends::PortAudioBackend::new(cfg_from_params(p))
            .map_err(|e| format!("portaudio: {e}"))?;
        Ok(Box::new(b))
    });
}

/// Register lo‑fi chip emulation backends.
#[cfg(feature = "lofi")]
pub fn register_lofi_backends(factory: &mut rill_graph::backend_factory::BackendFactory<f32>) {
    factory.register("ay38910", |p| {
        let sr = p
            .get("sample_rate")
            .and_then(|v| v.as_f32())
            .unwrap_or(44100.0);
        let chip_clock = p
            .get("chip_clock")
            .and_then(|v| v.as_f32())
            .unwrap_or(1_750_000.0);
        Ok(Box::new(rill_lofi::Ay38910Backend::new(chip_clock, sr)))
    });
}

#[cfg(feature = "io")]
fn cfg_from_params(p: &HashMap<String, ParamValue>) -> crate::io::AudioConfig {
    let sr = p
        .get("sample_rate")
        .and_then(|v| v.as_i32())
        .unwrap_or(44100) as u32;
    let bs = p.get("buffer_size").and_then(|v| v.as_i32()).unwrap_or(256) as u32;
    let ch = p.get("channels").and_then(|v| v.as_i32()).unwrap_or(2) as u32;
    let in_ch = p
        .get("input_channels")
        .and_then(|v| v.as_i32())
        .unwrap_or(ch as i32) as u32;
    let out_ch = p
        .get("output_channels")
        .and_then(|v| v.as_i32())
        .unwrap_or(ch as i32) as u32;
    crate::io::AudioConfig::new()
        .with_sample_rate(sr)
        .with_buffer_size(bs)
        .with_input_channels(in_ch)
        .with_output_channels(out_ch)
}
