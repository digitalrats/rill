//! # Rill Core Actor — actor model infrastructure
//!
//! A lightweight, domain-agnostic actor model for lock-free message passing.
//! Actors are single-threaded: handler is created and drained on the same thread.
//!
//! ## Key types
//!
//! | Type | Role |
//! |------|------|
//! | [`Actor<M>`] | Handler + mailbox — drained in-place (no separate thread) |
//! | [`ActorRef<M>`] | Thread-safe handle to send messages to an actor |
//! | [`ActorSystem`] | Named actor registry, dead letters, `spawn()`, `spawn_detached()` |
//!
//! ## Architecture
//!
//! ```text
//! // Handler drained inline (Graph, Rack):
//! system.spawn(name, handler) → Actor<M> → actor.drain() on caller's thread
//!
//! // Handler created & drained inside a new thread (Servo, factory modules):
//! system.spawn_detached(name, make_handler, ms) → ActorRef<M>
//!   └── thread::spawn: handler = make_handler() → actor.drain() loop
//! ```

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rill_core::queues::MpscQueue;

// ============================================================================
// Mailbox — private, never visible outside this crate
// ============================================================================

pub struct Mailbox<M: Send + 'static> {
    pub(crate) queue: MpscQueue<M>,
    alive: AtomicBool,
}

impl<M: Send + 'static> Mailbox<M> {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: MpscQueue::with_capacity(capacity),
            alive: AtomicBool::new(true),
        }
    }

    pub fn pop(&self) -> Option<M> {
        self.queue.pop()
    }

    pub fn actor_ref(self: &Arc<Self>) -> ActorRef<M> {
        ActorRef {
            inner: self.clone(),
        }
    }
}

// ============================================================================
// Actor — single‑threaded (handler: !Send, drained inline)
// ============================================================================

pub struct Actor<M: Send + 'static> {
    mailbox: Arc<Mailbox<M>>,
    handler: Box<dyn FnMut(M) + 'static>,
}

impl<M: Send + 'static> Actor<M> {
    pub fn drain(&mut self) {
        while let Some(msg) = self.mailbox.pop() {
            (self.handler)(msg);
        }
    }

    pub fn actor_ref(&self) -> ActorRef<M> {
        self.mailbox.actor_ref()
    }
}

// ============================================================================
// ActorRef
// ============================================================================

pub struct ActorRef<M: Send + 'static> {
    inner: Arc<Mailbox<M>>,
}

impl<M: Send + 'static> Clone for ActorRef<M> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<M: Send + 'static> ActorRef<M> {
    pub fn send(&self, msg: M) {
        if self.inner.alive.load(Ordering::Acquire) {
            let _ = self.inner.queue.push(msg);
        }
    }
}

// ============================================================================
// ActorSystem
// ============================================================================

pub struct ActorSystem {
    actors: Mutex<Vec<(String, Box<dyn Any + Send>)>>,
    dead: Arc<MpscQueue<Box<dyn Any + Send>>>,
}

impl ActorSystem {
    pub fn new() -> Self {
        Self {
            actors: Mutex::new(Vec::new()),
            dead: Arc::new(MpscQueue::new()),
        }
    }

    /// Spawn an actor drained inline by the caller (Graph, Rack).
    /// Handler does not need `Send` — it lives and dies on the caller's thread.
    pub fn spawn<M: Send + 'static>(
        &self,
        name: &str,
        handler: impl FnMut(M) + 'static,
    ) -> Actor<M> {
        let actor = Actor {
            mailbox: Arc::new(Mailbox::new(64)),
            handler: Box::new(handler),
        };
        self.actors
            .lock()
            .unwrap()
            .push((name.to_string(), Box::new(actor.actor_ref())));
        actor
    }

    /// Spawn a detached actor — handler is created inside a new OS thread
    /// and drained in a loop. Returns the [`ActorRef`] immediately.
    ///
    /// `make_handler` is called inside the spawned thread, so the returned
    /// handler closure does not need `Send`.
    pub fn spawn_detached<M: Send + 'static>(
        &self,
        name: &str,
        make_handler: impl FnOnce() -> Box<dyn FnMut(M) + 'static> + Send + 'static,
        interval_ms: u64,
    ) -> ActorRef<M> {
        let mailbox = Arc::new(Mailbox::new(64));
        let actor_ref = mailbox.actor_ref();
        self.actors
            .lock()
            .unwrap()
            .push((name.to_string(), Box::new(actor_ref.clone())));
        std::thread::spawn(move || {
            let handler = make_handler();
            let mut actor = Actor { mailbox, handler };
            loop {
                actor.drain();
                std::thread::sleep(Duration::from_millis(interval_ms));
            }
        });
        actor_ref
    }

    /// Spawn a detached actor on a tokio task — handler must be `Send`.
    /// Useful when many actors are needed (e.g. 24 Servos), avoiding OS thread overhead.
    #[cfg(feature = "tokio")]
    pub fn spawn_detached_tokio<M: Send + 'static>(
        &self,
        name: &str,
        make_handler: impl FnOnce() -> Box<dyn FnMut(M) + Send + 'static> + Send + 'static,
        interval_ms: u64,
    ) -> ActorRef<M> {
        let mailbox = Arc::new(Mailbox::new(64));
        let actor_ref = mailbox.actor_ref();
        self.actors
            .lock()
            .unwrap()
            .push((name.to_string(), Box::new(actor_ref.clone())));
        tokio::spawn(async move {
            let mut handler = make_handler();
            let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
            loop {
                interval.tick().await;
                while let Some(msg) = mailbox.pop() {
                    handler(msg);
                }
            }
        });
        actor_ref
    }

    pub fn route<M: Send + 'static>(&self, name: &str, msg: M) {
        if let Ok(actors) = self.actors.lock() {
            for (n, actor_ref) in actors.iter() {
                if n == name {
                    if let Some(ar) = actor_ref.downcast_ref::<ActorRef<M>>() {
                        ar.send(msg);
                        return;
                    }
                }
            }
        }
        let _ = self.dead.push(Box::new(msg));
    }

    pub fn broadcast<M: Send + Clone + 'static>(&self, msg: M) {
        if let Ok(actors) = self.actors.lock() {
            for (_, actor_ref) in actors.iter() {
                if let Some(ar) = actor_ref.downcast_ref::<ActorRef<M>>() {
                    ar.send(msg.clone());
                }
            }
        }
    }

    pub fn drain_dead(&self) -> Vec<Box<dyn Any + Send>> {
        let mut msgs = Vec::new();
        while let Some(msg) = self.dead.pop() {
            msgs.push(msg);
        }
        msgs
    }

    pub fn actor_count(&self) -> usize {
        self.actors.lock().map(|a| a.len()).unwrap_or(0)
    }
}

impl Default for ActorSystem {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_and_drain() {
        let system = ActorSystem::new();
        let received = Arc::new(Mutex::new(Vec::new()));
        let recv = received.clone();
        let mut actor = system.spawn("test", move |msg: String| {
            recv.lock().unwrap().push(msg);
        });
        assert_eq!(system.actor_count(), 1);

        let ref_a = actor.actor_ref();
        ref_a.send("hello".into());
        ref_a.send("world".into());

        actor.drain();
        assert_eq!(received.lock().unwrap().len(), 2);
    }

    #[test]
    fn test_multiple_refs_share_mailbox() {
        let system = ActorSystem::new();
        let log = Arc::new(Mutex::new(Vec::new()));
        let l = log.clone();
        let mut actor = system.spawn("multi", move |msg: String| {
            l.lock().unwrap().push(msg);
        });

        let ref_a = actor.actor_ref();
        let ref_b = actor.actor_ref();
        ref_a.send("alpha".into());
        ref_b.send("beta".into());

        actor.drain();
        let drained = log.lock().unwrap();
        assert_eq!(drained.len(), 2);
    }

    #[test]
    fn test_queue_overflow_drops() {
        let system = ActorSystem::new();
        let sum = Arc::new(Mutex::new(0));
        let s = sum.clone();
        let mut actor = system.spawn("drop", move |msg: i32| {
            *s.lock().unwrap() += msg;
        });

        let ref_a = actor.actor_ref();
        for i in 0..200 {
            ref_a.send(i);
        }
        actor.drain();
        let total = *sum.lock().unwrap();
        assert!(total > 0);
        assert!(total < (0..200).sum::<i32>());
    }

    #[test]
    fn test_route() {
        let system = ActorSystem::new();
        let log = Arc::new(Mutex::new(Vec::new()));
        let l = log.clone();
        let mut actor = system.spawn("echo", move |msg: String| {
            l.lock().unwrap().push(msg);
        });

        system.route("echo", "routed".to_string());
        actor.drain();
        assert_eq!(*log.lock().unwrap(), vec!["routed".to_string()]);
    }

    #[test]
    fn test_route_unknown_goes_to_dead() {
        let system = ActorSystem::new();
        system.route("unknown", "lost".to_string());
        let dead: Vec<String> = system
            .drain_dead()
            .into_iter()
            .filter_map(|b| b.downcast::<String>().ok().map(|b| *b))
            .collect();
        assert_eq!(dead, vec!["lost".to_string()]);
    }

    #[test]
    fn test_broadcast() {
        let system = ActorSystem::new();
        let log_a = Arc::new(Mutex::new(Vec::new()));
        let log_b = Arc::new(Mutex::new(Vec::new()));
        let la = log_a.clone();
        let lb = log_b.clone();
        let mut actor_a = system.spawn("a", move |msg: String| {
            la.lock().unwrap().push(msg);
        });
        let mut actor_b = system.spawn("b", move |msg: String| {
            lb.lock().unwrap().push(msg);
        });

        system.broadcast("all".to_string());
        actor_a.drain();
        actor_b.drain();
        assert_eq!(*log_a.lock().unwrap(), vec!["all".to_string()]);
        assert_eq!(*log_b.lock().unwrap(), vec!["all".to_string()]);
    }

    #[test]
    fn test_spawn_detached() {
        let system = ActorSystem::new();
        let received = Arc::new(Mutex::new(Vec::new()));
        let recv = received.clone();
        let actor_ref = system.spawn_detached(
            "detached",
            move || Box::new(move |msg: String| recv.lock().unwrap().push(msg)),
            1,
        );
        actor_ref.send("hello".into());
        actor_ref.send("world".into());
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(received.lock().unwrap().len(), 2);
    }
}
