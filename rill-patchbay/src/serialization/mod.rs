#![allow(missing_docs)]
//! Serializable rack document types (de)serialised from JSON/CBOR.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rill_core::traits::{ParamValue, ParameterId, Params, PortId};
use rill_core::NodeId;

use crate::automaton::envelope::{EnvelopeAutomaton, EnvelopeType};
use crate::automaton::factory::{AutomatonFactory, ServoTarget};
use crate::automaton::lfo::{LfoAutomaton, LfoWaveform};
use crate::automaton::sequencer::{PlayMode, SequencerAutomaton, Step};
pub use crate::engine::EventPattern;
use crate::engine::{
    Automaton, Mapping, OscSurface, ParameterMapping, Servo, Target, Time, Transform,
};
use crate::function_registry::FunctionRegistry;
use crate::module_factory::ModuleFactory;
use crate::strategy::{ConflictStrategy, ControlStrategy};
use rill_core::queues::CommandEnum;
use rill_core_actor::{ActorRef, ActorSystem};

pub mod dot;

// ============================================================================
// AutomatonDef
// ============================================================================

/// Serializable description of a control automaton.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub enum AutomatonDef {
    Lfo {
        id: String,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
    },
    Envelope {
        id: String,
        envelope_type: EnvelopeType,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
        curve: f64,
    },
    Sequencer {
        id: String,
        steps: Vec<StepDef>,
        play_mode: PlayMode,
        tempo: f64,
    },
    NamedFunction {
        id: String,
        function_name: String,
        params: HashMap<String, f64>,
    },
    /// Custom automaton — dispatched via [`AutomatonFactory`].
    Custom {
        id: String,
        type_name: String,
        #[serde(default)]
        params: HashMap<String, ParamValue>,
    },
}

impl AutomatonDef {
    pub fn id(&self) -> &str {
        match self {
            AutomatonDef::Lfo { id, .. } => id,
            AutomatonDef::Envelope { id, .. } => id,
            AutomatonDef::Sequencer { id, .. } => id,
            AutomatonDef::NamedFunction { id, .. } => id,
            AutomatonDef::Custom { id, .. } => id,
        }
    }
}

/// Serializable step for [`AutomatonDef::Sequencer`].
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct StepDef {
    /// Duration in beat fractions (1.0 = quarter note at the given tempo).
    pub duration: f64,
}

// ============================================================================
// ServoDef
// ============================================================================

/// Type of value mapping for a servo.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MappingType {
    Linear,
    Exponential,
    Logarithmic,
    Inverted,
}

impl MappingType {
    pub fn to_parameter_mapping(self) -> ParameterMapping {
        match self {
            MappingType::Linear => ParameterMapping::Linear,
            MappingType::Exponential => ParameterMapping::Exponential,
            MappingType::Logarithmic => ParameterMapping::Logarithmic,
            MappingType::Inverted => ParameterMapping::Inverted,
        }
    }
}

/// Describes a servo: which automaton drives which node parameter.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct ServoDef {
    pub automaton_id: String,
    pub target_node: u32,
    pub target_param: String,
    pub mapping: MappingType,
    pub min: f64,
    pub max: f64,
    pub enabled: bool,

    /// Async mode: update interval in milliseconds.
    /// When `Some`, the automaton runs as a green thread (tokio task)
    /// with the given interval. When `None`, falls back to sync mode
    /// (requires manual `Patchbay::update()` calls).
    #[serde(default)]
    pub async_interval_ms: Option<f64>,

    /// Async mode: control strategy (defaults to `Absolute`).
    #[serde(default)]
    pub control_strategy: Option<ControlStrategy>,

    /// Async mode: conflict resolution (defaults to `LastWriteWins`).
    #[serde(default)]
    pub conflict_strategy: Option<ConflictStrategy>,

    /// Optional value table for index-based automata.
    /// When set, the servo looks up `table[automaton_output]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<Vec<ParamValue>>,
}

// ============================================================================
// MappingDef
// ============================================================================

/// Serializable transform (without closure variant).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub enum TransformDef {
    Linear,
    Exponential,
    Logarithmic,
    Inverted,
    NamedFunction {
        name: String,
        params: HashMap<String, f64>,
    },
}

impl TransformDef {
    pub fn to_transform(&self, registry: &FunctionRegistry) -> Transform {
        match self {
            TransformDef::Linear => Transform::Linear,
            TransformDef::Exponential => Transform::Exponential,
            TransformDef::Logarithmic => Transform::Logarithmic,
            TransformDef::Inverted => Transform::Inverted,
            TransformDef::NamedFunction { name, params } => {
                let name = name.clone();
                let params = params.clone();
                let reg = registry.clone();
                Transform::Custom(Arc::new(move |x| {
                    reg.apply(&name, x as f64, &params).unwrap_or(x as f64) as f32
                }))
            }
        }
    }
}

/// Describes a mapping from an external event to a node parameter.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct MappingDef {
    pub event_pattern: EventPattern,
    pub target_node: u32,
    pub target_param: String,
    pub transform: TransformDef,
    pub min: f64,
    pub max: f64,
    pub enabled: bool,
}

// ============================================================================
// SensorDef
// ============================================================================

/// Serializable external input sensor.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub enum SensorDef {
    /// MIDI input.
    Midi { backend: String, port_name: String },
}

impl SensorDef {
    #[cfg(feature = "midi")]
    pub fn into_sensor(&self) -> Option<Box<dyn crate::sensor::Sensor>> {
        match self {
            SensorDef::Midi { backend, port_name } => {
                use rill_io::midi_backend::MidiBackend;
                let be: Box<dyn MidiBackend> = match backend.as_str() {
                    "midir" => Box::new(rill_io::backends::MidirBackend::new(port_name).ok()?),
                    "alsa_seq" => {
                        #[cfg(feature = "alsa")]
                        {
                            Box::new(
                                rill_io::backends::AlsaSeqBackend::new(port_name)
                                    .map_err(|e| log::warn!("AlsaSeqBackend: {e}"))
                                    .ok()?,
                            )
                        }
                        #[cfg(not(feature = "alsa"))]
                        {
                            log::warn!("ALSA seq backend requires 'alsa' feature");
                            return None;
                        }
                    }
                    _ => {
                        log::warn!("unknown MIDI backend '{backend}'");
                        return None;
                    }
                };
                let hub = crate::midi::MidiHub::new(port_name.as_str(), be);
                Some(Box::new(hub))
            }
        }
    }
    #[cfg(not(feature = "midi"))]
    pub fn into_sensor(&self) -> Option<Box<dyn crate::sensor::Sensor>> {
        None
    }
}

// ============================================================================
// ModuleDef — unified servo and sensor serialization
// ============================================================================

/// A rack module — either a Servo (automaton → parameter), a Sensor (external input),
/// or a Custom module dispatched through [`ModuleFactory`](crate::module_factory::ModuleFactory).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub enum ModuleDef {
    /// Servo: automaton → graph parameter bridge.
    Servo(ServoDef),
    /// Sensor: external input (MIDI, OSC, etc.).
    Sensor(SensorDef),
    /// Custom module — dispatched through the module factory.
    Custom {
        /// Module type name for factory lookup.
        type_name: String,
        /// Module-specific parameters.
        #[serde(default)]
        params: HashMap<String, ParamValue>,
    },
}

// ============================================================================
// PatchbayDef
// ============================================================================

/// Serializable patchbay configuration — automata + modules without a signal graph.
/// For full rack configuration (graph + automata + modules), use
/// [`rill_adrift::modular::serialization::RackDef`].
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct PatchbayDef {
    pub automata: Vec<AutomatonDef>,
    /// Unified modules — servos and sensors.
    pub modules: Vec<ModuleDef>,
    pub mappings: Vec<MappingDef>,

    /// OSC → EventPattern bridge.
    #[serde(default)]
    pub osc_surface: OscSurface,

    /// Optional human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl PatchbayDef {
    pub fn new() -> Self {
        Self {
            automata: Vec::new(),
            modules: Vec::new(),
            mappings: Vec::new(),
            osc_surface: Vec::new(),
            description: None,
        }
    }

    /// Build servo actors from the rack definition.
    ///
    /// Returns a map of servo ID → [`ActorRef`] for external control.
    /// Each servo spawns its own tokio drain task.
    pub fn build_servos(
        &self,
        registry: &FunctionRegistry,
        module_factory: &ModuleFactory,
        system: &Arc<ActorSystem>,
        graph_ref: &ActorRef<CommandEnum>,
    ) -> Result<HashMap<String, ActorRef<CommandEnum>>, String> {
        let auto_ids: std::collections::HashSet<&str> =
            self.automata.iter().map(|a| a.id()).collect();

        let mut modules = HashMap::new();

        for m in &self.modules {
            match m {
                ModuleDef::Servo(s) => {
                    if !auto_ids.contains(s.automaton_id.as_str()) {
                        return Err(format!(
                            "servo references unknown automaton '{}'",
                            s.automaton_id
                        ));
                    }
                    let def = self
                        .automata
                        .iter()
                        .find(|a| a.id() == s.automaton_id)
                        .unwrap();
                    let nid = NodeId(s.target_node);
                    let mapping = s.mapping.to_parameter_mapping();

                    match def {
                        AutomatonDef::Lfo {
                            id,
                            frequency,
                            amplitude,
                            offset,
                            waveform,
                        } => {
                            let automaton =
                                LfoAutomaton::new(id, *frequency, *amplitude, *offset, *waveform);
                            let servo = Servo::new(
                                id,
                                automaton,
                                nid,
                                &s.target_param,
                                mapping,
                                s.min,
                                s.max,
                                system.clone(),
                                graph_ref.clone(),
                            );
                            let actor_ref = servo.spawn(system);
                            modules.insert(id.clone(), actor_ref);
                        }
                        AutomatonDef::Envelope {
                            id,
                            envelope_type: _,
                            attack,
                            decay,
                            sustain,
                            release,
                            curve,
                        } => {
                            let automaton =
                                EnvelopeAutomaton::adsr(id, *attack, *decay, *sustain, *release)
                                    .with_curve(*curve);
                            let servo = Servo::new(
                                id,
                                automaton,
                                nid,
                                &s.target_param,
                                mapping,
                                s.min,
                                s.max,
                                system.clone(),
                                graph_ref.clone(),
                            );
                            let actor_ref = servo.spawn(system);
                            modules.insert(id.clone(), actor_ref);
                        }
                        AutomatonDef::Sequencer {
                            id,
                            steps,
                            play_mode,
                            tempo,
                        } => {
                            let seq_steps: Vec<Step> = steps
                                .iter()
                                .map(|sd| Step {
                                    duration: sd.duration,
                                })
                                .collect();
                            let automaton = SequencerAutomaton::new(id, seq_steps)
                                .with_mode(*play_mode)
                                .with_tempo(*tempo);
                            let mut servo = Servo::new(
                                id,
                                automaton,
                                nid,
                                &s.target_param,
                                mapping,
                                s.min,
                                s.max,
                                system.clone(),
                                graph_ref.clone(),
                            );
                            if let Some(ref t) = s.table {
                                servo = servo.with_table(t.clone());
                            }
                            let actor_ref = servo.spawn(system);
                            modules.insert(id.clone(), actor_ref);
                        }
                        AutomatonDef::NamedFunction { id, .. } => {
                            log::warn!("NamedFunction automaton '{}' requires manual setup", id);
                        }
                        AutomatonDef::Custom {
                            id,
                            type_name,
                            params,
                        } => {
                            log::warn!("Custom automaton '{}' not yet supported", id);
                            let _ = (type_name, params);
                        }
                    }
                }
                ModuleDef::Sensor(_s) => {
                    // Sensors are created separately via their own factory
                    log::info!("Sensor module — needs manual setup");
                }
                ModuleDef::Custom { type_name, params } => {
                    match module_factory.construct(type_name, type_name, params, system, graph_ref)
                    {
                        Ok(module) => {
                            if let Some(handle) = module.handle() {
                                modules.insert(type_name.clone(), handle);
                            }
                        }
                        Err(e) => log::warn!("Custom module '{}' failed: {}", type_name, e),
                    }
                }
            }
        }

        Ok(modules)
    }
}

impl Default for PatchbayDef {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Serialisation helpers
// ============================================================================

#[cfg(feature = "json")]
pub fn to_json(doc: &PatchbayDef) -> Result<String, String> {
    serde_json::to_string_pretty(doc).map_err(|e| e.to_string())
}

#[cfg(feature = "json")]
pub fn from_json(json: &str) -> Result<PatchbayDef, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

#[cfg(feature = "cbor")]
pub fn to_cbor(doc: &PatchbayDef) -> Result<Vec<u8>, String> {
    serde_cbor::to_vec(doc).map_err(|e| e.to_string())
}

#[cfg(feature = "cbor")]
pub fn from_cbor(bytes: &[u8]) -> Result<PatchbayDef, String> {
    serde_cbor::from_slice(bytes).map_err(|e| e.to_string())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::queues::MpscQueue;

    fn sample_doc() -> PatchbayDef {
        PatchbayDef {
            automata: vec![AutomatonDef::Lfo {
                id: "lfo1".into(),
                frequency: 0.3,
                amplitude: 1.0,
                offset: 0.0,
                waveform: LfoWaveform::Sine,
            }],
            modules: vec![ModuleDef::Servo(ServoDef {
                automaton_id: "lfo1".into(),
                target_node: 1,
                target_param: "delay_time".into(),
                mapping: MappingType::Linear,
                min: 0.01,
                max: 0.5,
                enabled: true,
                async_interval_ms: None,
                control_strategy: None,
                conflict_strategy: None,
                table: None,
            })],
            mappings: vec![],
            osc_surface: vec![],
            description: None,
        }
    }

    #[test]
    #[cfg(feature = "json")]
    fn test_json_roundtrip() {
        let doc = sample_doc();
        let json = to_json(&doc).unwrap();
        let restored = from_json(&json).unwrap();
        assert_eq!(restored.automata.len(), 1);
        assert_eq!(restored.modules.len(), 1);
        match &restored.modules[0] {
            ModuleDef::Servo(s) => assert_eq!(s.target_param, "delay_time"),
            _ => panic!("expected Servo"),
        }
    }

    #[test]
    #[cfg(feature = "cbor")]
    fn test_cbor_roundtrip() {
        let doc = sample_doc();
        let cbor = to_cbor(&doc).unwrap();
        let restored = from_cbor(&cbor).unwrap();
        assert_eq!(restored.automata.len(), 1);
        assert_eq!(restored.automata[0].id(), "lfo1");
    }

    #[test]
    fn test_build_servos_success() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let _guard = rt.enter();
        let doc = sample_doc();
        let system = Arc::new(ActorSystem::new());
        let graph_actor = system.spawn("graph", |_: CommandEnum| {});
        let registry = FunctionRegistry::builtin();
        let module_factory = ModuleFactory::new();
        let modules = doc
            .build_servos(
                &registry,
                &module_factory,
                &system,
                &graph_actor.actor_ref(),
            )
            .unwrap();
        assert_eq!(modules.len(), 1);
        assert!(modules.contains_key("lfo1"));
    }

    #[test]
    fn test_missing_automaton_error() {
        let doc = PatchbayDef {
            automata: vec![],
            modules: vec![ModuleDef::Servo(ServoDef {
                automaton_id: "nonexistent".into(),
                target_node: 1,
                target_param: "gain".into(),
                mapping: MappingType::Linear,
                min: 0.0,
                max: 1.0,
                enabled: true,
                async_interval_ms: None,
                control_strategy: None,
                conflict_strategy: None,
                table: None,
            })],
            mappings: vec![],
            osc_surface: vec![],
            description: None,
        };
        let system = Arc::new(ActorSystem::new());
        let graph_actor = system.spawn("graph", |_: CommandEnum| {});
        let registry = FunctionRegistry::builtin();
        let module_factory = ModuleFactory::new();
        assert!(doc
            .build_servos(
                &registry,
                &module_factory,
                &system,
                &graph_actor.actor_ref()
            )
            .is_err());
    }
}
