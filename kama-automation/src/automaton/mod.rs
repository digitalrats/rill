// kama-automation/src/automaton/mod.rs

mod lfo;
mod envelope;

pub use lfo::{LfoAutomaton, LfoAction, LfoState, LfoWithEnvelopeAutomaton, LfoWithEnvelopeState};
pub use envelope::{EnvelopeState, EnvelopeStage};

use crate::context::AutomationContext;

/// Алгебраические типы для автоматов
pub trait Automaton: Send + Sync {
    type Time;
    type Context;
    type Action: Clone + Default + Send + Sync + 'static;
    type State: Clone + Send + Sync + 'static;
    
    fn step(
        &self,
        time: Self::Time,
        context: &Self::Context,
        action: Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<Self::Action>);
    
    fn initial_state(&self) -> Self::State;
    fn name(&self) -> &str;
    fn extract_value(&self, state: &Self::State) -> f64;
}