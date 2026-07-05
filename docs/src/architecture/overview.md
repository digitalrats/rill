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
│  rill-oscillators  │  rill-digital-filters  │  rill-digital  │
│  -effects  │  rill-router  │  rill-lofi                     │
│  rill-core-model  │  rill-analog-filters  │  rill-analog      │
│  -effects                                                  │
├─────────────────────────────────────────────────────────────┤
│  rill-io (PortAudio / ALSA / PipeWire / JACK)                    │
├─────────────────────────────────────────────────────────────┤
│  rill-core (traits, math, buffers, queues, time, macros)   │
└─────────────────────────────────────────────────────────────┘
```

## Key concepts

### Signal graph (DAG)

Rill's processing model is a **static directed acyclic graph (DAG)**:

- **Nodes** — processing units: `Source` (generates), `Processor` (transforms),
  `Sink` (consumes)
- **Ports** — typed connection points: `Signal`, `Control`, `Clock`, `Feedback`
- **Edges** — zero-copy routes between output and input ports

Graph topology is fixed at construction time via `GraphBuilder`. Processing
is driven by `Port::propagate()` — a recursive traversal that starts at
a source node and cascades through the DAG.

### Two-thread architecture

- **Signal thread** (hard or soft RT) — runs the process callback:
  `generate()` → `propagate()` → `consume()`. No heap allocs, no locks,
  no syscalls.
- **Control thread** (tokio green threads) — runs `Patchbay` with
  automatons (LFO, envelopes, sequencers). Communicates with the signal
  thread via lock‑free `MpscQueue` (the graph actor mailbox, `ActorRef<CommandEnum>`).

See [Signal graph (rill-graph)](../architecture/graph.md) for details.

### Processing models

| Direction | Active side | Node type |
|-----------|------------|-----------|
| **Output** | Playback | Sink writes to `IoPlayback` |
| **Input** | Capture | Source reads from `IoCapture` |

### Port-based propagation

The signal graph has no external engine loop. Each `Port` owns its buffer,
downstream connections, and feedback state. Processing is driven by
`ProcessingState::process_block()`:

1. Drain the actor mailbox — apply queued `SetParameter` commands
2. `Source::generate()` fills output buffers (reads from `IoCapture` for Input nodes)
3. `Port::propagate()` copies data to downstream input ports (zero-copy for 1:1 fan-out)
4. Each downstream node runs `Processor::process()` or `Sink::consume()`
5. Recursion continues through the DAG until all sinks are reached
6. `send_clock_tick()` dispatches timing to the control rack

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
   outside audio (embedded, IoT, robotics)
2. **Minimal dependencies** — each crate depends only on what it uses
3. **Zero-cost abstractions** — static dispatch, const generics, SIMD-ready vectors
4. **Real-time safety** — no allocation, no locks, no syscalls on the signal path
5. **Single-threaded DAG** — the signal graph is a single-owner tree,
   no atomics or mutexes in the hot path
