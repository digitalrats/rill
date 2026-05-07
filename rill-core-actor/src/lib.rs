//! # Rill Core Actor — actor model infrastructure
//!
//! Provides the foundational types for the Rill actor system:
//!
//! * [`ActorCell`] — trait for types that process messages
//! * [`ActorRef`] — thread-safe handle for sending messages to an actor
//! * [`MessageDispatcher`] — dispatcher with dead letters support
//!
//! ## Usage
//!
//! ```ignore
//! use rill_core_actor::{ActorCell, ActorRef, MessageDispatcher};
//! use rill_core::queues::MpscQueue;
//! use std::sync::Arc;
//!
//! let mailbox = Arc::new(MpscQueue::with_capacity(64));
//! let dead = Arc::new(MpscQueue::new());
//! let actor_ref = ActorRef::new(&mailbox);
//! let dispatcher = MessageDispatcher::new(actor_ref, dead);
//! ```

use std::sync::Arc;

use rill_core::queues::MpscQueue;

// ============================================================================
// ActorCell
// ============================================================================

/// Something that can receive and process messages.
///
/// Analogous to Akka/Pekko's `ActorCell` — the internal environment that
/// makes a component an actor. Implement this trait on types that own
/// a mailbox (an [`MpscQueue`]) and process messages from it.
pub trait ActorCell: Send + 'static {
    /// The message type this actor can process.
    type Msg: Send + 'static;

    /// Process a single message.
    fn receive(&mut self, msg: Self::Msg);
}

// ============================================================================
// ActorRef
// ============================================================================

/// Thread-safe handle for sending messages to an actor.
///
/// Holds a strong reference (`Arc`) to the actor's mailbox. The mailbox
/// lives as long as the actor or any `ActorRef` keeps it alive.
///
/// Multiple `ActorRef`s can exist for the same actor — they all share
/// the same underlying lock-free MPSC queue.
///
/// # Lifecycle
///
/// The actor's mailbox is **owned by the actor** (e.g. `Graph`).
/// `ActorRef` is just a borrowed handle — it does not create or own
/// queues. Obtain one via the actor's public API (e.g. `Graph::handle()`).
#[derive(Clone)]
pub struct ActorRef<M: Send + 'static> {
    mailbox: Arc<MpscQueue<M>>,
}

impl<M: Send + 'static> ActorRef<M> {
    /// Create a new `ActorRef` from the actor's mailbox.
    pub fn new(mailbox: &Arc<MpscQueue<M>>) -> Self {
        Self {
            mailbox: mailbox.clone(),
        }
    }

    /// Create a new `(ActorRef, Arc<MpscQueue>)` pair with a fresh mailbox.
    ///
    /// The caller should store the `Arc<MpscQueue>` in the actor and
    /// keep the `ActorRef` for external communication. The mailbox
    /// has capacity 64 (bounded).
    pub fn new_pair() -> (Self, Arc<MpscQueue<M>>) {
        let mbox = Arc::new(MpscQueue::with_capacity(64));
        let this = Self::new(&mbox);
        (this, mbox)
    }

    /// Send a message to the actor.
    ///
    /// The message is pushed into the actor's lock-free MPSC queue.
    /// If the queue is full the message is silently dropped (bounded queue).
    pub fn send(&self, msg: M) {
        let _ = self.mailbox.push(msg);
    }
}

// ============================================================================
// MessageDispatcher
// ============================================================================

/// Dispatcher that routes messages to an actor with a dead letters fallback.
///
/// Holds an [`ActorRef`] for normal delivery and a separate dead letters
/// queue for messages that cannot be delivered (stale actor detected
/// externally).
///
/// This is the application-level "actor system" — it does not create or
/// own the queues (those belong to the actor and the Runtime respectively).
#[derive(Clone)]
pub struct MessageDispatcher<M: Send + 'static> {
    actor_ref: ActorRef<M>,
    dead: Arc<MpscQueue<M>>,
}

impl<M: Send + 'static> MessageDispatcher<M> {
    /// Create a new dispatcher.
    ///
    /// * `actor_ref` — reference to the actor's mailbox (from `Graph::handle()` etc.)
    /// * `dead` — unbounded dead letters queue (owned by the Runtime)
    pub fn new(actor_ref: ActorRef<M>, dead: Arc<MpscQueue<M>>) -> Self {
        Self { actor_ref, dead }
    }

    /// Send a message to the actor.
    ///
    /// Delegates to [`ActorRef::send`]. If the queue is full the message
    /// is silently dropped (bounded queue limitation).
    pub fn send(&self, msg: M) {
        self.actor_ref.send(msg);
    }

    /// Send a message directly to dead letters (actor is known stale).
    pub fn send_dead(&self, msg: M) {
        let _ = self.dead.push(msg);
    }

    /// Drain the dead letters queue for inspection.
    pub fn drain_dead(&self) -> Vec<M> {
        let mut msgs = Vec::new();
        while let Some(msg) = self.dead.pop() {
            msgs.push(msg);
        }
        msgs
    }

    /// Check whether there are any dead letters.
    pub fn has_dead(&self) -> bool {
        !self.dead.is_empty()
    }

    /// Access the inner [`ActorRef`].
    pub fn actor_ref(&self) -> &ActorRef<M> {
        &self.actor_ref
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
}
