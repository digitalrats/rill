# Signal graph (rill-graph)

Static DAG signal graph — topology and port connections only.
Processing is driven by `Port::propagate`.

> ⚠️ **Deprecated**: `ProcessingState` and `Port::propagate` are the legacy execution engine.
> The recommended path is `GraphBuilder::build_ir()` → `GraphIr` → `RillGraphEngine`
> via the `lang` feature in `rill-graph`. See the [execution unification spec](../../docs/execution-unification.md) for details.

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                      GraphBuilder                        │
│  add_source() → idx  add_processor() → idx              │
│  add_sink() → idx    connect_signal(from, to)           │
│  connect_feedback(from, to)                              │
│                                                          │
│  build()       → Graph → ProcessingState  (old path)    │
│  build_ir(r)   → GraphIr → RillGraphEngine  (new path)  │
└──────────────────────────────────────────────────────────┘
```

### Two build paths

`GraphBuilder` supports two execution paths, controlled by the `lang` feature:

**Old path** (`build()`, gated behind `not(lang)`): Constructs nodes from
`NodeFactory`, wires port pointer connections, performs Kahn topological sort,
and produces an immutable `Graph` container. The runtime `ProcessingState` is
extracted via `into_processing_state()` and driven by the I/O callback with
`process_block()`.

**New path** (`build_ir(registry)`, gated behind `lang`): Looks up each node
type in the `Registry`, produces a `GraphIr` (language-agnostic typed IR — see
`rill-lang/src/graph_ir.rs`). The IR is then optimized (`inline_graph` —
dead-code elimination, inlining, merge), lowered to a `ScheduledGraph`
(register-allocation-style buffer assignment), and executed by `RillGraphEngine`
with a pre-allocated buffer pool. This path bypasses `NodeFactory`, `ProcessingState`,
and `Port::propagate` entirely.

### Per-crate registration

Each DSP crate provides a `register_lang_builtins<T>(&mut Registry<T>)`
function that self-sufficiently registers all its built-in functions.
`rill-adrift::lang_builtins::full_registry()` aggregates them into
a single registry — it is a thin aggregator with no built-in logic
of its own.

```rust
use rill_core::builtin::Registry;
use rill_adrift::lang_builtins::full_registry;

let reg: Registry<f32> = full_registry();
// reg now contains all DSP, router, effects, FFT, analog, sampler builtins
```

Node types are registered by their type-name string (e.g. `"rill/lpf"`, `"rill/gain"`)
with per-sample or whole-buffer kind, typed parameter signatures (`ParamType::Signal`,
`ParamType::Float`, `ParamType::Record(RecordSchema)`, etc.), and factory closures.

### Backend ownership

Backends are created **externally** by the orchestrator, not inside
graph nodes.  The `GraphBuilder` no longer holds a `BackendFactory`.

1. The orchestrator creates a backend via `BackendFactory::create_output()` /
   `create_input()` / `create_duplex()`, obtaining `IoDriver`, `IoCapture`,
   `IoPlayback` capability objects
2. `ProcessingState` is extracted from the `Graph` via `into_processing_state()`
3. `state.wire_backends(capture, playback)` injects backends into Source / Sink nodes
4. The orchestrator runs the graph through the driver:

```rust,ignore
let OutputBundle { driver, playback } = bf.create_output(name, &params)?;
let mut state = graph.into_processing_state();
state.wire_backends(None, Some(playback));
state.run_with_driver(driver, running)?;
```

`run_with_driver()` internally calls `set_process_callback` + `run` + parks until stopped.

### Processing flow (old path)

`ProcessingState::process_block(&tick)`:
1. Adopt `tick.sample_rate` — re-`init` all nodes if it differs from the rate
   they were built with (the graph has no clock of its own; it runs at whatever
   rate the backend's `ClockTick` carries — e.g. JACK at 48 kHz)
2. `actor.drain()` — queues/applies `SetParameter` commands
3. Apply sample-accurate parameter changes due for this 256-sample block
   (writes carrying `sample_pos`; writes without one are applied immediately)
4. Creates `RenderContext` from the tick
5. `source.process_block(&ctx, &tick)` — fills output ports
6. `Port::propagate()` — recursive DAG traversal

### Processing flow (new path — `RillGraphEngine`)

`RillGraphEngine::process_tick()` executes a `ScheduledGraph` — a linear
schedule of `Step` variants — over a pre-allocated buffer pool:

| Step | Purpose |
|------|---------|
| `InlineProgram { node_idx, input_bufs, output_bufs }` | Execute a compiled `RillProgram` (implements both `Algorithm<T>` and `MultichannelAlgorithm<T>`) |
| `BufferCopy { from, to, gain, add }` | Copy or accumulate between buffer slots (fan-out / fan-in) |
| `ReadDelay { slot, target }` | Read previous tick's feedback value |
| `WriteDelay { source, slot }` | Save current buffer for next tick's feedback |
| `ReadFeedback { name, target_buf }` | Mix named feedback buffer into sub-engine buffer (duplex only) |
| `WriteFeedback { name, source_buf }` | Capture sub-engine output into feedback buffer (duplex only) |

Parameter routing uses `SetParameter.anchor` (the schedule's `program_names`
entry) for O(1) lookup into the correct program's param map.

`send_clock_tick(&tick)` forwards the `ClockTick` to the rack actor (gated by
`tick.is_final`). Chunking backends (PipeWire, JACK) leave `is_final = true` on
every `block_size` chunk, so control modules receive **one tick per block**;
sample-accurate placement of their resulting parameter writes is handled by
`SetParameter.sample_pos` + `ClockTick.io_quantum`, not by coalescing ticks.

## Serialized graphs (GraphDef)

```rust
let def = GraphDef {
    nodes: vec![
        NodeDef {
            id: 0,
            type_name: "rill/lofi_input",
            backend: Some("ay38910".into()),
            parameters: [("bit_depth", ParamValue::Int(8))].into(),
        },
        NodeDef {
            id: 1,
            type_name: "rill/output",
            backend: None,  // nodes can optionally reference backends by name
            parameters: [("channels", ParamValue::Float(1.0))].into(),
        },
    ],
    connections: vec![
        ConnectionDef { kind: Signal, from_node: 0, from_port: 0, to_node: 1, to_port: 0 },
    ],
};
def.populate(&mut builder)?;
let graph = builder.build()?;
```

## Actor interface

The graph spawns an inline actor (`Actor<CommandEnum>`) whose handler applies
parameter changes to nodes. Control-side code sends `CommandEnum::SetParameter`
through `ActorRef<CommandEnum>` obtained from `graph.handle()`:

```rust
// Simplified graph actor handler (rill-graph/src/graph.rs)
system.spawn("graph", move |msg: CommandEnum| {
    if let CommandEnum::SetParameter(param) = msg {
        if param.sample_pos.is_some() {
            pending.borrow_mut().push(param);      // sample-accurate: defer
        } else {
            let idx = param.port.node_id().inner() as usize;
            nodes[idx].set_parameter(&param.parameter, param.value); // ASAP
        }
    }
});
```

The mailbox is drained from the process callback via
`ProcessingState::process_block()`, which then applies any deferred
sample-accurate writes due for the current block.

## Key components

| Component | Purpose |
|-----------|---------|
| `GraphBuilder` | Mutable builder: nodes and connections; `build()` (old path) or `build_ir(registry)` (new path) |
| `Graph` | Immutable DAG container (old path); `into_processing_state()` extracts runtime state |
| `ProcessingState` | Runtime processor (old path): `process_block()`, `send_clock_tick()` |
| `GraphDef` | Serializable graph topology (nodes + connections) |
| `NodeDef` | Node in a serialized graph: type, params, optional backend name |
| `Port` | Owns buffer, downstream routes, and feedback state (old path) |
| `GraphIr` | Language-agnostic typed IR for multi-node graphs (new path — `rill-lang/src/graph_ir.rs`) |
| `ScheduledGraph` | Linear execution schedule with buffer pool (new path — `rill-lang/src/graph_lower.rs`) |
| `RillGraphEngine` | Buffer pool executor running `ScheduledGraph` steps; implements `Algorithm<T>` and `MultichannelAlgorithm<T>` (new path) |
| `DuplexSchedule` | Split schedule for bridge graphs: left/right `ScheduledGraph` + feedback names (new path) |

## Bridge and feedback

Graph nodes carry `is_bridge`, `feedback_read`, and `feedback_write` annotations
on `GraphNode`. A bridge node splits the graph into left (recording) and right
(playback) sub-graphs. `lower_duplex()` produces a `DuplexSchedule` with embedded
`ReadFeedback`/`WriteFeedback` steps.

`RillGraphEngine::process_tick()` runs a **5-phase tick** for duplex graphs:
1. ReadFeedback — copy named feedback buffers into sub-engine buffer inputs
2. process_left — execute left sub-graph, then `bridge.process_left(inputs)`
3. process_right — `bridge.process_right(outputs)`, then execute right sub-graph
4. WriteFeedback — capture sub-engine outputs into named feedback buffers
5. Shadow copy — swap read/write feedback buffers

Feedback edges in `GraphIr` (marked `EdgeKind::Feedback`) are excluded from
topological sort and lowered to `ReadDelay`/`WriteDelay` steps with implicit
1-sample delay.

## Integration

- `rill-core` — `Node`, `Source`/`Processor`/`Sink` traits, `ClockTick`, `BuiltinSig`, `Registry`, `MultichannelAlgorithm`, `BridgeAlgorithm`
- `rill-core-actor` — `Actor<CommandEnum>` / `ActorRef<CommandEnum>` (mailbox infrastructure)
- `rill-io` — `Input`/`Output` nodes, `IoBackend` trait
- `rill-patchbay` — automation via parameter commands through the actor mailbox
- `rill-fft` — `ConvolverNode` graph node (IR convolution)
- `rill-lang` — `GraphIr`, `RillGraphEngine`, `compile_graph()`, `ScheduledGraph`, `DuplexSchedule` (new execution path)
