# Actor Model (rill-core-actor)

Rill implements a lightweight actor model for lock-free message passing
between threads.

The actor model is **domain-agnostic** — `ActorRef<M>`, `ActorCell`,
`MessageDispatcher<M>`, and `ActorSystem<M>` are generic over
`M: Send + 'static`. They have no dependency on audio or signal types.

## RT boundary

The mailbox marks the **exact boundary** between soft-RT and hard-RT:

```
Soft-RT (any thread)                         Hard-RT (actor's thread)
┌──────────────────────────┐                 ┌────────────────────────┐
│ PortCombiner (tokio)     │    send()       │ Graph::receive()      │
│ OSC dispatch             │  ────────────→  │   nodes[idx].set()    │
│ Sequencer                │    mailbox      │   (no alloc, no lock) │
│ ANY producer             │                 │                        │
└──────────────────────────┘                 └────────────────────────┘
         ▲ soft-RT allowed                          ▲ hard-RT only
         │ heap, locks, IO                          │ alloc-free, lock-free
```

### Bidirectional pattern

Actors communicate through **two unidirectional channels**:

```
┌─────────────────────────────────────────────────────────┐
│                     Audio thread                         │
│                                                          │
│   Graph (ActorCell<SetParameter>)                        │
│   ├── mailbox → incoming SetParameter commands          │
│   └── clock_tx: ActorRef<ClockTick> → outgoing ticks    │
│                                                          │
└──────────────┬──────────────────────┬───────────────────┘
               │ send(SetParameter)    │ send(ClockTick)
               ▼                       ▼
┌──────────────────────┐  ┌────────────────────────────────┐
│  Control actors      │  │  SequencerActor                │
│  (PortCombiner, OSC) │  │  (ActorCell<ClockTick>)        │
│   hold graph.handle()│  │  receives ticks,               │
│   send parameters    │  │  sends SetParameter back       │
└──────────────────────┘  └────────────────────────────────┘
```

| Direction | Message type | Mailbox owner | Sender |
|-----------|-------------|---------------|--------|
| Control → Audio | `SetParameter` | `Graph` (audio thread) | Control actors via `graph.handle()` |
| Audio → Control | `ClockTick` | Sequencer (control thread) | Graph via `graph.clock_tx` |

### Who guarantees RT safety?

**The actor implementation, not the framework.** `rill-core-actor` provides:

| Method | RT-safe? | Notes |
|--------|----------|-------|
| `ActorRef::send()` | ✅ Hard RT | Lock-free, bounded queue (cap 64) |
| `ActorCell::receive()` | ⚠️ Depends on actor's thread | Called by the consumer — shares its RT profile |
| `ActorSystem::route()` | ❌ Soft RT only | Heap iteration |
| `ActorSystem::broadcast()` | ❌ Soft RT only | Heap iteration + clone |

## Core concepts

### `ActorCell`

Trait for types that own a mailbox and process messages from it.

```rust
pub trait ActorCell: 'static {
    type Msg: Send + 'static;
    fn receive(&mut self, msg: Self::Msg);
}
```

Implementations:
- [`Graph`](https://docs.rs/rill-graph) (`Msg = SetParameter`) — processes parameter
  commands by writing to the target node
- Sequencer actors (`Msg = ClockTick`) — receive clock ticks, compute patterns,
  send control commands back to the graph

### `ActorRef<M>`

Thread-safe handle for sending messages to an actor. Holds a strong reference
(`Arc`) to the actor's mailbox.

```rust
let (actor_ref, mailbox) = ActorRef::<ClockTick>::new_pair();
actor_ref.send(ClockTick::new(0, 64, 44100.0));
while let Some(msg) = mailbox.pop() {
    actor.receive(msg);
}
```

Key properties:
- **Strong reference** — `Arc<MpscQueue<M>>`, not `Weak`
- **Bounded queue** — capacity 64. Full queue silently drops messages
- **Lock-free** — `send()` safe from any thread including RT callbacks

## Relation to Akka/Pekko

| Akka | Rill | RT-safe? |
|------|------|----------|
| `ActorCell` | `ActorCell` trait | ⚠️ depends on consumer |
| `ActorRef` | `ActorRef<M>` | ✅ `send()` hard-RT |
| `ActorSystem` | `MessageDispatcher<M>` / `ActorSystem<M>` | ❌ soft-RT only |
| `Mailbox` | `MpscQueue<M>` | ✅ lock-free, bounded |

## Crate structure

```
rill-core-actor           (depends on rill-core)
├── ActorCell trait       (what processes messages)
├── ActorRef<M>           (handle to send messages)
├── MessageDispatcher<M>  (dispatcher with dead letters)
└── ActorSystem<M>        (named mailbox registry)
```
