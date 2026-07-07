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
    #[cfg(feature = "lang")]
    register_lang::<BUF_SIZE>(factory);
    #[cfg(feature = "fft")]
    register_fft::<BUF_SIZE>(factory);
}

#[cfg(feature = "io")]
fn register_io<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_core::io::{IoCapture, IoPlayback};
    use std::sync::Arc;

    struct NullCapture;
    impl IoCapture for NullCapture {
        fn read_input(&self, _channel: usize, dst: &mut [f32]) -> usize {
            dst.fill(0.0);
            dst.len()
        }
        fn num_input_channels(&self) -> usize {
            2
        }
    }

    struct NullPlayback;
    impl IoPlayback for NullPlayback {
        fn write_output(&self, _channel: usize, _src: &[f32]) -> usize {
            _src.len()
        }
        fn num_output_channels(&self) -> usize {
            2
        }
    }

    node_ctor!(
        factory,
        "rill/output",
        move |id: NodeId, params: &Params| {
            let ch = params.get_f32("channels", 2.0) as usize;
            let null_pb = Arc::new(NullPlayback);
            let mut n = crate::io::output::Output::<f32, BUF_SIZE>::with_channels(null_pb, ch);
            Node::set_id(&mut n, id);
            Node::init(&mut n, params.sample_rate);
            NodeVariant::Sink(Box::new(n))
        }
    );

    node_ctor!(factory, "rill/input", move |id: NodeId, params: &Params| {
        let ch = params.get_f32("channels", 2.0) as usize;
        let null_cap = Arc::new(NullCapture);
        let mut n = crate::io::input::Input::<f32, BUF_SIZE>::with_channels(null_cap, ch);
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

    node_ctor!(factory, "rill/sampler", |id: NodeId, params: &Params| {
        let mut n = SamplePlayerNode::<f32, BUF_SIZE>::new();
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
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

    node_ctor!(factory, "rill/lofi_chip", |id: NodeId, params: &Params| {
        use rill_lofi::{Ay38910Chip, LofiChipSource, LofiConfig};
        let bit_depth = params.get_i32("bit_depth", 8) as u8;
        let nonlinear = params.get_bool("nonlinear", false);
        let noise_floor = params.get_f32("noise_floor", -48.0);
        let dc_offset = params.get_f32("dc_offset", 0.0);
        let output_gain = params.get_f32("output_gain", 1.0);
        let output_ceiling = params.get_f32("output_ceiling", 1.0);
        let mut config = LofiConfig::for_system(rill_lofi::ClassicSystem::Custom {
            bit_depth,
            sample_rate: params.sample_rate,
            nonlinear,
            noise_floor,
        });
        config.dc_offset = dc_offset;
        config.output_gain = output_gain;
        config.output_ceiling = output_ceiling;
        let chip = Ay38910Chip::new(1_750_000.0);
        let mut n = LofiChipSource::<Ay38910Chip, BUF_SIZE>::new(chip, config, 1);
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
// Rill Lang (DSL graph node)
// ============================================================================

#[cfg(feature = "lang")]
fn register_lang<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    node_ctor!(factory, "rill/lang", |id: NodeId, params: &Params| {
        let source = params
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("process = _;");
        let reg = std::sync::Arc::new(crate::lang_builtins::full_registry::<f32>());
        let mut n = crate::lang_node::LangNode::<f32, BUF_SIZE>::from_source_with(
            source,
            reg,
            params.sample_rate,
        )
        .unwrap_or_else(|_| crate::lang_node::LangNode::identity());
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });

    node_ctor!(factory, "rill/graph_lang", |id: NodeId, params: &Params| {
        let source = params
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("process = _;");
        let reg = std::sync::Arc::new(crate::lang_builtins::full_registry::<f32>());
        let mut n = crate::lang_node::GraphLangNode::<f32, BUF_SIZE>::from_source_with(
            source,
            reg,
            params.sample_rate,
        )
        .unwrap_or_else(|_| crate::lang_node::GraphLangNode::identity());
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });
}

// ============================================================================
// Rill Oscillators
// ============================================================================
fn register_oscillators<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_oscillators::signal::{NoiseOsc, NoiseType, SawOsc, SineOsc};

    node_ctor!(factory, "rill/sine", |id: NodeId, params: &Params| {
        let mut n = SineOsc::<f32, BUF_SIZE>::new()
            .with_frequency(params.get_f32("freq", 440.0))
            .with_amplitude(params.get_f32("amp", 0.0));
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });

    node_ctor!(factory, "rill/saw", |id: NodeId, params: &Params| {
        let mut n = SawOsc::<f32, BUF_SIZE>::new()
            .with_frequency(params.get_f32("freq", 440.0))
            .with_amplitude(params.get_f32("amp", 0.0));
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

// ============================================================================
// Module registration — custom rack modules (MIDI, OSC, etc.)
// ============================================================================

/// Register all built-in module constructors into a [`ModuleFactory`].
///
/// Called once at application startup. Modules are constructed on-demand
/// when a [`RackDef`] or [`PatchbayDef`] is processed.
pub fn register_modules(factory: &mut rill_patchbay::module_factory::ModuleFactory) {
    factory.register(rill_patchbay::servo_constructor::ServoConstructor);
    #[cfg(feature = "midi")]
    register_midi_module(factory);
    #[cfg(feature = "midi")]
    register_clock_module(factory);
    #[cfg(feature = "osc")]
    register_osc_module(factory);
}

#[cfg(feature = "midi")]
fn register_midi_module(factory: &mut rill_patchbay::module_factory::ModuleFactory) {
    use rill_core::queues::CommandEnum;
    use rill_io::midi_input::MidiInput;
    use rill_patchbay::module_def::{ModuleDef, SensorDef};
    use rill_patchbay::module_factory::{ModuleConstructor, ModuleError};

    struct MidiConstructor;

    impl ModuleConstructor for MidiConstructor {
        fn type_name(&self) -> &'static str {
            "midi"
        }

        fn construct(
            &self,
            module: &ModuleDef,
            _automaton_defs: &[rill_patchbay::module_def::AutomatonDef],
            system: &std::sync::Arc<rill_core_actor::ActorSystem>,
            graph_ref: &rill_core_actor::ActorRef<CommandEnum>,
        ) -> Result<rill_core_actor::ActorRef<CommandEnum>, ModuleError> {
            let (backend, port_name, mappings) = match module {
                ModuleDef::Sensor(SensorDef::Midi {
                    backend,
                    port_name,
                    mappings,
                }) => (backend, port_name, mappings),
                _ => {
                    return Err(ModuleError::ConstructionFailed(
                        "MidiConstructor requires ModuleDef::Sensor(SensorDef::Midi)".into(),
                    ));
                }
            };

            let be: Box<dyn MidiInput> = match backend.as_str() {
                "midir" => {
                    let b = rill_io::backends::MidirBackend::new_by_name("rill-midi", port_name)
                        .or_else(|_| rill_io::backends::MidirBackend::new("rill-midi"))
                        .map_err(|e| ModuleError::ConstructionFailed(e.to_string()))?;
                    Box::new(b)
                }
                #[cfg(feature = "alsa")]
                "alsa_seq" => Box::new(
                    rill_io::backends::AlsaSeqBackend::new(port_name)
                        .map_err(|e| ModuleError::ConstructionFailed(e.to_string()))?,
                ),
                _ => {
                    return Err(ModuleError::ConstructionFailed(format!(
                        "unknown MIDI backend '{backend}'"
                    )))
                }
            };

            let mappings: Vec<rill_patchbay::engine::Mapping> =
                mappings.iter().map(|m| m.to_mapping()).collect();

            // Create a servo to apply mappings — the sensor only decodes MIDI
            let servo_ref = rill_patchbay::Servo::new(
                format!("midi_servo_{port_name}"),
                rill_patchbay::engine::NoAction, // no automaton — mapping-only servo
                NodeId(0),
                "",
                rill_patchbay::engine::ParameterMapping::Linear,
                0.0,
                1.0,
                system.clone(),
                graph_ref.clone(),
            )
            .with_mappings(mappings)
            .spawn(system);

            // Spawn the sensor, pointing raw events to the servo
            let _sensor_ref =
                rill_patchbay::midi::spawn_midi_sensor(port_name, be, system, servo_ref.clone());
            Ok(servo_ref)
        }

        fn clone_box(&self) -> Box<dyn ModuleConstructor> {
            Box::new(MidiConstructor)
        }
    }

    factory.register(MidiConstructor);
}

#[cfg(feature = "osc")]
fn register_osc_module(factory: &mut rill_patchbay::module_factory::ModuleFactory) {
    use rill_core::queues::CommandEnum;
    use rill_patchbay::module_def::{ModuleDef, SensorDef};
    use rill_patchbay::module_factory::{ModuleConstructor, ModuleError};
    use std::net::SocketAddr;

    struct OscConstructor;

    impl ModuleConstructor for OscConstructor {
        fn type_name(&self) -> &'static str {
            "osc"
        }

        fn construct(
            &self,
            module: &ModuleDef,
            _automaton_defs: &[rill_patchbay::module_def::AutomatonDef],
            system: &std::sync::Arc<rill_core_actor::ActorSystem>,
            graph_ref: &rill_core_actor::ActorRef<CommandEnum>,
        ) -> Result<rill_core_actor::ActorRef<CommandEnum>, ModuleError> {
            let (port, mappings) = match module {
                ModuleDef::Sensor(SensorDef::Osc { port, mappings }) => (port, mappings),
                _ => {
                    return Err(ModuleError::ConstructionFailed(
                        "OscConstructor requires ModuleDef::Sensor(SensorDef::Osc)".into(),
                    ));
                }
            };

            let bind_addr = SocketAddr::from(([0, 0, 0, 0], *port));
            let mappings: Vec<rill_patchbay::engine::Mapping> =
                mappings.iter().map(|m| m.to_mapping()).collect();

            // Create a servo to apply mappings — the sensor only decodes OSC
            let servo_ref = rill_patchbay::Servo::new(
                format!("osc_servo_{port}"),
                rill_patchbay::engine::NoAction, // no automaton — mapping-only servo
                NodeId(0),
                "",
                rill_patchbay::engine::ParameterMapping::Linear,
                0.0,
                1.0,
                system.clone(),
                graph_ref.clone(),
            )
            .with_mappings(mappings)
            .spawn(system);

            // Spawn the sensor, pointing raw events to the servo
            let _sensor_ref = rill_patchbay::osc::spawn_osc_sensor(
                &format!("osc_{port}"),
                bind_addr,
                system,
                servo_ref.clone(),
            );
            Ok(servo_ref)
        }

        fn clone_box(&self) -> Box<dyn ModuleConstructor> {
            Box::new(OscConstructor)
        }
    }

    factory.register(OscConstructor);
}

#[cfg(feature = "midi")]
fn register_clock_module(factory: &mut rill_patchbay::module_factory::ModuleFactory) {
    use rill_core::queues::CommandEnum;
    use rill_core_actor::ActorRef;
    use rill_io::midi_output::MidiOutput;
    use rill_patchbay::midi_clock::spawn_midi_clock_output;
    use rill_patchbay::module_def::{ClockDef, ModuleDef};
    use rill_patchbay::module_factory::{ModuleConstructor, ModuleError};

    struct ClockConstructor;

    impl ModuleConstructor for ClockConstructor {
        fn type_name(&self) -> &'static str {
            "clock"
        }

        fn construct(
            &self,
            module: &ModuleDef,
            _automaton_defs: &[rill_patchbay::module_def::AutomatonDef],
            system: &std::sync::Arc<rill_core_actor::ActorSystem>,
            _graph_ref: &ActorRef<CommandEnum>,
        ) -> Result<ActorRef<CommandEnum>, ModuleError> {
            let (backend, port_name, auto_start) = match module {
                ModuleDef::Clock(ClockDef {
                    backend,
                    port_name,
                    auto_start,
                }) => (backend, port_name, auto_start),
                _ => {
                    return Err(ModuleError::ConstructionFailed(
                        "ClockConstructor requires ModuleDef::Clock".into(),
                    ));
                }
            };

            let output: Box<dyn MidiOutput> = match backend.as_str() {
                "midir" => {
                    let b = rill_io::backends::MidirBackend::new_output_by_name(
                        "rill-clock",
                        port_name,
                    )
                    .or_else(|_| rill_io::backends::MidirBackend::new_output("rill-clock"))
                    .map_err(|e| ModuleError::ConstructionFailed(e.to_string()))?;
                    Box::new(b)
                }
                #[cfg(feature = "alsa")]
                "alsa_seq" => Box::new(
                    rill_io::backends::AlsaSeqBackend::new_output(port_name)
                        .map_err(|e| ModuleError::ConstructionFailed(e.to_string()))?,
                ),
                _ => {
                    return Err(ModuleError::ConstructionFailed(format!(
                        "unknown MIDI output backend '{backend}'"
                    )));
                }
            };

            let clock_ref = spawn_midi_clock_output(system, output);

            if *auto_start {
                use rill_core::queues::control_event::{ControlEvent, MidiTransportKind};
                clock_ref.send(CommandEnum::Control(ControlEvent::MidiTransport {
                    kind: MidiTransportKind::Start,
                }));
            }

            Ok(clock_ref)
        }

        fn clone_box(&self) -> Box<dyn ModuleConstructor> {
            Box::new(ClockConstructor)
        }
    }

    factory.register(ClockConstructor);
}

/// Register all built-in backends into a [`BackendFactory`](rill_graph::backend_factory::BackendFactory).
#[cfg(feature = "io")]
pub fn register_backends(factory: &mut rill_graph::backend_factory::BackendFactory) {
    use std::sync::Arc;

    factory.register("null", |p| {
        let b = Arc::new(crate::io::backends::NullBackend::new(cfg_from_params(p)));
        Ok((b as Arc<dyn rill_core::io::IoDriver>, None, None))
    });

    #[cfg(feature = "alsa")]
    factory.register("alsa", |p| {
        let cfg = cfg_from_params(p);
        let in_ch = cfg.input_channels > 0;
        let out_ch = cfg.output_channels > 0;
        let b =
            Arc::new(crate::io::backends::AlsaBackend::new(cfg).map_err(|e| format!("alsa: {e}"))?);
        Ok((
            b.clone() as Arc<dyn rill_core::io::IoDriver>,
            if in_ch {
                Some(b.clone() as Arc<dyn rill_core::io::IoCapture>)
            } else {
                None
            },
            if out_ch {
                Some(b.clone() as Arc<dyn rill_core::io::IoPlayback>)
            } else {
                None
            },
        ))
    });

    #[cfg(feature = "pipewire")]
    factory.register("pipewire", |p| {
        let cfg = cfg_from_params(p);
        let in_ch = cfg.input_channels > 0;
        let out_ch = cfg.output_channels > 0;
        let be = Arc::new(
            crate::io::backends::PipewireBackend::new(cfg).map_err(|e| format!("pipewire: {e}"))?,
        );
        Ok((
            be.clone() as Arc<dyn rill_core::io::IoDriver>,
            if in_ch {
                Some(be.clone() as Arc<dyn rill_core::io::IoCapture>)
            } else {
                None
            },
            if out_ch {
                Some(be.clone() as Arc<dyn rill_core::io::IoPlayback>)
            } else {
                None
            },
        ))
    });

    #[cfg(feature = "jack")]
    factory.register("jack", |p| {
        let cfg = cfg_from_params(p);
        let out_ch = cfg.output_channels > 0;
        let b =
            Arc::new(crate::io::backends::JackBackend::new(cfg).map_err(|e| format!("jack: {e}"))?);
        Ok((
            b.clone() as Arc<dyn rill_core::io::IoDriver>,
            None,
            if out_ch {
                Some(b.clone() as Arc<dyn rill_core::io::IoPlayback>)
            } else {
                None
            },
        ))
    });

    #[cfg(feature = "portaudio")]
    factory.register("portaudio", |p| {
        let cfg = cfg_from_params(p);
        let out_ch = cfg.output_channels > 0;
        let b = Arc::new(
            crate::io::backends::PortAudioBackend::new(cfg)
                .map_err(|e| format!("portaudio: {e}"))?,
        );
        Ok((
            b.clone() as Arc<dyn rill_core::io::IoDriver>,
            None,
            if out_ch {
                Some(b.clone() as Arc<dyn rill_core::io::IoPlayback>)
            } else {
                None
            },
        ))
    });
}

#[cfg(feature = "io")]
fn cfg_from_params(p: &HashMap<String, ParamValue>) -> crate::io::AudioConfig {
    let sr = p
        .get("sample_rate")
        .and_then(|v| v.as_i32())
        .unwrap_or(44100) as u32;
    let bs = p.get("buffer_size").and_then(|v| v.as_i32()).unwrap_or(256) as u32;
    let blocks = p
        .get("buffer_blocks")
        .and_then(|v| v.as_i32())
        .filter(|&v| v > 0)
        .map(|v| v as usize)
        .unwrap_or(16);
    let ch = p.get("channels").and_then(|v| v.as_i32()).unwrap_or(2) as u32;
    let in_ch = p
        .get("input_channels")
        .and_then(|v| v.as_i32())
        .unwrap_or(0) as u32;
    let out_ch = p
        .get("output_channels")
        .and_then(|v| v.as_i32())
        .unwrap_or(ch as i32) as u32;
    let mut cfg = crate::io::AudioConfig::new()
        .with_sample_rate(sr)
        .with_buffer_size(bs)
        .with_buffer_blocks(blocks)
        .with_input_channels(in_ch)
        .with_output_channels(out_ch);
    if let Some(ParamValue::String(ref d)) = p.get("input_device") {
        cfg = cfg.with_input_device(d.as_str());
    }
    if let Some(ParamValue::String(ref d)) = p.get("output_device") {
        cfg = cfg.with_output_device(d.as_str());
    }
    cfg
}

// ============================================================================
// FFT — frequency-domain processing
// ============================================================================

#[cfg(feature = "fft")]
fn register_fft<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use rill_core::traits::Node;
    use rill_fft::nodes::convolver_node::ConvolverNode;

    node_ctor!(factory, "rill/convolver", |id: NodeId, params: &Params| {
        let ir_len = params.get_f32("ir_len", 4096.0) as usize;
        let mut n = ConvolverNode::<f32, BUF_SIZE>::new(ir_len, params.sample_rate);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });
}

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
