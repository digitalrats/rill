//! # Functional automata
//!
//! Automata built on arbitrary time functions.
//! Allow implementing any mathematical relationship.

use crate::engine::{Automaton, NoAction, Range, Time};
use rill_core::traits::ParamValue;
use std::fmt;
use std::sync::Arc;

/// Functional automaton (stateless)
#[derive(Clone)]
pub struct FunctionAutomaton {
    /// Automaton name
    name: String,
    /// Generator function
    generator: Arc<dyn Fn(Time) -> f64 + Send + Sync>,
    /// Output value range
    range: Range,
}

impl fmt::Debug for FunctionAutomaton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FunctionAutomaton")
            .field("name", &self.name)
            .field("range", &self.range)
            .finish()
    }
}

impl FunctionAutomaton {
    /// Create a new functional automaton
    pub fn new<F>(name: &str, generator: F) -> Self
    where
        F: Fn(Time) -> f64 + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            generator: Arc::new(generator),
            range: Range::bipolar(),
        }
    }

    /// Set the range
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }
}

impl Automaton for FunctionAutomaton {
    type Internal = ();
    type Action = NoAction;

    fn step(
        &self,
        _internal: &mut Self::Internal,
        _current: &ParamValue,
        time: Time,
        _action: &Self::Action,
    ) -> ParamValue {
        let value = (self.generator)(time);
        let clamped = self.range.clamp(value);
        ParamValue::Float(clamped as f32)
    }

    fn initial_internal(&self) -> Self::Internal {}

    fn name(&self) -> &str {
        &self.name
    }
}

/// Functional automaton with state
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct StatefulFunctionAutomaton<S> {
    /// Automaton name
    name: String,
    /// Generator function with state
    generator: Arc<dyn Fn(Time, &mut S) -> f64 + Send + Sync>,
    /// Initial state
    initial_state: S,
    /// Output value range
    range: Range,
}

impl<S: fmt::Debug + Send + Sync + Clone + 'static> fmt::Debug for StatefulFunctionAutomaton<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatefulFunctionAutomaton")
            .field("name", &self.name)
            .field("initial_state", &self.initial_state)
            .field("range", &self.range)
            .finish()
    }
}

impl<S: Send + Sync + Clone + 'static> StatefulFunctionAutomaton<S> {
    /// Create a new stateful automaton
    pub fn new<F>(name: &str, generator: F, initial_state: S) -> Self
    where
        F: Fn(Time, &mut S) -> f64 + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            generator: Arc::new(generator),
            initial_state,
            range: Range::bipolar(),
        }
    }

    /// Set the range
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }
}

impl<S: fmt::Debug + Send + Sync + Clone + 'static> Automaton for StatefulFunctionAutomaton<S> {
    type Internal = S;
    type Action = NoAction;

    fn step(
        &self,
        internal: &mut Self::Internal,
        _current: &ParamValue,
        time: Time,
        _action: &Self::Action,
    ) -> ParamValue {
        let value = (self.generator)(time, internal);
        let clamped = self.range.clamp(value);
        ParamValue::Float(clamped as f32)
    }

    fn initial_internal(&self) -> Self::Internal {
        self.initial_state.clone()
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Generator function for LFO (convenience wrapper)
pub fn lfo_function(freq: f64, phase: f64, waveform: &'static str) -> impl Fn(Time) -> f64 {
    move |t| {
        let p = (t * freq + phase).fract();
        match waveform {
            "sine" => (p * 2.0 * std::f64::consts::PI).sin(),
            "saw" => 2.0 * p - 1.0,
            "square" => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_automaton() {
        let automaton = FunctionAutomaton::new("Test", |t| (t * 2.0).sin());
        automaton.initial_internal();
        let current = ParamValue::Float(0.0);

        let value = automaton.step(&mut (), &current, 1.0, &NoAction);
        assert!(value.as_f32().is_some());
    }

    #[test]
    fn test_stateful_automaton() {
        let automaton = StatefulFunctionAutomaton::new(
            "Counter",
            |_t, counter: &mut i32| {
                *counter += 1;
                *counter as f64
            },
            0,
        )
        .with_range(Range::new(0.0, 100.0));

        let mut internal = automaton.initial_internal();
        let current = ParamValue::Float(0.0);

        let value = automaton.step(&mut internal, &current, 1.0, &NoAction);
        assert_eq!(value.as_f32().unwrap(), 1.0);

        let value = automaton.step(&mut internal, &current, 1.0, &NoAction);
        assert_eq!(value.as_f32().unwrap(), 2.0);
    }
}
