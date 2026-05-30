//! ServoConstructor — creates Servo actors from [`ModuleDef::Servo`] descriptors.

use std::sync::Arc;

use rill_core::queues::CommandEnum;
use rill_core::traits::NodeId;
use rill_core_actor::{ActorRef, ActorSystem};

use crate::automaton::envelope::EnvelopeAutomaton;
use crate::automaton::lfo::LfoAutomaton;
use crate::automaton::sequencer::{SequencerAutomaton, Step};
use crate::engine::Servo;
use crate::module_def::{AutomatonDef, ModuleDef};
use crate::module_factory::{ModuleConstructor, ModuleError};

/// Module constructor for the `"servo"` type — bridges an automaton to a graph parameter.
pub struct ServoConstructor;

impl ModuleConstructor for ServoConstructor {
    fn type_name(&self) -> &'static str {
        "servo"
    }

    fn construct(
        &self,
        module: &ModuleDef,
        automaton_defs: &[AutomatonDef],
        system: &Arc<ActorSystem>,
        graph_ref: &ActorRef<CommandEnum>,
    ) -> Result<ActorRef<CommandEnum>, ModuleError> {
        let ModuleDef::Servo(s) = module else {
            return Err(ModuleError::ConstructionFailed(
                "ServoConstructor requires ModuleDef::Servo".into(),
            ));
        };

        let def = automaton_defs
            .iter()
            .find(|a| a.id() == s.automaton_id)
            .ok_or_else(|| {
                ModuleError::ConstructionFailed(format!(
                    "servo '{}' references unknown automaton '{}'",
                    s.automaton_id, s.automaton_id
                ))
            })?;

        let nid = NodeId(s.target_node);
        let mapping = s.mapping.to_parameter_mapping();

        let actor_ref = match def {
            AutomatonDef::Lfo {
                id,
                frequency,
                amplitude,
                offset,
                waveform,
            } => {
                let a = LfoAutomaton::new(id, *frequency, *amplitude, *offset, *waveform);
                let mut servo = Servo::new(
                    id,
                    a,
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
                servo.spawn(system)
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
                let a = EnvelopeAutomaton::adsr(id, *attack, *decay, *sustain, *release)
                    .with_curve(*curve);
                let servo = Servo::new(
                    id,
                    a,
                    nid,
                    &s.target_param,
                    mapping,
                    s.min,
                    s.max,
                    system.clone(),
                    graph_ref.clone(),
                );
                servo.spawn(system)
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
                let a = SequencerAutomaton::new(id, seq_steps)
                    .with_mode(*play_mode)
                    .with_tempo(*tempo);
                let mut servo = Servo::new(
                    id,
                    a,
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
                servo.spawn(system)
            }
            AutomatonDef::NamedFunction { id, .. } => {
                return Err(ModuleError::ConstructionFailed(format!(
                    "NamedFunction automaton '{}' requires manual setup",
                    id
                )));
            }
            AutomatonDef::Custom { id, type_name, .. } => {
                return Err(ModuleError::ConstructionFailed(format!(
                    "Custom automaton '{}' (type '{}') not yet supported via ServoConstructor",
                    id, type_name,
                )));
            }
        };

        Ok(actor_ref)
    }

    fn clone_box(&self) -> Box<dyn ModuleConstructor> {
        Box::new(ServoConstructor)
    }
}
