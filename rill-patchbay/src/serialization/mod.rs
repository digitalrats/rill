#![allow(missing_docs)]
//! Serializable patchbay document types (de)serialised from JSON/CBOR.

#[cfg(test)]
use rill_core_actor::ActorRef;
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
    Automaton, BoxedModule, Mapping, OscSurface, ParameterMapping, Patchbay, Servo, Target, Time,
    Transform,
};
use crate::function_registry::FunctionRegistry;
use crate::strategy::{ConflictStrategy, ControlStrategy};
use rill_core::queues::{SetParameter, SignalOrigin};

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
    pub value: f64,
    pub duration: f64,
    /// Curve for transition to the next step. CBOR-roundtrip safe.
    #[cfg_attr(feature = "serde", serde(default))]
    pub curve: Option<f64>,
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

/// A rack module — either a Servo (automaton → parameter) or a Sensor (external input).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub enum ModuleDef {
    /// Servo: automaton → graph parameter bridge.
    Servo(ServoDef),
    /// Sensor: external input (MIDI, OSC, etc.).
    Sensor(SensorDef),
}

// ============================================================================
// PatchbayDef
// ============================================================================

/// Serializable patchbay configuration.
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

    /// Apply the document to a [`Patchbay`].
    pub fn apply_to(
        &self,
        control: &mut Patchbay,
        registry: &FunctionRegistry,
    ) -> Result<(), String> {
        let auto_ids: std::collections::HashSet<&str> =
            self.automata.iter().map(|a| a.id()).collect();

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
                            control.add_lfo(
                                id,
                                *frequency,
                                *amplitude,
                                *offset,
                                *waveform,
                                nid,
                                &s.target_param,
                                s.min,
                                s.max,
                            );
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
                            let servo: BoxedModule = Box::new(Servo::new(
                                id,
                                automaton,
                                nid,
                                &s.target_param,
                                mapping,
                                s.min,
                                s.max,
                            ));
                            control.add_boxed_servo(id.clone(), servo);
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
                                    value: sd.value,
                                    duration: sd.duration,
                                    curve: sd.curve,
                                })
                                .collect();
                            let automaton = SequencerAutomaton::new(id, seq_steps)
                                .with_mode(*play_mode)
                                .with_tempo(*tempo);
                            let servo: BoxedModule = Box::new(Servo::new(
                                id,
                                automaton,
                                nid,
                                &s.target_param,
                                mapping,
                                s.min,
                                s.max,
                            ));
                            control.add_boxed_servo(id.clone(), servo);
                        }
                        AutomatonDef::NamedFunction { id, .. } => {
                            log::warn!("NamedFunction automaton '{}' requires manual setup", id);
                        }
                        AutomatonDef::Custom {
                            id,
                            type_name,
                            params,
                        } => {
                            let nid = NodeId(s.target_node);
                            let mapping = s.mapping.to_parameter_mapping();
                            let mut p = Params::new(48000.0);
                            for (k, v) in params {
                                p = p.with(k.clone(), v.clone());
                            }
                            let target = ServoTarget {
                                target_node: nid,
                                target_param: s.target_param.clone(),
                                mapping,
                                min: s.min,
                                max: s.max,
                                table: s.table.clone(),
                            };
                            if let Some(ref factory) = control.automaton_factory {
                                match factory.construct(type_name, id, &p, &target) {
                                    Ok(module) => {
                                        control.add_boxed_servo(id.clone(), module);
                                    }
                                    Err(e) => {
                                        log::warn!("Custom automaton '{}': {e}", id);
                                    }
                                }
                            } else {
                                log::warn!("Custom '{}' needs factory — none set", id);
                            }
                        }
                    }
                }
                ModuleDef::Sensor(s) => {
                    if let Some(mut sensor) = s.into_sensor() {
                        let events = control.event_handle();
                        sensor.attach(events);
                        sensor.start();
                        control.add_sensor("midi", sensor);
                    }
                }
            }
        }

        for m in &self.mappings {
            let transform = m.transform.to_transform(registry);
            let name = format!("{:?} -> {}", m.event_pattern, m.target_param);
            control.add_mapping(Mapping {
                pattern: m.event_pattern.clone(),
                target: Target {
                    node_id: NodeId(m.target_node),
                    param_name: m.target_param.clone(),
                    min: m.min as f32,
                    max: m.max as f32,
                },
                transform,
                name,
                enabled: m.enabled,
            });
        }

        Ok(())
    }

    /// Apply the document to a [`Patchbay`] using async automaton tasks.
    ///
    /// For each servo with `async_interval_ms: Some(...)`, creates a green
    /// thread (tokio task) with the specified strategies. Falls back to sync
    /// mode for servos without async configuration.
    ///
    /// Requires an active tokio runtime.
    pub fn apply_to_async(
        &self,
        control: &mut Patchbay,
        registry: &FunctionRegistry,
    ) -> Result<(), String> {
        let auto_ids: std::collections::HashSet<&str> =
            self.automata.iter().map(|a| a.id()).collect();

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

                    match def {
                        AutomatonDef::Lfo {
                            id,
                            frequency,
                            amplitude,
                            offset,
                            waveform,
                        } => {
                            if let Some(interval_ms) = s.async_interval_ms {
                                let id_str = id.to_string();
                                let interval = Duration::from_secs_f64(interval_ms / 1000.0);
                                let automaton = LfoAutomaton::new(
                                    &id_str, *frequency, *amplitude, *offset, *waveform,
                                );
                                let mapping = s.mapping.to_parameter_mapping();
                                let command_queue = control.command_queue();
                                let node_id = nid;
                                let param = s.target_param.clone();
                                let min = s.min;
                                let max = s.max;
                                let task_id = id_str.clone();

                                let handle = tokio::spawn(async move {
                                    let mut internal = automaton.initial_internal();
                                    let mut current = ParamValue::Float(0.0);
                                    let mut time: Time = 0.0;
                                    let mut ticker = tokio::time::interval(interval);
                                    ticker.tick().await;
                                    loop {
                                        ticker.tick().await;
                                        time += interval.as_secs_f64();
                                        current = automaton.step(
                                            &mut internal,
                                            &current,
                                            time,
                                            &<LfoAutomaton as Automaton>::Action::default(),
                                        );
                                        let raw = current.as_f32().unwrap_or(0.0) as f64;
                                        let mapped = mapping.apply(raw);
                                        let value = min + mapped * (max - min);
                                        let pid = ParameterId::new(&param).unwrap();
                                        command_queue.send(SetParameter::new(
                                            PortId::param(node_id, 0),
                                            pid,
                                            ParamValue::Float(value as f32),
                                            SignalOrigin::Automaton(id_str.clone()),
                                        ));
                                    }
                                });
                                control.store_task_handle(task_id, handle);
                            } else {
                                control.add_lfo(
                                    id,
                                    *frequency,
                                    *amplitude,
                                    *offset,
                                    *waveform,
                                    nid,
                                    &s.target_param,
                                    s.min,
                                    s.max,
                                );
                            }
                        }
                        AutomatonDef::Envelope {
                            id,
                            attack,
                            decay,
                            sustain,
                            release,
                            curve,
                            ..
                        } => {
                            if let Some(interval_ms) = s.async_interval_ms {
                                let id_str = id.to_string();
                                let interval = Duration::from_secs_f64(interval_ms / 1000.0);
                                let automaton = EnvelopeAutomaton::adsr(
                                    &id_str, *attack, *decay, *sustain, *release,
                                )
                                .with_curve(*curve);
                                let mapping = s.mapping.to_parameter_mapping();
                                let command_queue = control.command_queue();
                                let node_id = nid;
                                let param = s.target_param.clone();
                                let min = s.min;
                                let max = s.max;
                                let task_id = id_str.clone();

                                let handle = tokio::spawn(async move {
                                    let mut internal = automaton.initial_internal();
                                    let mut current = ParamValue::Float(0.0);
                                    let mut time: Time = 0.0;
                                    let mut ticker = tokio::time::interval(interval);
                                    ticker.tick().await;
                                    loop {
                                        ticker.tick().await;
                                        time += interval.as_secs_f64();
                                        current = automaton.step(
                                            &mut internal,
                                            &current,
                                            time,
                                            &<EnvelopeAutomaton as Automaton>::Action::default(),
                                        );
                                        let raw = current.as_f32().unwrap_or(0.0) as f64;
                                        let mapped = mapping.apply(raw);
                                        let value = min + mapped * (max - min);
                                        let pid = ParameterId::new(&param).unwrap();
                                        command_queue.send(SetParameter::new(
                                            PortId::param(node_id, 0),
                                            pid,
                                            ParamValue::Float(value as f32),
                                            SignalOrigin::Automaton(id_str.clone()),
                                        ));
                                    }
                                });
                                control.store_task_handle(task_id, handle);
                            } else {
                                let automaton = EnvelopeAutomaton::adsr(
                                    id, *attack, *decay, *sustain, *release,
                                )
                                .with_curve(*curve);
                                let mapping = s.mapping.to_parameter_mapping();
                                let servo: BoxedModule = Box::new(Servo::new(
                                    id,
                                    automaton,
                                    nid,
                                    &s.target_param,
                                    mapping,
                                    s.min,
                                    s.max,
                                ));
                                control.add_boxed_servo(id.clone(), servo);
                            }
                        }
                        AutomatonDef::Sequencer {
                            id,
                            steps,
                            play_mode,
                            tempo,
                        } => {
                            log::warn!(
                                "Sequencer sync mode not fully wired in apply_to_async; use manual setup"
                            );
                            let _ = (id, steps, play_mode, tempo, nid, s);
                        }
                        AutomatonDef::NamedFunction { id, .. } => {
                            log::warn!("NamedFunction automaton '{}' requires manual setup", id);
                        }
                        AutomatonDef::Custom { id, .. } => {
                            log::warn!(
                                "Custom automaton '{}' not supported in async mode; use sync",
                                id
                            );
                        }
                    }
                }
                ModuleDef::Sensor(s) => {
                    if let Some(mut sensor) = s.into_sensor() {
                        let events = control.event_handle();
                        sensor.attach(events);
                        sensor.start();
                        control.add_sensor("midi", sensor);
                    }
                }
            }
        }

        for m in &self.mappings {
            let transform = m.transform.to_transform(registry);
            control.add_mapping(Mapping {
                pattern: m.event_pattern.clone(),
                target: Target {
                    node_id: NodeId(m.target_node),
                    param_name: m.target_param.clone(),
                    min: m.min as f32,
                    max: m.max as f32,
                },
                transform,
                name: format!("{:?} -> {}", m.event_pattern, m.target_param),
                enabled: m.enabled,
            });
        }

        Ok(())
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
    fn test_cbor_roundtrip() {
        let doc = sample_doc();
        let cbor = to_cbor(&doc).unwrap();
        let restored = from_cbor(&cbor).unwrap();
        assert_eq!(restored.automata.len(), 1);
        assert_eq!(restored.automata[0].id(), "lfo1");
    }

    #[test]
    fn test_apply_to_adds_servo() {
        let doc = sample_doc();
        let _mailbox = Arc::new(MpscQueue::with_capacity(64));
        let actor_ref = ActorRef::new(&_mailbox);
        let mut control = Patchbay::new(actor_ref);
        let registry = FunctionRegistry::builtin();
        doc.apply_to(&mut control, &registry).unwrap();
        control.update(0.01);
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
        let _mailbox = Arc::new(MpscQueue::with_capacity(64));
        let actor_ref = ActorRef::new(&_mailbox);
        let mut control = Patchbay::new(actor_ref);
        let registry = FunctionRegistry::builtin();
        assert!(doc.apply_to(&mut control, &registry).is_err());
    }

    #[test]
    fn test_apply_to_async_roundtrip() {
        let doc = PatchbayDef {
            automata: vec![AutomatonDef::Lfo {
                id: "lfo1".into(),
                frequency: 1.0,
                amplitude: 1.0,
                offset: 0.0,
                waveform: LfoWaveform::Sine,
            }],
            modules: vec![ModuleDef::Servo(ServoDef {
                automaton_id: "lfo1".into(),
                target_node: 1,
                target_param: "cutoff".into(),
                mapping: MappingType::Linear,
                min: 100.0,
                max: 1000.0,
                enabled: true,
                async_interval_ms: Some(10.0),
                control_strategy: Some(ControlStrategy::Absolute),
                conflict_strategy: Some(ConflictStrategy::LastWriteWins),
                table: None,
            })],
            mappings: vec![],
            osc_surface: vec![],
            description: None,
        };

        let json = to_json(&doc).unwrap();
        let restored = from_json(&json).unwrap();
        assert_eq!(restored.modules.len(), 1);
        match &restored.modules[0] {
            ModuleDef::Servo(s) => {
                assert_eq!(s.async_interval_ms, Some(10.0));
                assert_eq!(s.control_strategy, Some(ControlStrategy::Absolute));
                assert_eq!(s.conflict_strategy, Some(ConflictStrategy::LastWriteWins));
            }
            _ => panic!("expected Servo"),
        }
    }

    #[tokio::test]
    async fn test_apply_to_async_spawns_tasks() {
        use rill_core::queues::MpscQueue;

        let doc = PatchbayDef {
            automata: vec![AutomatonDef::Lfo {
                id: "lfo1".into(),
                frequency: 10.0,
                amplitude: 1.0,
                offset: 0.0,
                waveform: LfoWaveform::Sine,
            }],
            modules: vec![ModuleDef::Servo(ServoDef {
                automaton_id: "lfo1".into(),
                target_node: 1,
                target_param: "cutoff".into(),
                mapping: MappingType::Linear,
                min: 100.0,
                max: 1000.0,
                enabled: true,
                async_interval_ms: Some(10.0),
                control_strategy: Some(ControlStrategy::Absolute),
                conflict_strategy: Some(ConflictStrategy::LastWriteWins),
                table: None,
            })],
            mappings: vec![],
            osc_surface: vec![],
            description: None,
        };

        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let actor_ref = ActorRef::new(&mailbox);
        let mut control = Patchbay::new(actor_ref);
        let registry = FunctionRegistry::builtin();
        doc.apply_to_async(&mut control, &registry).unwrap();

        // Let the green thread produce a value
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(!mailbox.is_empty(), "async LFO should have pushed a value");
    }
}
