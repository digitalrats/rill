//! Serialization for modular system documents.
//!
//! A [`ModularSystemDef`] describes a modular processing system — one or more
//! racks, each with a signal graph and control modules — in a single
//! self-contained JSON document.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use rill_core::queues::CommandEnum;
use rill_core::traits::{NodeId, ParamValue};
use rill_core_actor::{ActorRef, ActorSystem};
use rill_graph::serialization::GraphDef;
use rill_patchbay::automaton::envelope::EnvelopeAutomaton;
use rill_patchbay::automaton::lfo::LfoAutomaton;
use rill_patchbay::automaton::sequencer::{SequencerAutomaton, Step};
use rill_patchbay::engine::Servo;
use rill_patchbay::function_registry::FunctionRegistry;
use rill_patchbay::module_factory::ModuleFactory;
use rill_patchbay::serialization::{AutomatonDef, MappingDef, SensorDef, ServoDef};

// ============================================================================
// ModuleDef
// ============================================================================

/// A rack module — Servo (automaton → parameter), Sensor, Custom (factory),
/// or Graph (signal graph with its own I/O loop).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModuleDef {
    /// Documentation.
    Servo(ServoDef),
    /// Documentation.
    Sensor(SensorDef),
    /// Documentation.
    Custom {
        /// Documentation.
        type_name: String,
        /// Documentation.
        #[serde(default)]
        params: HashMap<String, ParamValue>,
    },
    /// Signal graph — owns the I/O loop.
    Graph {
        /// Documentation.
        graph: GraphDef,
    },
}

// ============================================================================
// RackDef
// ============================================================================

/// A modular processing rack — one signal graph + its control modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RackDef {
    /// Documentation.
    pub name: String,
    /// Documentation.
    pub graph: GraphDef,
    /// Documentation.
    #[serde(default)]
    pub automata: Vec<AutomatonDef>,
    /// Documentation.
    #[serde(default)]
    pub modules: Vec<ModuleDef>,
    /// Documentation.
    #[serde(default)]
    pub mappings: Vec<MappingDef>,
    /// Documentation.
    pub description: Option<String>,
}

impl RackDef {
    /// Documentation.
    pub fn new(name: impl Into<String>, graph: GraphDef) -> Self {
        Self {
            name: name.into(),
            graph,
            automata: Vec::new(),
            modules: Vec::new(),
            mappings: Vec::new(),
            description: None,
        }
    }

    /// Build servo and custom module actors from the rack definition.
    /// Graph modules are skipped — handled by the caller.
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
                            let actor_ref = Servo::new(
                                id,
                                automaton,
                                nid,
                                &s.target_param,
                                mapping,
                                s.min,
                                s.max,
                                system.clone(),
                                graph_ref.clone(),
                            )
                            .spawn(system);
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
                            let actor_ref = Servo::new(
                                id,
                                automaton,
                                nid,
                                &s.target_param,
                                mapping,
                                s.min,
                                s.max,
                                system.clone(),
                                graph_ref.clone(),
                            )
                            .spawn(system);
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
                            log::warn!("NamedFunction '{}' requires manual setup", id);
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
                ModuleDef::Sensor(_) => {
                    log::info!("Sensor module — needs manual setup");
                }
                ModuleDef::Custom { type_name, params } => {
                    match module_factory.construct(type_name, type_name, params, system, graph_ref)
                    {
                        Ok(module) => {
                            if let Some(h) = module.handle() {
                                modules.insert(type_name.clone(), h);
                            }
                        }
                        Err(e) => log::warn!("Custom module '{}' failed: {}", type_name, e),
                    }
                }
                ModuleDef::Graph { .. } => {}
            }
        }
        Ok(modules)
    }
}

// ============================================================================
// ModularSystemDef
// ============================================================================

/// Top-level document describing a full modular processing system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModularSystemDef {
    /// Documentation.
    pub format_version: String,
    /// Documentation.
    pub sample_rate: f32,
    /// Documentation.
    pub block_size: usize,
    /// Documentation.
    pub racks: Vec<RackDef>,
    /// Documentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
/// Documentation.

pub fn to_json(def: &ModularSystemDef) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(def)
}
/// Documentation.

pub fn from_json(json: &str) -> Result<ModularSystemDef, serde_json::Error> {
    serde_json::from_str(json)
}
