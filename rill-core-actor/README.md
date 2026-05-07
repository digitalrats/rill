# rill-core-actor

Minimal, domain-agnostic actor model for lock-free message passing.

## Philosophy

This is not Pekko, not Erlang/OTP. No supervision, clustering,
persistence, streaming, or Kafka connectors.

Four types, two of which are optional:

| Type | Purpose | Required? |
|---|---|---|
| `ActorRef<M>` | Thread-safe handle for sending messages | yes |
| `ActorCell` | Trait: "I can receive and process messages" | yes |
| `MessageDispatcher<M>` | ActorRef + dead letters | no |
| `ActorSystem<M>` | Named mailbox registry with routing | no |

Everything else lives upstream (rill-patchbay, Runtime with its lifecycle).

## Usage

```rust
use rill_core_actor::ActorRef;

let (ar, mailbox) = ActorRef::<String>::new_pair();
ar.send("hello".into());
assert_eq!(mailbox.pop(), Some("hello".into()));
```

## RT-safe?

`send()` — yes (lock-free, bounded queue).  
`receive()` — depends on the calling thread (the actor decides).  
`route()` / `broadcast()` — soft-RT only.

## How not to grow into Pekko

Rule: if new functionality needs more than 10 lines in
`ActorRef` or `ActorSystem`, it probably does not belong here.
Move it upstream (rill-patchbay, rill-adrift).
