use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rill_core::NodeId;

use crate::automaton::lfo::LfoWaveform;
use crate::automaton::envelope::{EnvelopeAutomaton, EnvelopeType};
use crate::automaton::sequencer::{PlayMode, SequencerAutomaton, Step};
use crate::control::{BoxedServo, Mapping, OscSurface, PatchbayControl, Servo, ParameterMapping, Target, Transform};
pub use crate::control::EventPattern;
use crate::function_registry::FunctionRegistry;
use crate::strategy::{ConflictStrategy, ControlStrategy};

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
}

impl AutomatonDef {
    pub fn id(&self) -> &str {
        match self {
            AutomatonDef::Lfo { id, .. } => id,
            AutomatonDef::Envelope { id, .. } => id,
            AutomatonDef::Sequencer { id, .. } => id,
            AutomatonDef::NamedFunction { id, .. } => id,
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
    /// (requires manual `PatchbayControl::update()` calls).
    #[serde(default)]
    pub async_interval_ms: Option<f64>,

    /// Async mode: control strategy (defaults to `Absolute`).
    #[serde(default)]
    pub control_strategy: Option<ControlStrategy>,

    /// Async mode: conflict resolution (defaults to `LastWriteWins`).
    #[serde(default)]
    pub conflict_strategy: Option<ConflictStrategy>,
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
// PatchbayDocument
// ============================================================================

/// Serializable patchbay configuration.
///
/// Analogous to `rill_graph::serialization::GraphDocument`, linked through
/// shared `node_id` values.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct PatchbayDocument {
    pub automata: Vec<AutomatonDef>,
    pub servos: Vec<ServoDef>,
    pub mappings: Vec<MappingDef>,

    /// OSC → EventPattern bridge (see [`OscSurfaceEntry`]).
    /// Consumed by the host runtime to register user‑facing OSC handlers.
    #[serde(default)]
    pub osc_surface: OscSurface,

    /// Optional human-readable description (attribution, preset notes, …).
    /// Not interpreted by the engine; preserved through serialisation round-trips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl PatchbayDocument {
    pub fn new() -> Self {
        Self {
            automata: Vec::new(),
            servos: Vec::new(),
            mappings: Vec::new(),
            osc_surface: Vec::new(),
            description: None,
        }
    }

    /// Apply the document to a [`PatchbayControl`].
    pub fn apply_to(
        &self,
        control: &mut PatchbayControl,
        registry: &FunctionRegistry,
    ) -> Result<(), String> {
        let auto_ids: std::collections::HashSet<&str> =
            self.automata.iter().map(|a| a.id()).collect();

        for s in &self.servos {
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
                AutomatonDef::Lfo { id, frequency, amplitude, offset, waveform } => {
                    control.add_lfo(
                        id, *frequency, *amplitude, *offset, *waveform,
                        nid, &s.target_param, s.min, s.max,
                    );
                }
                AutomatonDef::Envelope { id, envelope_type, attack, decay, sustain, release, curve } => {
                    let automaton = EnvelopeAutomaton::adsr(id, *attack, *decay, *sustain, *release)
                        .with_curve(*curve);
                    let servo: BoxedServo = Box::new(
                        Servo::new(id, automaton, nid, &s.target_param, mapping, s.min, s.max),
                    );
                    control.add_boxed_servo(id.clone(), servo);
                }
                AutomatonDef::Sequencer { id, steps, play_mode, tempo } => {
                    let seq_steps: Vec<Step> = steps
                        .iter()
                        .map(|sd| Step { value: sd.value, duration: sd.duration, curve: sd.curve })
                        .collect();
                    let automaton = SequencerAutomaton::new(id, seq_steps)
                        .with_mode(*play_mode)
                        .with_tempo(*tempo);
                    let servo: BoxedServo = Box::new(
                        Servo::new(id, automaton, nid, &s.target_param, mapping, s.min, s.max),
                    );
                    control.add_boxed_servo(id.clone(), servo);
                }
                AutomatonDef::NamedFunction { id, .. } => {
                    log::warn!("NamedFunction automaton '{}' requires manual setup", id);
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

    /// Apply the document to a [`PatchbayControl`] using async automaton tasks.
    ///
    /// For each servo with `async_interval_ms: Some(...)`, creates a green
    /// thread (tokio task) with the specified strategies. Falls back to sync
    /// mode for servos without async configuration.
    ///
    /// Requires an active tokio runtime.
    pub fn apply_to_async(
        &self,
        control: &mut PatchbayControl,
        registry: &FunctionRegistry,
    ) -> Result<(), String> {
        let auto_ids: std::collections::HashSet<&str> =
            self.automata.iter().map(|a| a.id()).collect();

        for s in &self.servos {
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
            let target = (nid, s.target_param.clone());
            let range = (s.min, s.max);

            match def {
                AutomatonDef::Lfo { id, frequency, amplitude, offset, waveform } => {
                    if let Some(interval_ms) = s.async_interval_ms {
                        let interval = Duration::from_secs_f64(interval_ms / 1000.0);
                        let control_strategy = s.control_strategy.unwrap_or(ControlStrategy::Absolute);
                        let conflict_strategy = s.conflict_strategy.unwrap_or(ConflictStrategy::LastWriteWins);
                        control.add_lfo_task(
                            id, *frequency, *amplitude, *offset, *waveform,
                            interval, target, range,
                            control_strategy, conflict_strategy,
                        );
                    } else {
                        control.add_lfo(
                            id, *frequency, *amplitude, *offset, *waveform,
                            nid, &s.target_param, s.min, s.max,
                        );
                    }
                }
                AutomatonDef::Envelope { id, attack, decay, sustain, release, curve, .. } => {
                    if let Some(interval_ms) = s.async_interval_ms {
                        let interval = Duration::from_secs_f64(interval_ms / 1000.0);
                        let control_strategy = s.control_strategy.unwrap_or(ControlStrategy::Absolute);
                        let conflict_strategy = s.conflict_strategy.unwrap_or(ConflictStrategy::LastWriteWins);
                        control.add_envelope_task(
                            id, *attack, *decay, *sustain, *release,
                            interval, target, range,
                            control_strategy, conflict_strategy,
                        );
                    } else {
                        let automaton = EnvelopeAutomaton::adsr(id, *attack, *decay, *sustain, *release)
                            .with_curve(*curve);
                        let mapping = s.mapping.to_parameter_mapping();
                        let servo: BoxedServo = Box::new(
                            Servo::new(id, automaton, nid, &s.target_param, mapping, s.min, s.max),
                        );
                        control.add_boxed_servo(id.clone(), servo);
                    }
                }
                AutomatonDef::Sequencer { id, steps, play_mode, tempo } => {
                    log::warn!("Sequencer sync mode not fully wired in apply_to_async; use manual setup");
                    let _ = (id, steps, play_mode, tempo, nid, s);
                }
                AutomatonDef::NamedFunction { id, .. } => {
                    log::warn!("NamedFunction automaton '{}' requires manual setup", id);
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
}

impl Default for PatchbayDocument {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Serialisation helpers
// ============================================================================

#[cfg(feature = "json")]
pub fn to_json(doc: &PatchbayDocument) -> Result<String, String> {
    serde_json::to_string_pretty(doc).map_err(|e| e.to_string())
}

#[cfg(feature = "json")]
pub fn from_json(json: &str) -> Result<PatchbayDocument, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

#[cfg(feature = "cbor")]
pub fn to_cbor(doc: &PatchbayDocument) -> Result<Vec<u8>, String> {
    serde_cbor::to_vec(doc).map_err(|e| e.to_string())
}

#[cfg(feature = "cbor")]
pub fn from_cbor(bytes: &[u8]) -> Result<PatchbayDocument, String> {
    serde_cbor::from_slice(bytes).map_err(|e| e.to_string())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::queues::MpscQueue;

    fn sample_doc() -> PatchbayDocument {
        PatchbayDocument {
            automata: vec![
                AutomatonDef::Lfo {
                    id: "lfo1".into(),
                    frequency: 0.3,
                    amplitude: 1.0,
                    offset: 0.0,
                    waveform: LfoWaveform::Sine,
                },
            ],
            servos: vec![
                ServoDef {
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
                },
            ],
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
        assert_eq!(restored.servos.len(), 1);
        assert_eq!(restored.servos[0].target_param, "delay_time");
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
        let q = Arc::new(MpscQueue::new());
        let mut control = PatchbayControl::new(q);
        let registry = FunctionRegistry::builtin();
        doc.apply_to(&mut control, &registry).unwrap();
        control.update(0.01);
    }

    #[test]
    fn test_missing_automaton_error() {
        let doc = PatchbayDocument {
            automata: vec![],
            servos: vec![ServoDef {
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
            }],
            mappings: vec![],
            osc_surface: vec![],
            description: None,
        };
        let q = Arc::new(MpscQueue::new());
        let mut control = PatchbayControl::new(q);
        let registry = FunctionRegistry::builtin();
        assert!(doc.apply_to(&mut control, &registry).is_err());
    }

    #[test]
    fn test_apply_to_async_roundtrip() {
        let doc = PatchbayDocument {
            automata: vec![
                AutomatonDef::Lfo {
                    id: "lfo1".into(),
                    frequency: 1.0,
                    amplitude: 1.0,
                    offset: 0.0,
                    waveform: LfoWaveform::Sine,
                },
            ],
            servos: vec![ServoDef {
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
            }],
            mappings: vec![],
            osc_surface: vec![],
            description: None,
        };

        let json = to_json(&doc).unwrap();
        let restored = from_json(&json).unwrap();
        assert_eq!(restored.servos.len(), 1);
        assert_eq!(restored.servos[0].async_interval_ms, Some(10.0));
        assert_eq!(restored.servos[0].control_strategy, Some(ControlStrategy::Absolute));
        assert_eq!(restored.servos[0].conflict_strategy, Some(ConflictStrategy::LastWriteWins));
    }

    #[tokio::test]
    async fn test_apply_to_async_spawns_tasks() {
        use crate::control::ParameterCommand;
        use rill_core::queues::MpscQueue;

        let doc = PatchbayDocument {
            automata: vec![
                AutomatonDef::Lfo {
                    id: "lfo1".into(),
                    frequency: 10.0,
                    amplitude: 1.0,
                    offset: 0.0,
                    waveform: LfoWaveform::Sine,
                },
            ],
            servos: vec![ServoDef {
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
            }],
            mappings: vec![],
            osc_surface: vec![],
            description: None,
        };

        let q: Arc<MpscQueue<ParameterCommand>> = Arc::new(MpscQueue::new());
        let mut control = PatchbayControl::new(q.clone());
        let registry = FunctionRegistry::builtin();
        doc.apply_to_async(&mut control, &registry).unwrap();

        // Let the green thread produce a value
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(!q.is_empty(), "async LFO should have pushed a value");
    }
}
