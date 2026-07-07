//! Automatons — generative control-signal sources.
//!
//! This module provides various automaton types for generating real-time
//! control signals. An automaton is an algorithm with internal state:
//! given time and the current state, it produces a new state and an
//! optional output value. External state mutation is not required.

pub mod cellular;
pub mod envelope;
pub mod factory;
pub mod function;
pub mod lfo;
pub mod random;
pub mod sequencer;

pub use cellular::*;
pub use envelope::*;
pub use factory::*;
pub use function::*;
pub use lfo::*;
pub use random::*;

use std::fmt::Debug;

/// Synchronisation mode for an automaton.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncMode {
    /// Free-running at the automaton's own rate.
    Free,
    /// Synchronised to an external clock.
    Sync,
    /// Run once and stop.
    OneShot,
}

/// A numeric range with min/max bounds.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy)]
pub struct Range {
    /// Lower bound of the range.
    pub min: f64,
    /// Upper bound of the range.
    pub max: f64,
}

impl Range {
    /// Create a new range with the given bounds.
    pub const fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }

    /// Return a unipolar range [0.0, 1.0].
    pub const fn unipolar() -> Self {
        Self { min: 0.0, max: 1.0 }
    }

    /// Return a bipolar range [-1.0, 1.0].
    pub const fn bipolar() -> Self {
        Self {
            min: -1.0,
            max: 1.0,
        }
    }

    /// Clamp a value to lie within this range.
    pub fn clamp(&self, value: f64) -> f64 {
        value.clamp(self.min, self.max)
    }

    /// Normalize a value to [0.0, 1.0] based on this range.
    pub fn normalize(&self, value: f64) -> f64 {
        (value - self.min) / (self.max - self.min)
    }

    /// Denormalize a [0.0, 1.0] value back into this range.
    pub fn denormalize(&self, norm: f64) -> f64 {
        self.min + norm * (self.max - self.min)
    }
}

/// Summary of available automaton types and their characteristics.
#[derive(Debug)]
pub struct AutomatonComparison;

impl AutomatonComparison {
    /// Print a table of automaton types and their applications.
    pub fn types() -> &'static str {
        "Automaton types:\n\
         ┌─────────────────┬─────────────────────────────┬─────────────────┐\n\
         │ Automaton       │ Characteristics              │ Application    │\n\
         ├─────────────────┼─────────────────────────────┼─────────────────┤\n\
         │ LFO             │ Harmonic/relaxation          │ Vibrato, tremolo│\n\
         │ Envelope        │ ADSR, AR, ASR               │ Amplitude env.  │\n\
         │ Function        │ Arbitrary time function      │ Complex mod.    │\n\
         │ Sequencer       │ Patterns, steps              │ Rhythmic        │\n\
         │ RandomWalk      │ Random walks                 │ Generative      │\n\
         │ Chaos           │ Deterministic chaos          │ Unpredictable   │\n\
         │ Cellular        │ Cellular automatons            │ Organic         │\n\
         └─────────────────┴─────────────────────────────┴─────────────────┘"
    }

    /// Guide for choosing the right automaton.
    pub fn selection_guide() -> &'static str {
        "How to choose an automaton:\n\n\
         Periodic modulation:\n\
         → LFO (Sine, Triangle, Saw, Square)\n\n\
         One-shot events:\n\
         → Envelope (ADSR, AR, ASR)\n\n\
         Complex functions:\n\
         → Function with arbitrary closure\n\n\
         Rhythmic patterns:\n\
         → Sequencer with steps and durations\n\n\
         Generative processes:\n\
         → RandomWalk, Chaos, Cellular\n\n\
         Random values:\n\
         → Sample & Hold (LFO in S&H mode)"
    }

    /// Relative performance characteristics of each automaton type.
    pub fn performance_guide() -> &'static str {
        "Relative performance:\n\
         **Function** — fastest (simple functions)\n\
         **LFO** — moderate (trigonometry)\n\
         **Envelope** — moderate (transition logic)\n\
         **RandomWalk** — moderate (RNG)\n\
         **Sequencer** — slower (patterns)\n\
         **Chaos** — slower (iterations)\n\
         **Cellular** — slowest (neighbour lookups)"
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_automaton_types_are_debug() {
        fn assert_debug<T: std::fmt::Debug>(_: &T) {}
        let lfo = super::LfoAutomaton::new("test", 1.0, 1.0, 0.0, super::LfoWaveform::Sine);
        assert_debug(&lfo);
        let env = super::EnvelopeAutomaton::adsr("test", 0.1, 0.2, 0.7, 0.3);
        assert_debug(&env);
        let func = super::FunctionAutomaton::new("test", |t| t);
        assert_debug(&func);
    }

    #[test]
    fn test_comparison_guides() {
        assert!(!super::AutomatonComparison::types().is_empty());
        assert!(!super::AutomatonComparison::selection_guide().is_empty());
        assert!(!super::AutomatonComparison::performance_guide().is_empty());
    }
}
