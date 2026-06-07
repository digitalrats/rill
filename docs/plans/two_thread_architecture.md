# Two-Thread Architecture — Control + Signal (current implementation)

## Overview

**Signal thread (hard/soft RT)** — active I/O node drives the graph via
`Node::run()` with a tick closure. **Control thread** — Servo actors that send
`SetParameter` commands and receive `ClockTick` telemetry.
Communication via lock-free `MpscQueue` through `ActorRef`/`ActorCell`.

```
[Control Thread]                         [Signal Thread]
─────────────────────                    ────────────────────
  Servo (tokio actor)               Graph (ActorCell<SetParameter>)
    automaton.step()                     │
    mapping.apply()                 active_node.run(tick)
    ControlStrategy /                    │
    ConflictStrategy                tick closure (per block):
       │                            1. drain cmd_queue
       │ ActorRef::send(SetParameter)│ 2. process_block()
       ├───────────► Graph ◄─────────── 3. Port::propagate()
       │                            4. ActorRef::send(ClockTick)
       ◄─────────── ClockTick ────────  │
                ActorRef::send            │ 4. clock_tx.send(ClockTick)
                (ClockTick)              │
                                         ▼
                                  backend.run(running)
```

## Bidirectional channels

| Direction | Type | Mailbox owner | Sender | Purpose |
|-----------|------|---------------|--------|---------|
| Control → Signal | `SetParameter` | `Graph` | Servo actors via `graph_ref` | Parameter changes |
| Signal → Control | `ClockTick` | Servo actors | Graph via `actor.drain()` side effect | Block-level timing |

## Processing model

The tick closure (created by `Graph::run()`) handles one signal block:

1. Drain command queue (`SetParameter` → `set_parameter` on target nodes)
2. `Source::generate()` / `Processor::process()` / `Sink::consume()`
3. `Port::propagate()` — recursive DAG traversal via `downstream_input_ptrs`
4. Send `ClockTick` to control thread via `clock_tx`

## Backend ownership

Each I/O node owns its backend via `Box<dyn IoBackend<T>>`.
The active node's `Node::run()` sets up the process callback and blocks
on `backend.run()`.

## Thread Safety Summary

| Component | Thread | Requirements |
|-----------|-------|------------|
| `Port::propagate` | Signal (hard RT) | zero-alloc, lock-free |
| `Node::run()` | Audio | Stack buffers, no syscalls |
| `MpscQueue::push` / `ActorRef::send` | Any | Lock-free |
| `MpscQueue::pop` | Consumer's thread | Lock-free |
| `ActorCell::receive` | Consumer's thread | Matches consumer's RT profile |
| Control actors | Control (soft RT) | May allocate |
