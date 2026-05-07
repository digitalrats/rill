# Signal graph (rill-graph)

Static DAG signal graph — topology and port connections only.
Processing is driven by `Port::propagate` (not an external engine).

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    GraphBuilder                      │
│  add_source() → idx  add_processor() → idx          │
│  add_sink() → idx    connect_signal(from, to)       │
│  connect_feedback(from, to)    build() → Graph │
└──────────────────────┬──────────────────────────────┘
                       │ consume
                       ▼
┌─────────────────────────────────────────────────────┐
│                    Graph                        │
│  ┌────────┐   ┌────────────┐   ┌────────┐          │
│  │ Source │──►│ Processor  │──►│  Sink  │  ...      │
│  └────────┘   └────────────┘   └────────┘          │
│                                                     │
│  read-only: topo_order(), node_count(), etc.        │
│  NO modification or process() methods               │
└─────────────────────────────────────────────────────┘
                       │ external processing loop
                       ▼
┌─────────────────────────────────────────────────────┐
│              Port-level processing                   │
│  pre_process(tick) → snapshot_feedback() →           │
│  node.process_block() → propagate(tick)              │
└─────────────────────────────────────────────────────┘
```

## Processing model: Port::propagate

No `SignalEngine`. The source node (e.g. `AudioInput` from `rill-io`)
creates its own processing callback. The callback:

1. Drain `MpscQueue<ParameterCommand>` into graph nodes
2. Call `Source::generate()` — fills output buffers
3. Call `Port::propagate()` — recursive DAG traversal:
   - Copy data to downstream input ports (zero-copy for 1:1 via `upstream_buffer`)
   - Call the downstream node's `process_block` (`generate`/`process`/`consume`)
   - Recurse through output ports' `downstream_input_ptrs`

```rust
for &idx in graph.topo_order() {
    let node = &mut graph.nodes[idx];
    // 1. pre_process — mix feedback into input buffers
    for port in &mut node.input_ports { port.pre_process(&tick); }
    // 2. Block processing
    node.process_block(&tick, &inputs, &mut outputs)?;
    // 3. snapshot_feedback — save state for next block
    for port in &mut node.output_ports { port.snapshot_feedback(); }
    // 4. propagate — route output buffers to downstream inputs
    for port in &node.output_ports { port.propagate(&tick, &mut nodes); }
}
```

## Port types

| Port type | Description | Examples |
|-----------|-------------|----------|
| **Signal** | Audio-rate data (fixed-size blocks) | Source output, processor output |
| **Control** | Control signals (one value per block) | LFO, envelope, analyser output |
| **Clock** | Timing signals for synchronisation | ALSA sync, internal timer |
| **Feedback** | State storage between blocks | Delay lines, filter states |
| **Param** | Node configuration (not signals) | Cutoff frequency, gain |

## Key components

| Component | Purpose |
|-----------|---------|
| `GraphBuilder` | Mutable builder: adds nodes and connections, produces `Graph` |
| `Graph` | Immutable DAG container, no processing methods |
| `Port` | Owns buffer, downstream routes, and feedback state |
| `BuildError` | Errors during graph construction (e.g. cycle detection) |

## Graph configurations

### Linear chain (most common)
```
[Source] → [Processor] → [Processor] → [Sink]
```

### Parallel processing (split)
```
        ┌→ [Processor A] ─┐
[Source]┤                 ├→ [Mixer] → [Sink]
        └→ [Processor B] ─┘
```

### Feedback loop
```
[Source] → [Processor] → [Delay] → [Sink]
    ↑                        │
    └───────[feedback]───────┘
```

## Zero-copy routing

- **1:1 and fan-out** — no copy, reads directly from upstream buffer
- **Fan-in and feedback** — copy required (accumulation / state storage)
- **SIMD-friendly** — fixed buffer position in memory for graph lifetime

## Hard-RT safety

- No heap allocations in the signal path
- No locks or syscalls
- All data structures pre-allocated at graph construction time
- Communication with control thread exclusively through lock-free `MpscQueue`

## Usage

```rust
use rill_graph::prelude::*;
use rill_core::traits::*;

const BUF_SIZE: usize = 64;

let mut builder = GraphBuilder::<f32, BUF_SIZE>::new();
let src = builder.add_source(Box::new(MySource::new(440.0, 44100.0)));
let proc = builder.add_processor(Box::new(MyProcessor::new(44100.0)));
let sink = builder.add_sink(Box::new(MySink::new(44100.0)));

builder.connect_signal(src, 0, proc, 0);
builder.connect_signal(proc, 0, sink, 0);

let graph = builder.build()?;
```

## Integration

- `rill-core` — `Node`, `Source`/`Processor`/`Sink` traits, `ClockTick`
- `rill-io` — `AudioInput`/`AudioOutput` nodes that drive the graph
- `rill-patchbay` — automation via `MpscQueue<ParameterCommand>`
