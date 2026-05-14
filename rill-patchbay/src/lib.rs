//! # Rill Patchbay — Event routing and automation
//!
//! `rill-patchbay` is the evolution of `rill-automation` from version 0.2.0,
//! merged with the mapping functionality from `rill-control`.
//!
//! ## Core components
//!
//! - **Automata** — generative signal sources (LFO, envelopes, sequencers)
//! - **Servos** (in the `control` module) — connect automata to node parameters
//! - **Mappings** — connect external events (MIDI/OSC) to parameters
//! - **Sensors** — event sources from the external world
//! - **Manager** — central coordinator for dual-thread architecture
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     CONTROL THREAD                         │
//! │                                                              │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │               Manager                         │   │
//! │  │  ┌────────────┐  ┌────────────┐  ┌────────────┐     │   │
//! │  │  │  Automata  │  │  Servos    │  │  Mappings  │     │   │
//! │  │  └────────────┘  └────────────┘  └────────────┘     │   │
//! │  │                    │                │                │   │
//! │  │                    ▼                ▼                │   │
//! │  │              ┌──────────────────────────┐           │   │
//! │  │              │   RtQueue<ParameterCommand>│         │   │
//! │  │              └──────────────────────────┘           │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! │                              │                               │
//! │                              │ non-blocking queue              │
//! │                              ▼                               │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │                  AUDIO THREAD                          │   │
//! │  │              (rill-graph / rill-io)                  │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

#![warn(missing_docs)]
#![deny(unsafe_code)]
#![allow(clippy::too_many_arguments)]

// =============================================================================
// External dependencies
// =============================================================================

// Re-exports from rill-core
pub use rill_core::prelude::*;
pub use rill_core::queues::RtQueue;
pub use rill_core::{NodeId, ParamValue, ParameterId, PortId};

// =============================================================================
// Public modules
// =============================================================================

/// Automata — generative control sources
pub mod automaton;

/// Control and event mapping
pub mod engine;

/// Sensors — event sources from the external world
pub mod sensor;

/// Utilities and helper functions
pub mod utils;

/// Named function registry for serialization
pub mod function_registry;

/// Automaton control strategies
pub mod strategy;

/// Custom module factory — type registry for rack module construction
pub mod module_factory;

/// Automaton wrapper in a green thread (tokio task)
pub mod automaton_task;

/// Serialization — documents, DOT, formats
#[cfg(feature = "serde")]
pub mod serialization;

#[cfg(feature = "serde")]
pub use serialization::RackDef;

/// MIDI hub — raw MIDI → ControlEvent bridge
#[cfg(feature = "midi")]
pub mod midi;

/// Micro-control observer for RT safety monitoring
pub mod observer;

#[cfg(feature = "midi")]
pub use midi::MidiHub;
pub use sensor::Sensor;

// =============================================================================
// Re-exports for convenience
// =============================================================================

// Selective re-exports
pub use automaton::sequencer::{PlayMode, SequencerAutomaton, Step};
pub use automaton::{
    EnvelopeAutomaton, EnvelopeStage, EnvelopeType, FunctionAutomaton, LfoAutomaton, LfoWaveform,
    Range, StatefulFunctionAutomaton, SyncMode,
};
pub use automaton_task::spawn_automaton_task;
pub use engine::{
    midi_cc, osc_address, Automaton, BoxedModule, ControlEvent, EventPattern, Mapping, Module,
    NoAction, OscSurface, OscSurfaceEntry, ParameterMapping, Servo, Target, Transform,
};

pub use strategy::{ConflictStrategy, ControlStrategy};

// =============================================================================
// Prelude for convenient imports
// =============================================================================

/// Prelude for convenient import of core types
pub mod prelude {
    // Core types
    pub use crate::automaton::*;
    pub use crate::automaton_task::*;
    pub use crate::engine::*;
    pub use crate::strategy::*;
    pub use crate::utils::*;

    // Re-exports from rill-core
    pub use rill_core::prelude::*;
    pub use rill_core::queues::RtQueue;
    pub use rill_core::{NodeId, ParameterId, PortId};
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_imports() {
        // Just check that everything imports
        let _ = automaton::LfoWaveform::Sine;
        let _ = engine::Transform::Linear;
    }
}
