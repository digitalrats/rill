//! # Automaton control strategies
//!
//! Defines how an automaton affects a node parameter and how
//! conflicts between automatic and manual control are resolved.

/// How the automaton affects the target parameter
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlStrategy {
    /// The automaton sets the parameter value directly.
    ///
    /// The automaton output is expected in the [0, 1] range and is mapped
    /// to [min, max] of the target parameter.
    Absolute,

    /// The automaton modulates around the base value.
    ///
    /// The automaton output is expected in the [-1, 1] range.
    /// Final value: `base + mod_val * depth * (max - min)`,
    /// clamped to [min, max].
    Modulation {
        /// Modulation depth (0.0 — no modulation, 1.0 — full range)
        depth: f64,
    },
}

/// Conflict resolution strategy between UI and automaton
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConflictStrategy {
    /// UI touch freezes the automaton for this port.
    ///
    /// On `SetBase`, the automaton stops affecting the parameter.
    /// On `Release`, the automaton resumes control.
    TouchOverride,

    /// UI sets the base value, the automaton modulates around it.
    ///
    /// The final value is computed using the control strategy formula
    /// combining the UI base value and automaton modulation.
    BasePlusModulation,

    /// The last writer to the queue wins.
    ///
    /// UI and automaton write to the queue independently. The order of
    /// application is determined by the message order in the MpscQueue.
    LastWriteWins,
}

/// UI command (sent via actor mailbox, can be forwarded to an automaton)
#[derive(Debug, Clone)]
pub enum UiCommand {
    /// Set the base value
    SetValue(f64),

    /// Release control (TouchOverride only)
    Release,
}
