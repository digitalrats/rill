# Two-Thread Architecture — Control + Signal (current implementation)

## Overview

**Signal thread (hard/soft RT)** — the orchestrator creates a backend,
extracts `ProcessingState` from the graph, and registers a process callback
driven by the backend. **Control thread** — Servo actors that send
`SetParameter` commands and receive `ClockTick` telemetry.
Communication via lock-free `MpscQueue` through `ActorRef`/`ActorCell`.

```
[Control Thread]                         [Signal Thread]
─────────────────────                    ────────────────────
  Servo (tokio actor)               ProcessingState
    automaton.step()                     │
    mapping.apply()                 process_callback(tick):
    ControlStrategy /                    │
    ConflictStrategy                1. actor.drain()
       │                            2. process_block(tick)
       │ ActorRef::send(SetParameter)│ 3. Port::propagate()
       ├───────────► Graph ◄─────────── 4. send_clock_tick(tick)
       │                             │
       ◄─────────── ClockTick ────────  │
                ActorRef::send              │
                (ClockTick)              backend.run(running)
```

## Bidirectional channels

| Direction | Type | Mailbox owner | Sender | Purpose |
|-----------|------|---------------|--------|---------|
| Control → Signal | `SetParameter` | `Graph` | Servo actors via `graph_ref` | Parameter changes |
| Signal → Control | `ClockTick` | Servo actors | Graph via `actor.drain()` side effect | Block-level timing |

## Processing model

The process callback (registered on the backend by the orchestrator)
handles one signal block:

1. Drain command queue (`SetParameter` → `set_parameter` on target nodes)
2. `Source::generate()` / `Processor::process()` / `Sink::consume()`
3. `Port::propagate()` — recursive DAG traversal via `downstream_input_ptrs`
4. Send `ClockTick` to control thread via `clock_tx`

## Backend ownership

The orchestrator creates a backend via `BackendFactory::create()`,
extracts `ProcessingState` from the graph, and registers a process callback.
The backend's `run()` method enters the I/O loop.

## Thread Safety Summary

| Component | Thread | Requirements |
|-----------|-------|------------|
| `ProcessingState::process_block` | Signal (hard RT) | zero-alloc, lock-free |
| `IoBackend::run` | I/O | Stack buffers, no syscalls |
| `MpscQueue::push` / `ActorRef::send` | Any | Lock-free |
| `MpscQueue::pop` | Consumer's thread | Lock-free |
| `ActorCell::receive` | Consumer's thread | Matches consumer's RT profile |
| Control actors | Control (soft RT) | May allocate |
