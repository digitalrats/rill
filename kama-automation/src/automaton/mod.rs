// kama-automation/src/automaton/mod.rs

mod function;
mod lfo;

pub use function::{FunctionAutomaton, StatefulFunctionAutomaton, FunctionState, GeneratorFn};
pub use lfo::{LfoAutomaton, LfoWithEnvelopeAutomaton};  // Теперь это должно работать

// Реэкспортируем из kama-oscillators для удобства
pub use kama_oscillators::control::LfoWaveform as Waveform;

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

/// Вспомогательный трейт для создания автоматов из замыканий
pub trait IntoAutomaton {
    type Automaton: Automaton + 'static;
    
    fn into_automaton(self, target_node: &str, target_param: &str) -> Self::Automaton;
}

impl<F> IntoAutomaton for F
where
    F: Fn(f64) -> f64 + Send + Sync + 'static,
{
    type Automaton = FunctionAutomaton;
    
    fn into_automaton(self, target_node: &str, target_param: &str) -> Self::Automaton {
        FunctionAutomaton::new("Custom", self, target_node, target_param)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_into_automaton() {
        let automaton = (|t: f64| t.sin()).into_automaton("test", "param");
        assert_eq!(automaton.name(), "Custom");
    }
}