//! # RackCase — Eurorack case implementation
//!
//! A `RackCase` is a single Eurorack processing case — one row of
//! modules (audio nodes + control modules) with a shared backplane.
//! Each case holds its own signal graph and patchbay.
//!
//! Cases are created through [`ModularSystem::create_case`] and
//! registered as actors in the system's actor network.
//!
//! ## Architecture
//!
//! ```text
//! RackCase
//! ├── Graph (nodes: Source, Processor, Router, Sink)
//! │   ├── Node factory (shared across cases)
//! │   └── Backend factory (shared across cases)
//! └── Patchbay (modules: Servo, Sensor)
//!     └── Command queue (MpscQueue<SetParameter>)
//! ```

use std::sync::Arc;

use rill_core::queues::{CommandEnum, MpscQueue};
use rill_core::traits::Eurorack;
use rill_core_actor::ActorRef;
use rill_patchbay::engine::Patchbay;

/// A single Eurorack processing case.
///
/// Holds an audio signal graph and a control patchbay.
/// Created and managed by [`ModularSystem`] which registers each
/// case as an actor in the system's [`ActorSystem`].
///
/// ## Communication model
///
/// * **Internal** (Graph ↔ Patchbay within the same case):
///   direct [`MpscQueue<SetParameter>`] for RT-safe parameter changes.
/// * **External** (cross‑case / system‑level):
///   through the case's mailbox ([`CommandEnum`] messages).
///   Incoming commands are drained via [`receive`](Self::receive).
///   Outgoing commands are pushed via [`send`](Self::send) and
///   collected by [`ModularSystem::tick`] for routing.
pub struct RackCase<const BUF: usize> {
    /// Case identifier (unique within the system).
    name: String,

    /// Audio sample rate in Hz.
    sample_rate: f32,

    /// Actor mailbox for **incoming** commands from other cases or external actors.
    mailbox: Arc<MpscQueue<CommandEnum>>,

    /// Outgoing command queue — commands destined for other cases.
    /// Collected by [`ModularSystem::tick`] and routed via [`ActorSystem`](rill_core_actor::ActorSystem).
    outgoing: Vec<CommandEnum>,

    /// Control patchbay (automata, sensors, mappings).
    patchbay: Option<Patchbay>,
}

impl<const BUF: usize> RackCase<BUF> {
    /// Create a new case.
    ///
    /// Called by [`ModularSystem::create_case`] — not intended for
    /// direct construction.
    pub(crate) fn new(
        name: String,
        sample_rate: f32,
        mailbox: Arc<MpscQueue<CommandEnum>>,
    ) -> Self {
        Self {
            name,
            sample_rate,
            mailbox,
            outgoing: Vec::new(),
            patchbay: None,
        }
    }

    /// Return the actor handle for sending commands TO this case.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        ActorRef::new(&self.mailbox)
    }

    /// Send a command FROM this case to another actor in the system.
    ///
    /// The command is buffered in the outgoing queue.
    /// [`ModularSystem::tick`] drains outgoing commands from all cases
    /// and routes them via the actor system.
    pub fn send(&mut self, cmd: CommandEnum) {
        self.outgoing.push(cmd);
    }

    /// Take all outgoing commands, leaving the queue empty.
    ///
    /// Called by [`ModularSystem`] during the tick cycle.
    pub(crate) fn take_outgoing(&mut self) -> Vec<CommandEnum> {
        std::mem::take(&mut self.outgoing)
    }

    /// Drain the incoming mailbox and return all pending commands.
    ///
    /// Called by [`ModularSystem::tick`] at the start of each frame.
    /// The caller is responsible for dispatching commands to the
    /// appropriate subsystem (Graph, Patchbay).
    pub fn receive(&self) -> Vec<CommandEnum> {
        let mut msgs = Vec::new();
        while let Some(cmd) = self.mailbox.pop() {
            msgs.push(cmd);
        }
        msgs
    }

    /// Access the case's patchbay (read-only).
    pub fn patchbay(&self) -> Option<&Patchbay> {
        self.patchbay.as_ref()
    }
}

impl<const BUF: usize> Eurorack for RackCase<BUF> {
    fn name(&self) -> &str {
        &self.name
    }

    fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    fn block_size(&self) -> usize {
        BUF
    }
}
