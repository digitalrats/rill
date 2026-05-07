# Two-Thread Architecture — Audio + Control (current implementation)

## Overview

Implementation of the two-thread model. **Audio thread (hard/soft RT)** — `AudioInput`/`AudioOutput`
with `Port::propagate`. **Control thread (soft RT, tokio)** — `rill-patchbay::engine::Engine`.
They communicate via `MpscQueue<ParameterCommand>`.

```
[Control Thread (soft RT)]             [Audio Thread (hard RT)]
─────────────────────────────           ─────────────────────────
  Engine                              AudioInput / AudioOutput
  ├── Automata (LFO, Env)                           │
  ├── PortCombiner                                  │
  ├── Sequencer                              Port::propagate()
  ├── Event mappings                          recursive DAG walk
  └── Sensors                                    │
       │                                 generate() / process()
       │ MpscQueue<ParameterCommand>.push()     / consume()
       ├─────────── lock-free ───────────────►  │
       │           queue                        │
       │                                    drain MpscQueue
```

## Current State

| Component | Where | Status |
|-----------|-------|--------|
| `MpscQueue<T>` (lock-free SPSC) | `rill-core::queues::mpsc` | ✅ Implemented, cross-thread bridge |
| `SpscQueue<T, CAP>` (SPSC ring) | `rill-core::queues::spsc` | ✅ Implemented, tested |
| `CommandQueue` | `rill-core::queues::command` | ✅ Implemented |
| `Telemetry` / `TelemetryQueue` | `rill-core::queues::telemetry` | ✅ Implemented |
| `MicroControlObserver` | `rill-core::queues::observer` | ✅ Implemented, tested |
| `Automaton` trait | `rill-patchbay::engine` | ✅ Implemented |
| `Servo<A: Automaton>` | `rill-patchbay::engine` | ✅ Implemented |
| Sensors (acoustic, physical) | `rill-patchbay::sensor` | ✅ Implemented |
| `Engine` | `rill-patchbay::engine` | ✅ Centralised automata/servo/mapping API |
| `Manager` | `rill-patchbay::manager` | ✅ Multi-thread manager, tested |
| `PortCombiner` | `rill-patchbay::port_combiner` | ✅ Conflict resolution (Absolute/Modulation) |
| `Engine` | `rill-patchbay::engine` | ✅ **Orchestrator** — green threads + patchbay + sequencer |
| **`SignalEngine`** | *removed* | ❌ Removed. Replaced by `Port::propagate` |
| `GraphStats` | `rill-graph::graph` | ⚠️ Defined, not used |
| `crossbeam-channel` | `rill-graph/Cargo.toml` | ⚠️ In dependencies, not used in code |

### Queue systems

Two parallel implementations:

| System | RT-safe | Used by |
|---------|---------|-------------|
| `crossbeam_channel` (bounded) | ✅ | `CommandQueue`, `MicroControlObserver` |
| `MpscQueue`/`SpscQueue` (lock-free, `AtomicCell`) | ✅ (zero deps) | Communication audio ↔ control |

## Current processing model

The audio graph **has no external engine**. Processing lives in the `AudioIo` callback:

1. Drain `MpscQueue<ParameterCommand>` (parameters from control thread)
2. `Source::generate()` / `Processor::process()` / `Sink::consume()`
3. `Port::propagate()` — recursive DAG traversal via `downstream_input_ptrs`

`Engine` lives on the control thread and spawns green threads (tokio) for:
- Automata (LFO, envelope — `tokio::spawn` + `tokio::time::interval`)
- `PortCombiner` (receiving values from automaton and UI, resolving conflicts)
- `SnapshotSequencer` (receiving `CLOCK_TICK` from audio stream)

## Action Items

1. **Remove `crossbeam-channel`** from `rill-graph/Cargo.toml` — not used
2. **Remove or use `GraphStats`** — defined but dead code
3. **Telemetry loop** — `MicroControlObserver` is ready but not connected to any processing loop

## Thread Safety Summary

| Component | Thread | Requirements |
|-----------|-------|------------|
| `Port::propagate` | Audio (hard RT) | zero-alloc, lock-free |
| `AudioInput::start()` / `AudioOutput::start()` | Audio (hard/soft RT) | Stack buffers, no syscalls |
| `MpscQueue::push` | Control (soft RT) | Lock-free |
| `MpscQueue::pop` | Audio (hard RT) | Lock-free |
| `Engine::handle_event` | Control (soft RT) | May allocate |
| `Sequencer` | Control (blocking) | spawn_blocking |
