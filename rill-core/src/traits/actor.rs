use std::sync::{Arc, Weak};

use crate::queues::MpscQueue;

/// Something that can receive and process messages.
///
/// Analogous to Akka/Pekko's `ActorCell` — the internal environment that
/// makes a component an actor. Implement this trait on types that own
/// a mailbox (an [`MpscQueue`]) and process messages from it.
///
/// # Example
///
/// ```ignore
/// impl ActorCell for MyActor {
///     type Msg = MyMessage;
///     fn receive(&mut self, msg: MyMessage) {
///         // handle message
///     }
/// }
/// ```
pub trait ActorCell: Send + 'static {
    /// The message type this actor can process.
    type Msg: Send + 'static;

    /// Process a single message.
    ///
    /// Called when the actor's mailbox is drained (e.g. at the start
    /// of every audio processing cycle).
    fn receive(&mut self, msg: Self::Msg);
}

/// Thread-safe handle for sending messages to an [`ActorCell`].
///
/// Holds a **weak** reference to the actor's mailbox. If the actor has
/// been stopped and its mailbox dropped, `send` routes the message to
/// the **dead letters** queue instead of dropping it silently.
///
/// Multiple `ActorRef`s can exist for the same actor — they all share
/// the same underlying lock-free MPSC queue and dead letters.
///
/// # Dead letters
///
/// The dead letters queue is **unbounded** and never loses messages.
/// Use [`dead_letters`](Self::dead_letters) to inspect or drain
/// undeliverable messages for logging or debugging.
///
/// # Type parameters
///
/// * `M` — the message type (must be `Send + 'static`).
#[derive(Clone)]
pub struct ActorRef<M: Send + 'static> {
    mailbox: Weak<MpscQueue<M>>,
    dead: Arc<MpscQueue<M>>,
}

impl<M: Send + 'static> ActorRef<M> {
    /// Create a new `(ActorRef, Arc<MpscQueue<M>>)` pair.
    ///
    /// The caller should store the `Arc<MpscQueue<M>>` in the actor and
    /// keep the `ActorRef` for external communication. The `ActorRef`
    /// holds a `Weak` reference — when the actor drops its `Arc`, all
    /// subsequent `send` calls route to dead letters.
    pub fn new_pair() -> (Self, Arc<MpscQueue<M>>) {
        let dead = Arc::new(MpscQueue::new());
        let mailbox = Arc::new(MpscQueue::with_capacity(64));
        let this = Self {
            mailbox: Arc::downgrade(&mailbox),
            dead,
        };
        (this, mailbox)
    }

    /// Create an `ActorRef` from an existing mailbox + dead letters.
    pub fn new(mailbox: &Arc<MpscQueue<M>>, dead: Arc<MpscQueue<M>>) -> Self {
        Self {
            mailbox: Arc::downgrade(mailbox),
            dead,
        }
    }

    /// Send a message to the actor.
    ///
    /// Routes to dead letters when the actor is no longer alive
    /// (all strong references to the mailbox were dropped).
    /// If the actor's mailbox is full, the message is silently
    /// dropped (the mailbox has no way to recover the value).
    pub fn send(&self, msg: M) {
        match self.mailbox.upgrade() {
            Some(q) => {
                let _ = q.push(msg);
            }
            None => {
                let _ = self.dead.push(msg);
            }
        }
    }

    /// Access the dead letters queue for inspection / draining.
    pub fn dead_letters(&self) -> &MpscQueue<M> {
        &self.dead
    }

    /// Check whether the actor is still alive.
    pub fn is_alive(&self) -> bool {
        self.mailbox.upgrade().is_some()
    }
}
