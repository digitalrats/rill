use std::sync::Arc;

use crate::queues::MpscQueue;

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

    /// Send a message to the actor.
    ///
    /// The message is pushed into the actor's lock-free MPSC queue.
    /// If the queue is full the message is silently dropped (bounded queue).
    pub fn send(&self, msg: M) {
        let _ = self.mailbox.push(msg);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queues::MpscQueue;
    use std::sync::Arc;

    /// A simple test actor that records received messages.
    struct StringCollector {
        mailbox: Arc<MpscQueue<String>>,
        received: Vec<String>,
    }

    impl StringCollector {
        fn new(mailbox: Arc<MpscQueue<String>>) -> Self {
            Self {
                mailbox,
                received: Vec::new(),
            }
        }

        /// Drain the mailbox and process all pending messages.
        fn drain(&mut self) {
            while let Some(msg) = self.mailbox.pop() {
                self.receive(msg);
            }
        }
    }

    impl ActorCell for StringCollector {
        type Msg = String;

        fn receive(&mut self, msg: String) {
            self.received.push(msg);
        }
    }

    /// A numeric test actor for the overflow test.
    struct IntCollector {
        mailbox: Arc<MpscQueue<i32>>,
        received: Vec<i32>,
    }

    impl IntCollector {
        fn new(mailbox: Arc<MpscQueue<i32>>) -> Self {
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

    impl ActorCell for IntCollector {
        type Msg = i32;

        fn receive(&mut self, msg: i32) {
            self.received.push(msg);
        }
    }

    // ── Basic send & drain ─────────────────────────────────────────────

    #[test]
    fn test_send_and_drain() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let actor_ref = ActorRef::new(&mailbox);

        actor_ref.send("hello".to_string());
        actor_ref.send("world".to_string());

        let mut actor = StringCollector::new(mailbox);
        actor.drain();

        assert_eq!(actor.received.len(), 2);
        assert_eq!(actor.received[0], "hello");
        assert_eq!(actor.received[1], "world");
    }

    // ── ActorRef does not own the queue ────────────────────────────────

    #[test]
    fn test_actor_ref_does_not_own_queue() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let actor_ref = ActorRef::new(&mailbox);

        actor_ref.send("from_ref".to_string());

        // The actor drains from its own Arc — not through ActorRef
        let mut actor = StringCollector::new(mailbox);
        actor.drain();

        assert_eq!(actor.received, vec!["from_ref"]);
    }

    // ── Multiple ActorRefs share the same mailbox ──────────────────────

    #[test]
    fn test_multiple_refs_share_mailbox() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let ref1 = ActorRef::new(&mailbox);
        let ref2 = ActorRef::new(&mailbox);

        ref1.send("alpha".to_string());
        ref2.send("beta".to_string());

        let mut actor = StringCollector::new(mailbox);
        actor.drain();

        assert_eq!(actor.received.len(), 2);
    }

    // ── Bounded queue overflow ─────────────────────────────────────────

    #[test]
    fn test_queue_overflow_silently_drops() {
        let mailbox = Arc::new(MpscQueue::with_capacity(2));
        let actor_ref = ActorRef::new(&mailbox);

        actor_ref.send(1);
        actor_ref.send(2);
        actor_ref.send(3); // over capacity — silently dropped

        let mut actor = IntCollector::new(mailbox);
        actor.drain();

        assert_eq!(actor.received, vec![1, 2]);
    }

    // ── Empty queue drain ──────────────────────────────────────────────

    #[test]
    fn test_empty_queue_drain_does_nothing() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let mut actor = StringCollector::new(mailbox);
        actor.drain(); // should not panic
        assert!(actor.received.is_empty());
    }

    // ── Dead letters pattern ───────────────────────────────────────────

    #[test]
    fn test_dead_letters_pattern() {
        // This test demonstrates the dead letters pattern at the
        // application level (Runtime / PatchbayControl).
        //
        // The actor owns the mailbox. When the actor is dropped, any
        // code that still holds an ActorRef can detect staleness via
        // a separate mechanism (e.g. a health flag) and route to a
        // dedicated dead letters queue instead.

        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let dead = Arc::new(MpscQueue::new());

        let actor_ref = ActorRef::new(&mailbox);

        // Phase 1: actor alive — messages go to mailbox
        actor_ref.send("normal".to_string());
        assert!(!mailbox.is_empty(), "alive actor receives messages");

        // Drain the alive messages
        let mut actor = StringCollector::new(mailbox.clone());
        actor.drain();
        assert_eq!(actor.received, vec!["normal"]);

        // Phase 2: simulate actor death by dropping the actor's Arc.
        // In reality this happens when Graph is torn down.
        // After this, mailboxes held by ActorRef are still alive (Arc)
        // but nobody processes them.
        //
        // The Runtime detects this (e.g. via a health check or
        // lifecycle hook) and starts routing to dead letters.
        actor_ref.send("orphaned".to_string());

        // Route to dead letters instead of the stale actor
        while let Some(msg) = mailbox.pop() {
            dead.push(msg).ok();
        }

        assert!(mailbox.is_empty(), "stale mailbox drained");
        assert_eq!(dead.pop(), Some("orphaned".to_string()));
    }

    // ── Non-string message type ────────────────────────────────────────

    #[derive(Debug, Clone, PartialEq)]
    struct CustomMsg {
        id: u32,
        value: f64,
    }

    #[test]
    fn test_custom_message_type() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let actor_ref = ActorRef::new(&mailbox);

        actor_ref.send(CustomMsg { id: 1, value: 3.14 });
        actor_ref.send(CustomMsg { id: 2, value: 2.71 });

        let mut count = 0;
        while let Some(msg) = mailbox.pop() {
            count += 1;
            if msg.id == 1 {
                assert!((msg.value - 3.14).abs() < 1e-9);
            }
        }
        assert_eq!(count, 2);
    }

    // ── Send after actor ref is cloned ────────────────────────────────

    #[test]
    fn test_send_after_clone() {
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let ref_a = ActorRef::new(&mailbox);
        let ref_b = ref_a.clone();

        ref_a.send("from_a".to_string());
        ref_b.send("from_b".to_string());

        let mut actor = StringCollector::new(mailbox);
        actor.drain();
        assert_eq!(actor.received.len(), 2);
    }
}
