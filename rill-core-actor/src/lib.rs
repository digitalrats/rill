//! # Rill Core Actor — actor model infrastructure
//!
//! A lightweight, domain-agnostic actor model for lock-free message passing.
//! Generic over the message type `M: Send + 'static` — no coupling to audio,
//! signal processing, or any specific domain.
//!
//! ## Key types
//!
//! | Type | Role | Analogy (Akka/Pekko) |
//! |------|------|----------------------|
//! | [`ActorCell`] | Trait: "I can receive and process messages" | `ActorCell` |
//! | [`ActorRef<M>`] | Thread-safe handle to send messages to an actor | `ActorRef` |
//! | [`MessageDispatcher<M>`] | ActorRef + dead letters queue | — |
//! | [`ActorSystem<M>`] | Named mailbox registry with routing and dead letters | `ActorSystem` |
//!
//! ## RT boundary
//!
//! The mailbox is the **hard boundary** between soft-RT and hard-RT code:
//!
//! ```text
//! Soft-RT (any thread, tokio, OS calls)      Hard-RT (actor's thread)
//! ┌────────────────────────────┐             ┌────────────────────────┐
//! │ Engine::handle_event()     │             │ Graph::receive()      │
//! │ OSC dispatcher             │   send()    │   nodes[idx].set()    │
//! │ PortCombiner (tokio task)  │ ──────────→ │   Port::propagate()   │
//! │ Sequencer (spawn_blocking) │   mailbox   │   (no alloc, no lock) │
//! └────────────────────────────┘             └────────────────────────┘
//! ```
//!
//! - **`send()`** — lock-free, safe from any thread including RT. Bounded
//!   queue (capacity 64) prevents RT thread overload.
//! - **`receive()`** — runs on the **actor's thread**. The actor
//!   implementation determines the RT guarantees. If called from an audio
//!   callback, it must obey the callback's RT constraints (no alloc, no
//!   syscalls, no locks). The actor framework itself does not enforce this
//!   — it is the actor's responsibility.
//! - **`route()` / `broadcast()`** — soft-RT (may use heap iteration).
//!   Call from non-RT threads only.
//!
//! ## Architecture
//!
//! The actor owns its mailbox (`Arc<MpscQueue<M>>`). `ActorRef` holds a
//! strong reference to that same `Arc` — it is a borrowed handle, not the
//! owner. Multiple `ActorRef`s can share the same mailbox.
//!
//! ```text
//! Actor (owns the mailbox Arc)
//!   │
//!   ├── drains via mailbox.pop() in its processing loop
//!   │
//!   └── Graph::handle() → ActorRef<SetParameter>  (shared handle)
//!                              │
//!                              ├── cloned for Engine
//!                              ├── cloned for PortCombiner
//!                              └── cloned for OSC dispatcher
//! ```
//!
//! ## Domain independence
//!
//! This crate has no concept of audio, signal processing, or `SetParameter`.
//! `ActorRef<M>` works with any `M: Send + 'static`:
//!
//! ```rust
//! use rill_core_actor::ActorRef;
//! use rill_core::queues::MpscQueue;
//! use std::sync::Arc;
//!
//! // String messages
//! let (ar, mbox) = ActorRef::<String>::new_pair();
//! ar.send("hello".into());
//! assert_eq!(mbox.pop(), Some("hello".into()));
//!
//! // Integer messages
//! let (ar, mbox) = ActorRef::<i32>::new_pair();
//! ar.send(42);
//! assert_eq!(mbox.pop(), Some(42));
//! ```
//!
//! The concrete message type (`SetParameter`) and its consumer (`Graph`)
//! belong to higher-level crates (`rill-patchbay`, `rill-graph`).

use std::sync::Arc;

use rill_core::queues::MpscQueue;

// ============================================================================
// ActorCell
// ============================================================================

/// Trait for types that own a mailbox and process messages from it.
///
/// Analogous to Akka/Pekko's `ActorCell` — the internal environment that
/// makes a component an actor. Implement this trait on types that hold an
/// [`Arc<MpscQueue<Self::Msg>>`](rill_core::queues::MpscQueue) and process
/// messages by draining it in their processing loop.
///
/// # Example
///
/// ```rust
/// use rill_core_actor::ActorCell;
/// use rill_core::queues::MpscQueue;
/// use std::sync::Arc;
///
/// struct MyActor {
///     mailbox: Arc<MpscQueue<String>>,
///     log: Vec<String>,
/// }
///
/// impl ActorCell for MyActor {
///     type Msg = String;
///     fn receive(&mut self, msg: String) {
///         self.log.push(msg);
///     }
/// }
/// ```
pub trait ActorCell: Send + 'static {
    /// The message type this actor can process.
    type Msg: Send + 'static;

    /// Process a single message.
    ///
    /// Called when the actor drains its mailbox. Runs on the actor's own
    /// thread or processing loop — the actor implementation determines its
    /// own real-time guarantees.
    ///
    /// # RT safety
    ///
    /// `rill-core-actor` itself does not enforce RT constraints. The caller
    /// (the actor's consumer, e.g. an audio callback) decides the RT profile:
    ///
    /// - **Hard RT** (audio callback): `receive()` must not allocate, block,
    ///   or make syscalls. The mailbox is lock-free and bounded, so `send()`
    ///   is safe from any thread, but the actor's `receive()` shares the
    ///   callback's RT constraints.
    /// - **Soft RT** (tokio task, dedicated thread): `receive()` may use
    ///   heap, locks, or I/O as appropriate for its thread.
    ///
    /// The actor framework provides the mailbox; the actor implementation
    /// provides the discipline.
    fn receive(&mut self, msg: Self::Msg);
}

// ============================================================================
// ActorRef
// ============================================================================

/// Thread-safe handle for sending messages to an actor.
///
/// Holds a **strong** reference (`Arc`) to the actor's mailbox. The mailbox
/// lives as long as the actor or any `ActorRef` keeps it alive.
///
/// # Lifecycle
///
/// The actor's mailbox is **owned by the actor** (e.g. a `Graph` struct).
/// `ActorRef` is a borrowed handle — it does not create or own queues.
/// Obtain one via the actor's public API (e.g. `Graph::handle()`).
///
/// ```rust
/// use rill_core_actor::ActorRef;
/// use rill_core::queues::MpscQueue;
/// use std::sync::Arc;
///
/// // The actor owns the mailbox:
/// let mailbox = Arc::new(MpscQueue::<String>::with_capacity(64));
///
/// // ActorRef is a handle — many can share the same mailbox:
/// let ref_a = ActorRef::new(&mailbox);
/// let ref_b = ActorRef::new(&mailbox);
///
/// ref_a.send("msg1".into());
/// ref_b.send("msg2".into());
///
/// // The actor drains its own Arc:
/// assert_eq!(mailbox.pop(), Some("msg1".into()));
/// assert_eq!(mailbox.pop(), Some("msg2".into()));
/// ```
///
/// # Thread safety
///
/// `ActorRef` is `Send + Sync` — safe to share across threads.
/// `send()` is lock-free and can be called from any thread (including
/// real-time audio callbacks).
#[derive(Clone)]
pub struct ActorRef<M: Send + 'static> {
    mailbox: Arc<MpscQueue<M>>,
}

impl<M: Send + 'static> ActorRef<M> {
    /// Create a new `ActorRef` from the actor's mailbox.
    ///
    /// The caller (the actor) owns the `Arc<MpscQueue<M>>`. The returned
    /// `ActorRef` shares the same `Arc` — the actor must keep its `Arc`
    /// alive for the `ActorRef` to function.
    pub fn new(mailbox: &Arc<MpscQueue<M>>) -> Self {
        Self {
            mailbox: mailbox.clone(),
        }
    }

    /// Create a new `(ActorRef, Arc<MpscQueue>)` pair with a fresh mailbox.
    ///
    /// This is a convenience constructor for simple setups where the actor
    /// does not pre-own a mailbox. The caller should typically store the
    /// `Arc<MpscQueue<M>>` (the mailbox) in the actor and keep the
    /// `ActorRef` for external communication.
    ///
    /// The mailbox has capacity **64** (bounded). If the queue is full,
    /// [`send`](Self::send) silently drops the message.
    pub fn new_pair() -> (Self, Arc<MpscQueue<M>>) {
        let mbox = Arc::new(MpscQueue::with_capacity(64));
        let this = Self::new(&mbox);
        (this, mbox)
    }

    /// Send a message to the actor.
    ///
    /// Pushes the message into the actor's lock-free MPSC queue. If the
    /// queue is full the message is silently dropped (bounded queue —
    /// capacity 64 prevents RT thread overload).
    ///
    /// # RT safety
    ///
    /// Lock-free, no allocation on the hot path. Safe to call from **any
    /// thread** including real-time audio callbacks. This is the only
    /// actor infrastructure method that is RT-safe — the actor's
    /// [`receive`](ActorCell::receive) runs on the consumer's thread
    /// and must obey that thread's RT constraints.
    pub fn send(&self, msg: M) {
        let _ = self.mailbox.push(msg);
    }
}

// ============================================================================
// MessageDispatcher
// ============================================================================

/// Dispatcher that routes messages to an actor with a dead letters fallback.
///
/// Combines an [`ActorRef`] for normal delivery with a separate dead letters
/// queue (`Arc<MpscQueue<M>>`) for messages that cannot be delivered
/// (e.g. when the actor has been stopped and detected stale).
///
/// This is the application-level "actor system" — it does not create or
/// own the underlying queues. Those belong to the actor and the Runtime
/// respectively.
///
/// # Dead letters
///
/// Dead letters are messages that could not be delivered. This can happen
/// when:
/// - The actor's mailbox is full (bounded queue reached capacity)
/// - The actor has been stopped and the application layer detected staleness
///
/// Unlike [`ActorRef::send`], `MessageDispatcher` provides an explicit
/// [`send_dead`](Self::send_dead) method for routing messages directly to
/// dead letters without attempting delivery to the actor.
///
/// # Example
///
/// ```rust
/// use rill_core_actor::{ActorRef, MessageDispatcher};
/// use rill_core::queues::MpscQueue;
/// use std::sync::Arc;
///
/// let mailbox = Arc::new(MpscQueue::with_capacity(64));
/// let dead = Arc::new(MpscQueue::new());  // unbounded
///
/// let dispatcher = MessageDispatcher::new(
///     ActorRef::new(&mailbox),
///     dead,
/// );
///
/// // Normal delivery
/// dispatcher.send("normal".to_string());
/// assert_eq!(mailbox.pop(), Some("normal".to_string()));
///
/// // Undeliverable — route to dead letters
/// dispatcher.send_dead("orphaned".to_string());
/// assert_eq!(dispatcher.drain_dead(), vec!["orphaned".to_string()]);
/// ```
#[derive(Clone)]
pub struct MessageDispatcher<M: Send + 'static> {
    actor_ref: ActorRef<M>,
    dead: Arc<MpscQueue<M>>,
}

impl<M: Send + 'static> MessageDispatcher<M> {
    /// Create a new dispatcher.
    ///
    /// # Parameters
    ///
    /// * `actor_ref` — reference to the actor's mailbox (obtain via
    ///   `Graph::handle()` or similar).
    /// * `dead` — unbounded dead letters queue, owned by the Runtime
    ///   or other top-level coordinator.
    pub fn new(actor_ref: ActorRef<M>, dead: Arc<MpscQueue<M>>) -> Self {
        Self { actor_ref, dead }
    }

    /// Send a message to the actor.
    ///
    /// Delegates to [`ActorRef::send`]. If the actor's mailbox is full
    /// the message is silently dropped (bounded queue — the underlying
    /// `MpscQueue` does not return the value on overflow).
    pub fn send(&self, msg: M) {
        self.actor_ref.send(msg);
    }

    /// Send a message directly to dead letters without attempting delivery.
    ///
    /// Use this when the actor is known to be stale (detected by the
    /// application layer, e.g. Runtime health check).
    pub fn send_dead(&self, msg: M) {
        let _ = self.dead.push(msg);
    }

    /// Drain the dead letters queue and return all undelivered messages.
    ///
    /// This is an O(n) operation that removes all messages from the
    /// dead letters queue. Use for inspection, logging, or replay.
    pub fn drain_dead(&self) -> Vec<M> {
        let mut msgs = Vec::new();
        while let Some(msg) = self.dead.pop() {
            msgs.push(msg);
        }
        msgs
    }

    /// Check whether there are any undelivered messages in dead letters.
    pub fn has_dead(&self) -> bool {
        !self.dead.is_empty()
    }

    /// Borrow the inner [`ActorRef`].
    ///
    /// Useful when the caller needs to pass the `ActorRef` to code that
    /// does not need dead letters support (e.g. `PortCombiner`).
    pub fn actor_ref(&self) -> &ActorRef<M> {
        &self.actor_ref
    }
}

// ============================================================================
// ActorSystem
// ============================================================================

/// Central registry of named mailboxes with dead letters support.
///
/// Routes messages to registered actors by name. If a target does not
/// exist, the message is forwarded to dead letters instead of being
/// silently dropped.
///
/// All actors in a system share the same message type `M`. For systems
/// that need different message types, create separate `ActorSystem`
/// instances (one per message type).
///
/// # Multiple consumers
///
/// Each registered mailbox is an [`Arc<MpscQueue<M>>`] that can be
/// drained by a dedicated consumer (e.g. audio callback, tokio task,
/// dedicated thread). This enables multiple actors processing different
/// streams of the same message type:
///
/// ```text
/// ActorSystem<SetParameter>
///   │
///   ├── "graph"   → audio thread consumer (hard RT)
///   ├── "midi"    → tokio task consumer  (soft RT, future)
///   └── "monitor" → tokio task consumer  (soft RT, future)
/// ```
///
/// # Dead letters
///
/// When [`route`](Self::route) is called with a name that is not
/// registered, the message goes to the system's dead letters queue.
/// Use [`drain_dead`](Self::drain_dead) to inspect undelivered messages.
///
/// # Example
///
/// ```rust
/// use rill_core_actor::ActorSystem;
/// use rill_core::queues::MpscQueue;
/// use std::sync::Arc;
///
/// let mut system = ActorSystem::<String>::new();
///
/// // Register two actors
/// let graph_mbox = system.register("graph");
/// let midi_mbox = system.register("midi");
///
/// // Route a message to a specific actor
/// system.route("graph", "hello graph".to_string());
/// assert_eq!(graph_mbox.pop(), Some("hello graph".to_string()));
///
/// // Unknown actor → dead letters
/// system.route("unknown", "lost".to_string());
/// assert_eq!(system.drain_dead(), vec!["lost".to_string()]);
///
/// // Broadcast to all registered actors
/// system.broadcast("to all".to_string());
/// assert_eq!(graph_mbox.pop(), Some("to all".to_string()));
/// assert_eq!(midi_mbox.pop(), Some("to all".to_string()));
/// ```
pub struct ActorSystem<M: Send + 'static> {
    actors: Vec<(String, Arc<MpscQueue<M>>)>,
    dead: Arc<MpscQueue<M>>,
}

impl<M: Send + 'static> ActorSystem<M> {
    /// Create an empty system.
    pub fn new() -> Self {
        Self {
            actors: Vec::new(),
            dead: Arc::new(MpscQueue::new()),
        }
    }

    /// Register a new named mailbox and return it.
    ///
    /// The caller is responsible for creating a consumer that drains
    /// the returned `Arc<MpscQueue<M>>`.
    pub fn register(&mut self, name: &str) -> Arc<MpscQueue<M>> {
        let mbox = Arc::new(MpscQueue::with_capacity(64));
        self.actors.push((name.to_string(), mbox.clone()));
        mbox
    }

    /// Route a message to a named actor.
    ///
    /// If the name is registered, the message is pushed to that actor's
    /// mailbox. Otherwise it is forwarded to dead letters.
    ///
    /// # RT safety
    ///
    /// **Soft-RT only.** Iterates the actor list (heap access).
    /// Must not be called from hard-RT threads (audio callbacks).
    pub fn route(&self, name: &str, msg: M) {
        for (n, mbox) in &self.actors {
            if n == name {
                let _ = mbox.push(msg);
                return;
            }
        }
        let _ = self.dead.push(msg);
    }

    /// Broadcast a message to all registered actors.
    ///
    /// Each actor receives a copy (the message is cloned).
    /// Messages that cannot be delivered (full mailbox) are silently
    /// dropped per-actor.
    ///
    /// # RT safety
    ///
    /// **Soft-RT only.** May clone the message (allocation) and iterate
    /// the actor list. Must not be called from hard-RT threads.
    pub fn broadcast(&self, msg: M)
    where
        M: Clone,
    {
        for (_, mbox) in &self.actors {
            let _ = mbox.push(msg.clone());
        }
    }

    /// Drain the dead letters queue for inspection.
    pub fn drain_dead(&self) -> Vec<M> {
        let mut msgs = Vec::new();
        while let Some(msg) = self.dead.pop() {
            msgs.push(msg);
        }
        msgs
    }

    /// Check whether there are any undelivered messages.
    pub fn has_dead(&self) -> bool {
        !self.dead.is_empty()
    }

    /// Number of registered actors.
    pub fn actor_count(&self) -> usize {
        self.actors.len()
    }

    /// Access the dead letters queue directly.
    pub fn dead_letters(&self) -> &MpscQueue<M> {
        &self.dead
    }
}

impl<M: Send + 'static> Default for ActorSystem<M> {
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

    struct TestActor {
        mailbox: Arc<MpscQueue<String>>,
        received: Vec<String>,
    }

    impl TestActor {
        fn new(mailbox: Arc<MpscQueue<String>>) -> Self {
            Self {
                mailbox,
                received: Vec::new(),
            }
        }
        fn drain(&mut self) {
            while let Some(msg) = self.mailbox.pop() {
                self.receive(msg);
            }
        }
    }

    impl ActorCell for TestActor {
        type Msg = String;
        fn receive(&mut self, msg: String) {
            self.received.push(msg);
        }
    }

    #[test]
    fn test_actor_ref_send_and_drain() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let actor_ref = ActorRef::new(&mailbox);

        actor_ref.send("hello".to_string());
        actor_ref.send("world".to_string());

        let mut actor = TestActor::new(mailbox);
        actor.drain();

        assert_eq!(actor.received.len(), 2);
        assert_eq!(actor.received[0], "hello");
        assert_eq!(actor.received[1], "world");
    }

    #[test]
    fn test_multiple_refs_share_mailbox() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let ref_a = ActorRef::new(&mailbox);
        let ref_b = ActorRef::new(&mailbox);

        ref_a.send("alpha".to_string());
        ref_b.send("beta".to_string());

        let mut count = 0;
        while mailbox.pop().is_some() {
            count += 1;
        }
        assert_eq!(count, 2);
    }

    #[test]
    fn test_queue_overflow_drops() {
        // capacity 2 — third message is silently dropped
        let mailbox = Arc::new(MpscQueue::with_capacity(2));
        let actor_ref = ActorRef::new(&mailbox);

        actor_ref.send(1);
        actor_ref.send(2);
        actor_ref.send(3); // dropped

        assert_eq!(mailbox.pop(), Some(1));
        assert_eq!(mailbox.pop(), Some(2));
        assert!(mailbox.pop().is_none());
    }

    #[test]
    fn test_new_pair_returns_connected_pair() {
        let (actor_ref, mailbox) = ActorRef::<String>::new_pair();
        actor_ref.send("via_ref".to_string());
        assert_eq!(mailbox.pop(), Some("via_ref".to_string()));
    }

    #[test]
    fn test_dispatcher_forwards_to_mailbox() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let dead = Arc::new(MpscQueue::new());
        let actor_ref = ActorRef::new(&mailbox);
        let dispatcher = MessageDispatcher::new(actor_ref, dead);

        dispatcher.send("normal".to_string());

        let mut actor = TestActor::new(mailbox);
        actor.drain();
        assert_eq!(actor.received, vec!["normal"]);
    }

    #[test]
    fn test_dispatcher_dead_letters() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let dead = Arc::new(MpscQueue::new());
        let actor_ref = ActorRef::new(&mailbox);
        let dispatcher = MessageDispatcher::new(actor_ref, dead.clone());

        dispatcher.send_dead("orphaned".to_string());

        let drained = dispatcher.drain_dead();
        assert_eq!(drained, vec!["orphaned"]);
        assert!(dead.is_empty()); // drained via dispatcher
    }

    #[test]
    fn test_different_message_types() {
        // String messages
        let (ar, mbox) = ActorRef::<String>::new_pair();
        ar.send("hello".into());
        assert_eq!(mbox.pop(), Some("hello".into()));

        // Integer messages
        let (ar, mbox) = ActorRef::<i32>::new_pair();
        ar.send(42);
        assert_eq!(mbox.pop(), Some(42));
    }
}
