//! Centralized registration of all built-in node types and backends.
//!
//! Provides `register_all_nodes` and `register_all_backends` for
//! populating factories before creating a `rill_graph::GraphBuilder`.
//!
//! `register_all_nodes` is deprecated — use per-crate `register_lang_builtins()`
//! with `rill_lang::builtin::Registry` instead.

#[cfg(feature = "io")]
use rill_core::traits::ParamValue;
#[cfg(feature = "io")]
use std::collections::HashMap;

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

            let servo_ref = rill_patchbay::Servo::new(
                format!("midi_servo_{port_name}"),
                rill_patchbay::engine::NoAction,
                0u32,
                "",
                rill_patchbay::engine::ParameterMapping::Linear,
                0.0,
                1.0,
                system.clone(),
                graph_ref.clone(),
            )
            .with_mappings(mappings)
            .spawn(system);

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

            let servo_ref = rill_patchbay::Servo::new(
                format!("osc_servo_{port}"),
                rill_patchbay::engine::NoAction,
                0u32,
                "",
                rill_patchbay::engine::ParameterMapping::Linear,
                0.0,
                1.0,
                system.clone(),
                graph_ref.clone(),
            )
            .with_mappings(mappings)
            .spawn(system);

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
