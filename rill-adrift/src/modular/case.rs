//! # RackCase — Eurorack case implementation
//!
//! A `RackCase` is a single Eurorack processing case — one row of
//! modules (signal nodes + control modules) with a shared backplane.
//! Each case holds its own signal graph and a map of control modules.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_core::queues::CommandEnum;
use rill_core::traits::Eurorack;
use rill_core_actor::ActorRef;

/// A single Eurorack processing case.
pub struct RackCase<const BUF: usize> {
    name: String,
    sample_rate: f32,
    actor_ref: ActorRef<CommandEnum>,
    tasks: Vec<std::thread::JoinHandle<()>>,

    /// Signal thread handle — the graph runs here (Graph is !Send).
    signal_thread: Option<std::thread::JoinHandle<()>>,

    /// Stop flag for the signal thread's run loop.
    running: Option<Arc<AtomicBool>>,
}

impl<const BUF: usize> RackCase<BUF> {
    /// Create a new case.
    pub(crate) fn new(
        name: String,
        sample_rate: f32,
        actor_ref: ActorRef<CommandEnum>,
        tasks: Vec<std::thread::JoinHandle<()>>,
    ) -> Self {
        Self {
            name,
            sample_rate,
            actor_ref,
            tasks,
            signal_thread: None,
            running: None,
        }
    }

    /// Return the actor handle for sending commands TO this case.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
    }

    /// Start the signal thread for this case.
    pub(crate) fn start<F>(&mut self, build: F)
    where
        F: FnOnce(Arc<AtomicBool>) + Send + 'static,
    {
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let handle = std::thread::spawn(move || build(r));
        self.running = Some(running);
        self.signal_thread = Some(handle);
    }

    /// Stop the signal thread and all module tasks.
    pub fn stop(&mut self) {
        for _task in self.tasks.drain(..) {
            // Drain threads are dropped — they'll be terminated at process exit
        }
        if let Some(ref running) = self.running {
            running.store(false, Ordering::Release);
        }
        if let Some(handle) = self.signal_thread.take() {
            handle.thread().unpark();
            let _ = handle.join();
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
