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
