# Backend Extraction from Graph Nodes

> **Status:** Design approved, awaiting implementation plan.
> **Date:** 2026-06-14

## Motivation

Three problems with the current callback-driven model where `Box<dyn IoBackend>` lives inside Input/Output nodes:

1. **"Callback hell" / full-duplex mode combinations.** One hardware device used for both input and output leads to complex callback interactions when `Input::generate()` calls `backend.read()` and `Output::consume()` calls `backend.write()` inside the same tick closure.

2. **DMA buffer isolation.** DMA buffers from the hardware stack should be passed by reference (zero-copy) and copied into internal `FixedBuffer` inside the graph thread, not directly accessed by graph nodes.

3. **Thread separation.** I/O DMA management and graph processing should be separate concerns. The I/O thread does DMA ↔ ring buffer synchronization; the graph processes from ring buffers via `BufferView`.

## Architecture Overview

```
┌──────────────────────────────────────────────────────────────────┐
│  Orchestrator (ModularSystem / RackCase)                         │
│                                                                   │
│  1. Creates backend (outside graph)                               │
│  2. Obtains IoRingBuffers + BufferView from backend               │
│  3. Passes BufferView to Input/Output nodes at graph build time   │
│  4. Registers process_callback on backend:                        │
│                                                                   │
│     backend.set_process_callback(|| {                             │
│         graph_actor.drain();          // apply SetParameter       │
│         graph.process_block(&tick);   // full DAG traversal        │
│         rack_actor.send(ClockTick);   // notify modules           │
│     });                                                            │
│                                                                   │
│  5. backend.run(running)  — blocks I/O thread                     │
└──────────────────────────────────────────────────────────────────┘

         ┌──────────────────────┐
         │   Backend (I/O thread)│
         │                       │
         │  DMA → input_ring     │
         │  process_callback()───┼─────────┐
         │  output_ring → DMA    │         │
         └──────────────────────┘         │
                 │                        ▼
         ┌───────┴────────┐   ┌─────────────────────────┐
         │  IoRingBuffers  │   │  Graph (I/O thread,      │
         │  ┌────────────┐ │   │   synchronous in callback)│
         │  │ input_ring │─┼───→ Input::generate(tick)     │
         │  │ output_ring│←┼─── Output::consume(tick)      │
         │  └────────────┘ │   │                          │
         └────────────────┘   │  port.propagate()         │
                              │  rack_actor.send(         │
                              │    CommandEnum::ClockTick)│
                              └─────────────┬────────────┘
                                            │ async (MpscQueue)
                              ┌─────────────▼────────────┐
                              │  Rack actor (detached)    │
                              │  → Servo actors (detached)│
                              │    → graph_actor.send(    │
                              │        SetParameter) ───┐ │
                              └─────────────────────────┼─┘
                                                         │
                                  on NEXT tick ──────────┘
                                  graph_actor.drain()
```

**Key decisions:**
- Graph remains single-threaded on the I/O thread, processed synchronously in the backend callback. No separate graph thread.
- Backend is NOT inside nodes — created by the orchestrator, owns DMA and IoRingBuffers.
- Data flows through `IoRingBuffer` (lock-free SPSC). Nodes read/write via `BufferView`.
- `ClockTick` carries `BufferView`. Only `Source` and `Sink` have access to it.
- `Rack actor` — separate thread, receives `ClockTick` asynchronously, fan-out to modules.

## Key Abstractions

### IoRingBuffers

Generalizes current `PwBuffers` for all backends.

```rust
// rill-core/src/buffer/
pub struct IoRingBuffers {
    pub input: Arc<IoRingBuffer>,    // backend writes, graph reads
    pub output: Arc<IoRingBuffer>,   // graph writes, backend reads
    pub num_input_channels: usize,
    pub num_output_channels: usize,
}
```

### BufferView trait

Backend-specific accessor for `IoRingBuffers`. Encapsulates interleave/deinterleave rules.

```rust
// rill-core/src/traits/
pub trait BufferView: Send + Sync {
    fn num_input_channels(&self) -> usize;
    fn num_output_channels(&self) -> usize;
    fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize;
    fn write_output(&self, channel: usize, src: &[f32]) -> usize;
}
```

| Backend | `read_input` | `write_output` |
|---------|-------------|----------------|
| PipeWire | deinterleave from `input_ring` | interleave to `output_ring` |
| PortAudio | deinterleave from `input_ring` | interleave to `output_ring` |
| JACK | deinterleave from `input_ring` | interleave to `output_ring` |
| ALSA | deinterleave from `input_ring` | interleave to `output_ring` |
| Null | fill zeros | no-op |

### IoBackend trait (new)

```rust
// rill-core/src/io.rs
pub trait IoBackend: Send {
    fn ring_buffers(&self) -> &IoRingBuffers;
    fn create_view(&self) -> Arc<dyn BufferView>;
    fn set_process_callback(&self, cb: Box<dyn Fn() + Send>);
    fn run(&self, running: Arc<AtomicBool>) -> IoResult<()>;
    fn stop(&self) -> IoResult<()>;
    fn as_control(&self) -> Option<&dyn IoControl> { None }
}
```

`read()`/`write()` removed — this is now internal to each backend's `run()` loop.

### ClockTick extension

```rust
pub struct ClockTick {
    pub sample_pos: u64,
    pub samples_since_last: u32,
    pub is_new_block: bool,
    pub sample_rate: f32,
    pub tempo: Option<f32>,
    // New fields:
    pub source: String,
    pub view: Arc<dyn BufferView>,
}
```

### Input / Output nodes (without backend)

```rust
// rill-io/src/input.rs
pub struct Input<T, const BUF_SIZE: usize> {
    // ... id, metadata, ports, state
    // Removed: backend: Option<Box<dyn IoBackend<T>>>
    // Removed: bufs: Vec<[T; BUF_SIZE]>
}
```

## Signal Flow Per Tick

```
I/O thread (backend.run blocks):

  ┌─ Backend internal loop ──────────────────────────────────────┐
  │                                                               │
  │ 1. DMA → input_ring::write()          // backend-specific    │
  │ 2. output_ring::read() → DMA          // (previous tick)     │
  │                                                               │
  │ 3. process_callback() ─────────────────────────────────────┐ │
  │    │                                                        │ │
  │    ▼                                                        │ │
  │ ┌──────────────────────────────────────────────────────────┐ │ │
  │ │ Graph::process_block(&mut self, tick: &ClockTick)        │ │ │
  │ │                                                          │ │ │
  │ │  actor.drain()                 ← SetParameter from Servo │ │ │
  │ │                                                          │ │ │
  │ │  source.process_block(ctx, tick) →                      │ │ │
  │ │    Input::generate(ctx, tick) →                          │ │ │
  │ │      tick.view.read_input(ch, buf)   ← input_ring       │ │ │
  │ │                                                          │ │ │
  │ │  port.propagate(buf, ctx) →   recursive DAG              │ │ │
  │ │    Processor::process(ctx) →  DSP                        │ │ │
  │ │    Router::route(ctx) →       routing                    │ │ │
  │ │    Sink::consume(ctx, tick) →                             │ │ │
  │ │      Output::consume(ctx, tick) →                         │ │ │
  │ │        tick.view.write_output(ch, buf) → output_ring     │ │ │
  │ │                                                          │ │ │
  │ │  rack_actor.send(CommandEnum::ClockTick(tick.clone()))   │ │ │
  │ └──────────────────────────────────────────────────────────┘ │ │
  │                                                               │
  └───────────────────────────────────────────────────────────────┘
                                 │ async (MpscQueue)
                                 ▼
┌─ Rack actor (detached) ──────────────────────────────────────┐
│  handler: for each module in modules:                        │
│    module.send(CommandEnum::ClockTick(tick.clone()))         │
└──────────────────────────┬───────────────────────────────────┘
                           │ async (MpscQueue)
                           ▼
┌─ Servo actor (detached) ─────────────────────────────────────┐
│  handler: ClockTick → automaton.step() →                     │
│    graph_actor.send(CommandEnum::SetParameter(...))          │
└──────────────────────────┬───────────────────────────────────┘
                           │ arrives on NEXT tick
                           ▼
          graph_actor.drain()  (step 1 of next tick)
```

## Signature Changes

| Trait | Before | After |
|-------|--------|-------|
| `Processable::process_block` | `(&mut self, ctx)` | `(&mut self, ctx, tick)` |
| `Source::generate` | `(&mut self, ctx)` | `(&mut self, ctx, tick)` |
| `Sink::consume` | `(&mut self, ctx)` | `(&mut self, ctx, tick)` |
| `Processor::process` | `(&mut self, ctx)` | unchanged |
| `Router::route` | `(&mut self, ctx)` | unchanged |
| `Port::propagate` | `(&self, buf, ctx)` | unchanged |

Only Source and Sink receive `tick.view`. Processor/Router operate exclusively on internal `FixedBuffer` as before.

## Removals

| Removed | Location | Reason |
|---------|----------|--------|
| `IoBackend::read()` | `rill-core/src/io.rs` | Nodes don't call read — data via ring |
| `IoBackend::write()` | `rill-core/src/io.rs` | Nodes don't call write — data via ring |
| `ActiveNode::run()` | `rill-core/src/traits/node.rs` | Backend launch moves to orchestrator |
| `Box<dyn IoBackend>` from Input/Output | `rill-io/src/input.rs`, `output.rs` | Replaced by `tick.view` |
| `bufs: Vec<[T; BUF_SIZE]>` in Input | `rill-io/src/input.rs` | Scratch buffers not needed |
| `BackendFactory` from GraphBuilder | `rill-graph/src/graph.rs` | Backend created by orchestrator |
| `PassiveRef<T>` | `rill-graph/src/graph.rs` | No shared backend between nodes |
| `GraphConstructor` | `rill-graph/src/graph_constructor.rs` | Orchestrator does this explicitly |

## Untouched

`FixedBuffer`, `Port`, `Port::propagate`, `Processor`, `Router`, `ActorSystem`, `Servo`, `Automaton`, `ModuleFactory`, `NodeFactory` (except Input/Output constructors), all DSP crates, `IoRingBuffer`.

## Migration Phases

1. **Additive** — new types/traits, no breakage: `BufferView`, `IoRingBuffers`, `ClockTick` fields, new `IoBackend` signature.
2. **Backends** — each backend implements `BufferView` and updated `IoBackend`.
3. **Graph** — Input/Output use `tick.view`, `process_block` receives `tick`.
4. **Orchestrator** — `ModularSystem` creates backend, sets callback, passes view.
5. **Cleanup** — remove `ActiveNode::run()`, `PassiveRef`, `GraphConstructor`, old `read()`/`write()`.
