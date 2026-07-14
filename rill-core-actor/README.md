# rill-core-actor

Minimal, domain-agnostic actor model for lock-free message passing.

## Philosophy

This is not Pekko, not Erlang/OTP. No supervision, clustering,
persistence, streaming, or Kafka connectors.

Four types, two of which are optional:

| Type | Purpose | Required? |
|---|---|---|
| `ActorRef<M>` | Thread-safe handle for sending messages | yes |
| `Actor<M>` | Handler + mailbox — drained inline | yes |
| `Mailbox<M>` | Lock-free SPSC queue backing an actor | no |
| `ActorSystem` | Named actor registry with routing, dead letters, spawn | no |

Everything else lives upstream (rill-patchbay, Runtime with its lifecycle).

## Usage

```rust
use rill_core_actor::{ActorRef, ActorSystem};

let system = ActorSystem::new();
let mut actor = system.spawn("hello", |msg: String| {
    println!("received: {}", msg);
});

let ref_a = actor.actor_ref();
ref_a.send("hello world".into());
actor.drain();
```

## RT-safe?

`send()` — yes (lock-free, bounded queue).  
`receive()` — depends on the calling thread (the actor decides).  
`route()` / `broadcast()` — soft-RT only.

## How not to grow into Pekko

Rule: if new functionality needs more than 10 lines in
`ActorRef` or `ActorSystem`, it probably does not belong here.
Move it upstream (rill-patchbay, rill-adrift).
