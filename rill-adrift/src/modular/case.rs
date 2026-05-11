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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_core::queues::CommandEnum;
use rill_core::traits::Eurorack;
use rill_core_actor::{ActorCell, ActorRef, Mbox};
use rill_patchbay::engine::Patchbay;
#[cfg(feature = "serialization")]
use rill_patchbay::function_registry::FunctionRegistry;
#[cfg(feature = "serialization")]
use rill_patchbay::serialization::PatchbayDef;

/// A single Eurorack processing case.
pub struct RackCase<const BUF: usize> {
    name: String,
    sample_rate: f32,
    mailbox: Arc<Mbox<CommandEnum>>,
    outgoing: Vec<CommandEnum>,
    patchbay: Option<Patchbay>,

    /// Audio thread handle — the graph runs here (Graph is !Send).
    audio_thread: Option<std::thread::JoinHandle<()>>,

    /// Stop flag for the audio thread's run loop.
    running: Option<Arc<AtomicBool>>,
}

impl<const BUF: usize> RackCase<BUF> {
    /// Create a new case.
    ///
    /// Called by [`ModularSystem::create_case`] — not intended for
    /// direct construction.
    pub(crate) fn new(name: String, sample_rate: f32, mailbox: Arc<Mbox<CommandEnum>>) -> Self {
        Self {
            name,
            sample_rate,
            mailbox,
            outgoing: Vec::new(),
            patchbay: None,
            audio_thread: None,
            running: None,
        }
    }

    /// Return the actor handle for sending commands TO this case.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.mailbox.actor_ref()
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
    /// Note: distinct from [`ActorCell::receive`] — this drains all messages
    /// at once for routing through the actor system.
    pub fn drain(&mut self) -> Vec<CommandEnum> {
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

    /// Create the patchbay from a serialized definition.
    ///
    /// The patchbay uses `graph_ref` to send `SetParameter` commands to the
    /// audio graph. ClockTick is delivered through the parent RackCase mailbox.
    #[cfg(feature = "serialization")]
    pub(crate) fn create_patchbay(
        &mut self,
        def: &PatchbayDef,
        graph_ref: ActorRef<CommandEnum>,
    ) -> Result<(), String> {
        let mut patchbay = Patchbay::new(self.mailbox.clone(), graph_ref);
        let registry = FunctionRegistry::builtin();
        def.apply_to(&mut patchbay, &registry)?;
        self.patchbay = Some(patchbay);
        Ok(())
    }

    /// Start the audio thread for this case.
    ///
    /// Takes a closure that builds the graph on the target thread.
    pub(crate) fn start<F>(&mut self, build: F)
    where
        F: FnOnce(Arc<AtomicBool>) + Send + 'static,
    {
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let handle = std::thread::spawn(move || build(r));
        self.running = Some(running);
        self.audio_thread = Some(handle);
    }

    /// Stop the audio thread and the patchbay.
    pub fn stop(&mut self) {
        if let Some(ref mut patchbay) = self.patchbay {
            patchbay.stop_all();
        }
        if let Some(ref running) = self.running {
            running.store(false, Ordering::Release);
        }
        if let Some(handle) = self.audio_thread.take() {
            let _ = handle.join();
        }
    }
}

// ============================================================================
// ActorCell implementation
// ============================================================================

impl<const BUF: usize> ActorCell for RackCase<BUF> {
    type Msg = CommandEnum;

    fn receive(&mut self, msg: CommandEnum) {
        match msg {
            CommandEnum::ClockTick(tick) => {
                // Forward ClockTick to the patchbay's mailbox
                if let Some(ref patchbay) = self.patchbay {
                    let _ = patchbay.mailbox_ref().send(CommandEnum::ClockTick(tick));
                }
            }
            _ => {
                self.outgoing.push(msg);
            }
        }
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
