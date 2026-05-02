# Two-Thread Architecture — Audio Engine + Control World

## Overview

Реализация двухпоточной модели поверх иммутабельного `SignalGraph`.
Звуковой поток (hard RT) и поток мира автоматов (soft RT) общаются
через неблокирующие очереди.

```
[Control Thread (soft RT)]             [Audio Thread (hard RT)]
─────────────────────────────           ─────────────────────────
  Automata (LFO, Env)                        SignalEngine
  Sensors (анализаторы)                        │
  Servos (приводы)                      Processing Loop:
       │                                 for idx in topo_order:
       │                                     pre_process
       │ CommandQueue.send()                 process_block
       ├─────────── неблокирующая ─────────►  snapshot_feedback
       │              очередь                 propagate
       │                                    │
       ◄─────────── неблокирующая ─────────┤
       │    TelemetryQueue.recv()           TelemetryQueue.send()
       │
  Sensor.update(value)
```

## Current State (verified against code)

| Component | Where | Status |
|-----------|-------|--------|
| `CommandQueue<T>` with crossbeam | `rill-core::queues::command` | ✅ Implemented, tested |
| `RtQueue<T>` (lock-free) | `rill-core::queues::rt_queue` | ✅ Implemented, wraps `SpscQueue`/`MpscQueue` |
| `SpscQueue<T, CAP>` (SPSC ring buffer) | `rill-core::queues::spsc` | ✅ Implemented, tested, uses `AtomicCell` |
| `MpscQueue<T>` (MPSC) | `rill-core::queues::mpsc` | ✅ Implemented |
| `QueueError` (thiserror) | `rill-core::queues::error` | ✅ Single source of truth |
| `RtQueueBase` trait | `rill-core::queues::mod` | ✅ Defines push/pop interface for lock-free queues |
| `OverflowPolicy`, `UnderflowPolicy` | `rill-core::queues::mod` | ✅ Defined |
| `QueueStats`, `QueueStatsSnapshot` | `rill-core::queues::mod` | ✅ Implemented, tested |
| `Telemetry` enum + `TelemetryQueue` | `rill-core::queues::telemetry` | ✅ Implemented (alias + ext trait) |
| `MicroControlObserver` | `rill-core::queues::observer` | ✅ Implemented, tested |
| `SignalSource`, `CommandEnum` | `rill-core::queues::signal` | ✅ Implemented |
| `Automaton` trait | `rill-patchbay::control` | ✅ Implemented |
| `Servo<A: Automaton>` | `rill-patchbay::control` | ✅ Implemented |
| Sensors (acoustic, physical) | `rill-patchbay::sensor` | ✅ Implemented |
| `PatchbayManager` | `rill-patchbay::manager` | ✅ Implemented, tested |
| `GraphStats` | `rill-graph::graph` | ✅ Declared, unused |
| **`SignalEngine` (two-thread)** | `rill-graph::engine` | ✅ Implemented — `process_tick()`, `process_block()`, `spawn()` |
| **ControlWorld / integration glue** | app-level | ❌ Not implemented |
| **`crossbeam-channel` + `parking_lot`** used in graph | `rill-graph/Cargo.toml` | ⚠️ Declared, unused |

### Important: Two Parallel Queue Systems

The codebase has two independent queue implementations — they are **not** layered:

| System | Implementation | RT-safe | Used by |
|--------|---------------|---------|---------|
| **Crossbeam** (`CommandQueue`) | `crossbeam_channel::bounded` | ✅ yes | `MicroControlObserver`, app-level wiring |
| **Lock-free** (`RtQueue`/`SpscQueue`/`MpscQueue`) | `AtomicCell` ring buffer | ✅ yes (no deps) | No external consumers yet |

`CommandQueue` uses crossbeam directly, **not** `RtQueue`. The lock-free queues (`SpscQueue`, `RtQueue`) exist as a zero-dependency alternative but are currently unused outside of their own tests.

### Phase 1.2 Compilation Fixes

The plan previously referenced `RtQueue` compilation bugs (`QueueStats` → `QueueStatsSnapshot`, import paths). These are **already fixed** — `SpscQueue` compiles and passes all tests (`test_spsc_basic`, `test_spsc_overwrite_policy`, `test_spsc_wraparound`, etc.).

## Current Implementation

The `SignalEngine` is implemented in `rill-graph::engine`:

```
SignalEngine<T, BUF_SIZE>
├── process_tick(&mut self, tick)     — clock boundary (drain + anti-ack + pre_process + apply)
├── process_block(&mut self, tick)    — full cycle (topo-order process + snapshot + propagate)
├── spawn(self) -> JoinHandle         — consumes engine, runs in dedicated thread
├── running_flag() -> Arc<AtomicBool> — for cooperative shutdown
├── nodes() / nodes_mut()             — for external topo-order iteration
├── topo_order() -> &[usize]
└── attach_command_rx / attach_telemetry_tx
```

Push model (Source active) and pull model (Sink active) are both supported.

## What Still Needs Implementation

### 2. Integration wiring

The top-level application code that wires `PatchbayManager` → `CommandQueue` → `SignalEngine` → `TelemetryQueue` → `PatchbayManager` does not exist. This lives at the application level (e.g. in `drift` or a test harness), not in any library crate.

### 3. Telemetry emission from processing loop

Per-node peak value emission, parameter change notifications, and processing time statistics are not wired into any processing loop. The `MicroControlObserver` is ready but nothing calls it yet.

## Action Items (in priority order)

1. ✅ **`SignalEngine`** — implemented in `rill-graph::engine`
2. **Wire `crossbeam-channel` in `rill-graph`** — currently declared but unused in graph.rs
3. **Connect `GraphStats`** to the actual processing loop
4. **Integrate `PatchbayManager` with `SignalEngine`** via queues (app-level demo)
5. **Remove unused dependencies** from `rill-graph/Cargo.toml` (crossbeam-channel, parking_lot)

## Thread Safety Summary

| Component | Thread | Requirements |
|-----------|--------|--------------|
| `SignalEngine::process_block` | Audio (hard RT) | Topo-order propagate (O(n) where n = connections) |
| `SignalEngine::start/stop` | Any | Atomic flags |
| `CommandQueue::send` | Control (soft RT) | Lock-free push |
| `CommandQueue::try_recv` | Audio (hard RT) | Lock-free pop |
| `TelemetryQueue::send` | Audio (hard RT) | Lock-free push |
| `TelemetryQueue::try_recv` | Control (soft RT) | Lock-free pop |
| `PatchbayManager::tick` | Control (soft RT) | May block, may allocate |
