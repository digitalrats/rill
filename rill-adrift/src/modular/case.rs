//! # RackCase — Eurorack case implementation
//!
//! A `RackCase` is a single Eurorack processing case. It owns the thread on
//! which the rack's rill-lang program runs, and it records the error that
//! thread reports when the program fails at run time (a compile failure is
//! returned by [`ModularSystem::launch`](super::ModularSystem::launch)
//! instead).
//!
//! ## The control-module host is currently inert
//!
//! A case does **not** drive a control plane. Control modules (servos,
//! sensors, clocks, custom modules) are constructed and registered with the
//! rack actor, but no clock or parameter source is wired: they receive no
//! `ClockTick`, so nothing steps automatons, samples sensors, or writes
//! parameters. The module path is retained for the deferred control/audio
//! plan — see the [`modular`](super) module docs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rill_core::queues::CommandEnum;
use rill_core::traits::Eurorack;
use rill_core_actor::ActorRef;

/// A single Eurorack processing case.
pub struct RackCase<const BUF: usize> {
    name: String,
    sample_rate: f32,
    actor_ref: ActorRef<CommandEnum>,
    tasks: Vec<std::thread::JoinHandle<()>>,

    /// Program thread handle — the rack's rill-lang program runs here.
    signal_thread: Option<std::thread::JoinHandle<()>>,

    /// Stop flag for the program thread's run loop.
    running: Option<Arc<AtomicBool>>,

    /// The run-time error reported by the program thread, if the program
    /// failed after launch. Written by the thread, read by
    /// [`run_error`](Self::run_error) once the thread has been joined.
    run_error: Arc<Mutex<Option<String>>>,
}

impl<const BUF: usize> RackCase<BUF> {
    /// Create a new case.
    #[cfg(feature = "serialization")]
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
            run_error: Arc::new(Mutex::new(None)),
        }
    }

    /// The error reported by this case's program thread, if the program failed
    /// at run time. Compile failures are returned by `launch`, so they never
    /// reach this slot.
    ///
    /// The slot is written when the run returns, so callers should
    /// [`stop`](Self::stop) (which joins the thread) before reading it.
    pub fn run_error(&self) -> Option<String> {
        match self.run_error.lock() {
            Ok(slot) => slot.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Return the actor handle for sending commands TO this case.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
    }

    /// Start the program thread for this case.
    ///
    /// `build` receives the thread's stop flag and the slot into which it must
    /// record a run-time error (see [`run_error`](Self::run_error)).
    #[cfg(feature = "serialization")]
    pub(crate) fn start<F>(&mut self, build: F)
    where
        F: FnOnce(Arc<AtomicBool>, Arc<Mutex<Option<String>>>) + Send + 'static,
    {
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let error_slot = self.run_error.clone();
        let handle = std::thread::spawn(move || build(r, error_slot));
        self.running = Some(running);
        self.signal_thread = Some(handle);
    }

    /// Stop the program thread and all module tasks.
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
