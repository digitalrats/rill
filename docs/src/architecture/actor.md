# Actor Model (rill-core-actor)

Rill implements a lightweight actor model for lock-free message passing
between threads. The actor API is minimal: `Actor<M>`, `ActorRef<M>`, `ActorSystem`,
and three spawn strategies.

## Core types

### `Actor<M>`

Handler closure + mailbox. Drained in-place by the caller — **no separate thread**.

```rust
use rill_core_actor::{ActorSystem, ActorRef};

let system = ActorSystem::new();
let mut actor = system.spawn("echo", |msg: String| {
    println!("got: {msg}");
});
let ref_a = actor.actor_ref();
ref_a.send("hello".into());
actor.drain(); // processes "hello"
```

- Handler is `Box<dyn FnMut(M)>` — created and drained on the same thread, no `Send` requirement.
- Used by **Graph** (handler captures `Rc<UnsafeCell<...>>` → `!Send`, drained inline in audio callback).
- Used by **Rack actor** — drained in a dedicated OS thread.

### `ActorRef<M>`

Thread-safe send-only handle (`Arc<Mailbox<M>>`). Lock-free `send()`, bounded queue (capacity 64).
Silently drops messages when queue is full.

### `ActorSystem`

Registry of named actors. Three spawn methods:

| Method | Handler location | Drain | Returns | Use case |
|--------|-----------------|-------|---------|----------|
| `spawn(name, handler)` | Caller's thread | Caller (`actor.drain()`) | `Actor<M>` | Graph, inline drain |
| `spawn_detached(name, make_handler, ms)` | Inside new OS thread | Auto (`std::thread::spawn` + sleep) | `ActorRef<M>` | Rack, Servo (handler `!Send`) |
| `spawn_detached_tokio(name, make_handler, ms)` | Inside new tokio task | Auto (`tokio::spawn` + interval) | `ActorRef<M>` | Servo (handler `Send`, many actors) |

**Key design rule:** The handler closure is **always created on the thread where it will be drained**.
For `spawn`, the caller creates the handler and drains it. For `spawn_detached*`, `make_handler()`
is called inside the spawned thread/task → handler never crosses thread boundary → `Send` not required.

## RT boundary

```
Soft-RT (control thread)                    Hard-RT (audio thread)
┌──────────────────────────┐               ┌──────────────────────────┐
│ PortCombiner (tokio)     │   send()      │ Graph actor              │
│ OSC dispatch             │  ──────────►  │   drain() in callback    │
│ Sequencer automaton      │   mailbox     │   → set_parameter()      │
│                          │               │   → generate()           │
└──────────────────────────┘               └──────────────────────────┘
```

| Direction | Message | Mailbox owner | Sender |
|-----------|---------|---------------|--------|
| Control → Audio | `SetParameter` | Graph actor | Servo actors via `graph.handle()` |
| Audio → Control | `ClockTick` | Rack actor | Graph via `parent_ref.send()` |

| Method | RT-safe? | Notes |
|--------|----------|-------|
| `ActorRef::send()` | ✅ Hard RT | Lock-free, bounded queue |
| `Actor::drain()` | ⚠️ Depends on caller's thread | In audio callback = hard RT, in control = soft RT |
| `ActorSystem::route()` | ❌ Soft RT only | Heap iteration |
| `ActorSystem::broadcast()` | ❌ Soft RT only | Heap iteration + clone |
