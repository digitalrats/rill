//! # Sensor — external input bridge
//!
//! A `Sensor` converts external data (MIDI, OSC, hardware knobs, audio analysis)
//! into [`ControlEvent`]s and sends them to the Patchbay via [`ActorRef`].
//!
//! ## Submodules
//!
//! - [`hearing`] — audio analysis algorithms (pitch, envelope, zero-crossing)
//!   for acoustic sensors that react to graph audio output.
//!
//! Multiple sensors can run independently — all events share a single
//! mailbox drained by [`Patchbay::drain_events`].

pub mod hearing;

use rill_core_actor::ActorRef;

use crate::engine::{ControlEvent, Module};

/// External input that produces [`ControlEvent`]s and dispatches them
/// via a shared [`ActorRef`].
///
/// `Sensor` extends [`Module`] so every sensor is also a rack module.
pub trait Sensor: Module {
    /// Attach the sensor to an event sink.
    ///
    /// Called before [`start`](Self::start). The sensor stores the
    /// [`ActorRef`] and uses it to send events during its lifetime.
    fn attach(&mut self, events: ActorRef<ControlEvent>);

    /// Start the sensor (begin polling, open device, spawn thread, etc.).
    fn start(&mut self);
}
