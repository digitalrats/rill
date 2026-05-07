# Actor Model (rill-core-actor)

Rill implements a lightweight actor model for lock-free message passing
between threads. The model is inspired by Akka/Pekko but specialised for
the real-time signal processing use case.

The actor model is **domain-agnostic** — `ActorRef<M>`, `ActorCell`,
`MessageDispatcher<M>`, and `ActorSystem<M>` are generic over
`M: Send + 'static`. They have no dependency on audio or signal types.
The concrete message type (`SetParameter`) and its consumer (`Graph`)
belong to higher-level crates (`rill-patchbay`, `rill-graph`), not to the
actor infrastructure itself.

## Core concepts

```
┌─────────────────────────────────────────────────────────────────┐
│                     rill-core-actor                              │
│                                                                  │
│  ┌────────────┐   ┌────────────┐   ┌────────────────────────┐   │
│  ┌────────────┐   ┌────────────┐   ┌─────────────────────┐   │
│  │ ActorCell  │   │ ActorRef   │   │  ActorSystem        │   │
│  │  (trait)   │   │  (handle)  │   │  (named mailboxes   │   │
│  └────────────┘   └────────────┘   │   + route + dead)   │   │
│       │                 │          └─────────────────────┘   │
│       │  receive()      │  send(msg)     │ route(name, msg)  │
│       │                 │               │ broadcast(msg)    │
│       ▼                 ▼               ▼                    │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │              MpscQueue (lock-free MPSC)                   │ │
│  └──────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

### `ActorCell`

Trait for types that own a mailbox and process messages from it.

```rust
pub trait ActorCell: Send + 'static {
    type Msg: Send + 'static;
    fn receive(&mut self, msg: Self::Msg);
}
```

Implemented by:
- [`Graph`](https://docs.rs/rill-graph) — processes `SetParameter` commands
  by writing to the target node's parameter storage.

### `ActorRef<M>`

Thread-safe handle for sending messages to an actor. Holds a strong reference
(`Arc`) to the actor's mailbox. Multiple `ActorRef`s can exist for the same
actor — they all share the same underlying lock-free MPSC queue.

```rust
let (actor_ref, mailbox) = ActorRef::<SetParameter>::new_pair();

// Send from any thread
actor_ref.send(SetParameter::new(...));

// The actor drains its mailbox
while let Some(msg) = mailbox.pop() {
    actor.receive(msg);
}
```

Key properties:
- **Strong reference** — `Arc<MpscQueue<M>>`, not `Weak`. The mailbox lives
  as long as the actor or any `ActorRef` keeps it alive.
- **Does not own the queue** — the actor owns the mailbox. `ActorRef` is a
  borrowed handle obtained via the actor's public API (e.g. `Graph::handle()`).
- **Bounded queue** — capacity 64 by default. Full queue silently drops messages.
- **Generic** — `ActorRef<String>`, `ActorRef<SetParameter>`, etc.

### `MessageDispatcher<M>`

Application-level "actor system" — combines an `ActorRef` with a dead letters
queue for undeliverable messages.

```rust
let mailbox = Arc::new(MpscQueue::with_capacity(64));
let dead = Arc::new(MpscQueue::new());  // unbounded

let dispatcher = MessageDispatcher::new(
    ActorRef::new(&mailbox),
    dead,
);

// Normal delivery
dispatcher.send(SetParameter::new(...));

// Route directly to dead letters (actor is known stale)
dispatcher.send_dead(SetParameter::new(...));

// Inspect dead letters
let undelivered = dispatcher.drain_dead();
```

The `MessageDispatcher` is the foundation for [`Engine`](https://docs.rs/rill-patchbay)
— the central dispatcher in the patchbay system.

### `ActorSystem<M>`

Central registry of named mailboxes with routing and dead letters support.
Designed for multiple consumers — each registered mailbox can be drained
by a dedicated thread or task.

```rust
let mut system = ActorSystem::<SetParameter>::new();

// Register named mailboxes — each gets its own consumer
let graph = system.register("graph");    // audio thread
let midi  = system.register("midi");     // tokio task (future)

// Route to a specific actor
system.route("graph", SetParameter::new(...));

// Unknown name → dead letters
system.route("unknown", SetParameter::new(...));

// Broadcast to all actors
system.broadcast(SetParameter::new(...));

// Inspect dead letters
let lost = system.drain_dead();
```

## Lifecycle

```
Runtime::new()
  │
  ├── GraphBuilder::build()
  │     └── creates mailbox (Arc<MpsqQueue>)
  │     └── Graph owns the mailbox
  │     └── Graph::handle() → ActorRef
  │
  ├── Engine::new(actor_ref)
  │     └── holds ActorRef, sends commands via MessageDispatcher
  │
  └── Graph runs signal callback
        └── drains mailbox via raw pointer (GraphHandle)
```

When `Graph` is dropped the mailbox `Arc` reference in Graph is released.
All `ActorRef`s still hold strong references — the queue stays alive but
nobody processes it. The application layer (Runtime) detects this and
routes subsequent messages to dead letters explicitly.

## Relation to Akka/Pekko

| Akka | Rill |
|------|------|
| `ActorCell` | `ActorCell` trait |
| `ActorRef` | `ActorRef<M>` |
| `ActorSystem` | `MessageDispatcher<M>` |
| `DeadLetterActorRef` | `Arc<MpscQueue<M>>` (application-level) |
| `Mailbox` | `MpscQueue<M>` |
| `context.watch()` | Application-level health check |

The main difference: Rill has a **single actor** (Graph) that processes
`SetParameter` messages. The actor system is simple by design — there is
no actor supervision, no death watch, and no message routing. Dead letters
are managed at the application level (Runtime) rather than at the ref level.

## Crate structure

```
rill-core-actor           (new crate, depends on rill-core)
├── ActorCell trait       (what processes messages)
├── ActorRef<M>           (handle to send messages)
├── MessageDispatcher<M>  (dispatcher with dead letters)
└── ActorSystem<M>        (named mailbox registry + route + broadcast)
```

The crate depends on [`rill-core`](https://docs.rs/rill-core) for
`MpscQueue` (lock-free MPSC queue) and `SetParameter` (command type).
