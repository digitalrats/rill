# Two-Thread Architecture — Control + Signal (current implementation)

## Overview

**Signal thread (hard/soft RT)** — the orchestrator creates a backend,
extracts `ProcessingState` from the graph, and registers a process callback
driven by the backend. **Control thread** — Servo actors that send
`SetParameter` commands and receive `ClockTick` telemetry.
Communication via the graph actor mailbox (`Mailbox<CommandEnum>` through `ActorRef<CommandEnum>`).

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
| Control → Signal | `CommandEnum::SetParameter` | Graph actor mailbox | Servo actor via `ActorRef<CommandEnum>` | Parameter changes |
| Signal → Control | `CommandEnum::ClockTick` | Servo actor mailbox | Graph via `ProcessingState::send_clock_tick()` | Block-level timing |

## Processing model

The process callback (registered on the backend by the orchestrator)
handles one signal block:

1. `actor.drain()` — applies queued `CommandEnum::SetParameter` commands
2. `Source::generate()` / `Processor::process()` / `Sink::consume()`
3. `Port::propagate()` — recursive DAG traversal via `downstream_input_ptrs`
4. Send `CommandEnum::ClockTick` to control rack via `send_clock_tick()`

## Backend ownership

The orchestrator creates a backend via `BackendFactory::create()`,
extracts `ProcessingState` from the graph, and registers a process callback.
The backend's `run()` method enters the I/O loop.

## Thread Safety Summary

| Component | Thread | Requirements |
|-----------|-------|------------|
| `ProcessingState::process_block` | Signal (hard RT) | zero-alloc, lock-free |
| `IoDriver::run` | I/O | Stack buffers, no syscalls |
| `ActorRef<CommandEnum>::send` | Any | Lock-free (mailbox) |
| `Mailbox<CommandEnum>::drain` | Consumer's thread | Lock-free |
| `Actor<CommandEnum>::handler` | Consumer's thread | Matches consumer's RT profile |
| Control actors | Control (soft RT) | May allocate |
