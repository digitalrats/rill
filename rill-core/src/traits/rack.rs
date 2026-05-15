//! # Eurorack — modular processing case archetype
//!
//! An `Eurorack` is a single processing unit (case) in a modular
//! signal processing system — equivalent to one row or enclosure in a Eurorack
//! modular synthesizer.
//!
//! Each case holds:
//! - **Signal processing modules** — signal generation, transformation, routing, consumption
//! - **Control modules** — automata (LFO, envelope), sensors (MIDI, OSC)
//! - **Signal routing** — port-to-port connections between modules
//!
//! ## Domain
//!
//! Eurorack is a modular synthesizer format standardised by Doepfer
//! in 1996.  Modules are mounted in cases (rows) that provide power
//! and a backplane for patching.  Multiple cases are patched together
//! via a [`ModularSystem`](super::super::ModularSystem) to form
//! a complete instrument.
//!
//! ## Hierarchy
//!
//! ```text
//! ModularSystem (ActorSystem)
//! └── RackCase: Eurorack (case 1)
//!     ├── Graph (nodes: Source, Processor, Router, Sink)
//!     └── Patchbay (modules: Servo, Sensor)
//! ```

/// A single Eurorack processing case — holds signal and control modules,
/// provides the backplane (sample rate, clock, command routing).
///
/// This is the archetype for any Eurorack-compatible processing unit.
/// Concrete implementations ([`RackCase`]) are managed by a
/// [`ModularSystem`] which acts as an actor system for inter-case
/// communication.
///
/// # Example
///
/// ```no_run
/// use rill_core::traits::Eurorack;
///
/// struct MyCase {
///     name: String,
///     sample_rate: f32,
/// }
///
/// impl Eurorack for MyCase {
///     fn name(&self) -> &str { &self.name }
///     fn sample_rate(&self) -> f32 { self.sample_rate }
///     fn block_size(&self) -> usize { 256 }
/// }
/// ```
pub trait Eurorack {
    /// Case identifier (unique within a [`ModularSystem`]).
    fn name(&self) -> &str;

    /// Sample rate in Hz.
    fn sample_rate(&self) -> f32;

    /// Processing block size in samples.
    fn block_size(&self) -> usize;
}
