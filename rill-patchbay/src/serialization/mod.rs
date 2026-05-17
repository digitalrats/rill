#![allow(missing_docs)]
//! Serializable rack document types (de)serialised from JSON/CBOR.

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::traits::ParamValue;
use rill_core::NodeId;

use crate::automaton::envelope::EnvelopeAutomaton;
use crate::automaton::lfo::{LfoAutomaton, LfoWaveform};
use crate::automaton::sequencer::{PlayMode, SequencerAutomaton, Step};
pub use crate::engine::EventPattern;
use crate::engine::{OscSurface, ParameterMapping, Servo, Transform};
use crate::function_registry::FunctionRegistry;
use crate::module_factory::ModuleFactory;
use crate::strategy::{ConflictStrategy, ControlStrategy};
use rill_core::queues::CommandEnum;
use rill_core_actor::{ActorRef, ActorSystem};

// Re-export all module definition types from the always-compiled module.
pub use crate::module_def::{
    AutomatonDef, MappingDef, MappingType, ModuleDef, SensorDef, ServoDef, StepDef, TransformDef,
};

pub mod dot;

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
        _registry: &FunctionRegistry,
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
                ModuleDef::Custom {
                    type_name,
                    params: _,
                } => {
                    // TODO: Custom module construction through updated factory API
                    log::warn!("Custom module '{}' — re-register needed", type_name);
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
