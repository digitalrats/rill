# Two-Thread Architecture — Audio + Control (current implementation)

## Overview

**Audio thread (hard/soft RT)** — active I/O node drives the graph via
`Node::run()` with a tick closure. **Control thread** — actors that send
`SetParameter` commands and receive `ClockTick` telemetry.
Communication via lock-free `MpscQueue` through `ActorRef`/`ActorCell`.

```
[Control Thread]                         [Audio Thread]
─────────────────────                    ────────────────────
  PortCombiner (tokio)                  Graph (ActorCell<SetParameter>)
  SequencerActor                            │
  OSC dispatch                        active_node.run(tick)
       │                                    │
       │ ActorRef::send(SetParameter)   tick closure (per block):
       ├───────────► Graph ◄───────────  │ 1. drain cmd_queue
       │                                 │ 2. process_block()
       ◄─────────── clock_tx ────────►  │ 3. Port::propagate()
                ActorRef::send            │ 4. clock_tx.send(ClockTick)
                (ClockTick)              │
                                         ▼
                                  backend.run(running)
```

## Bidirectional channels

| Direction | Type | Mailbox owner | Sender | Purpose |
|-----------|------|---------------|--------|---------|
| Control → Audio | `SetParameter` | `Graph` | Control actors via `graph.handle()` | Parameter changes |
| Audio → Control | `ClockTick` | Sequencer actor | Graph via `graph.clock_tx` | Block-level timing |

## Processing model

The tick closure (created by `Graph::run()`) handles one audio block:

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
| `Port::propagate` | Audio (hard RT) | zero-alloc, lock-free |
| `Node::run()` | Audio | Stack buffers, no syscalls |
| `MpscQueue::push` / `ActorRef::send` | Any | Lock-free |
| `MpscQueue::pop` | Consumer's thread | Lock-free |
| `ActorCell::receive` | Consumer's thread | Matches consumer's RT profile |
| Control actors | Control (soft RT) | May allocate |
