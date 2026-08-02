# Architecture Overview

Rill is a modular signal-processing ecosystem built around a minimal core
with traits. Each crate has a clear responsibility and can be used
independently.

## Layer diagram

```
┌─────────────────────────────────────────────────────────────┐
│  rill-osc  │  rill-graph  │  rill-patchbay  │  rill-sampler │
├─────────────────────────────────────────────────────────────┤
│  rill-core-dsp  (Algorithm trait, filters, generators, FX)  │
│  rill-digital-filters  │  rill-digital  │
│  -effects  │  rill-router  │  rill-lofi                     │
│  rill-core-model  │  rill-analog-filters  │  rill-analog      │
│  -effects  │  rill-lang  │  rill-fft                         │
├─────────────────────────────────────────────────────────────┤
│  rill-io (PortAudio / ALSA / PipeWire / JACK)               │
├─────────────────────────────────────────────────────────────┤
│  rill-telemetry                                            │
├─────────────────────────────────────────────────────────────┤
│  rill-core (traits, math, buffers, queues, time, macros)   │
│  rill-core-actor  (ActorRef, ActorSystem)       │
└─────────────────────────────────────────────────────────────┘
```

## Key concepts

### Signal graph (DAG)

Rill's processing model is a **static directed acyclic graph (DAG)**:

- **Nodes** — processing units added by type name (a flat `add_node` API, no
  separate source/processor/sink distinction at builder level)
- **Edge kinds** — `Signal` (forward flow, topologically sorted), `Control`
  (modulation), `Clock` (timing), `Feedback` (excluded from sort)
- **Connections** — wired as `(from_node, from_port, to_node, to_port)` tuples

Graph topology is fixed at construction time via `GraphBuilder::build_ir()`.
This produces a `GraphIr` (rill-lang's multi-node intermediate representation),
which `rill_lang::graph_compiler::compile()` transforms into a
`CompiledGraphEngine`. Processing is driven by `CompiledGraphEngine` — a flat vector of compiled
closures over a `FixedBuffer` pool, executed in topological order with
zero heap allocation on the signal path.

### Two-thread architecture

- **Signal thread** (hard or soft RT) — runs the process callback:
  `CompiledGraphEngine::process()`. Zero heap allocs, no locks, no syscalls.
- **Control thread** (tokio green threads) — runs `Patchbay` with
  automatons (LFO, envelopes, sequencers). Communicates with the signal
  thread via the graph actor mailbox (`ActorRef<CommandEnum>`).

See [Signal graph (rill-graph)](../architecture/graph.md) for details.

### Processing models

| Direction | Active side | Node type |
|-----------|------------|-----------|
| **Output** | Playback | Engine writes output buffers from `MultichannelAlgorithm::process()` |
| **Input** | Capture | Engine reads input buffers into `CompiledGraphEngine::process()` |

### Execution model

The signal graph has no external engine loop. `CompiledGraphEngine::process_tick()`
drives execution:

1. Drain the actor mailbox — apply queued `SetParameter` commands
2. Execute nodes in topological order via `NodeClosure::execute()`
3. Each node reads from its input buffers in the pool, runs its algorithm,
   writes to its output buffers
4. `CompiledGraphEngine` implements both `Algorithm<T>` (SISO) and
   `MultichannelAlgorithm<T>` (MIMO)

### Automation (The World of Automatons)

`rill-patchbay` provides generative control signals through **automatons** —
LFOs, envelopes, sequencers that run on the control thread. **Sensors**
(MIDI, OSC) decode external input into `ControlEvent`s and feed them
into the automaton world through mapping-only servos. Automatons
connect to graph node parameters through **servos** with configurable
mapping strategies (linear, exponential, logarithmic).

See [The World of Automatons](../guides/world-of-automatons.md) for details.

## Design principles

1. **Domain-agnostic core** — `Scalar`, `Vector`, lock-free queues work
   in any signal domain (embedded, IoT, robotics)
2. **Minimal dependencies** — each crate depends only on what it uses
3. **Zero-cost abstractions** — static dispatch, const generics, SIMD-ready vectors
4. **Real-time safety** — no allocation, no locks, no syscalls on the signal path
5. **Single-threaded DAG** — the signal graph is a single-owner tree,
   no atomics or mutexes in the hot path
