# Signal graph (rill-graph)

`rill-graph` provides a static DAG signal graph builder and a serializable graph format.
Processing is handled by `rill-lang`'s `CompiledGraphEngine` — `rill-graph` itself is a pure
topology description, not an execution engine.

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                      GraphBuilder<T, BUF_SIZE>               │
│  add_node(type, params) → idx                               │
│  add_node_with_name(type, params, id, name) → idx            │
│  connect_signal(from_n, from_p, to_n, to_p)                  │
│  connect_control(from_n, from_p, to_n, to_p)                 │
│  connect_feedback(from_n, from_p, to_n, to_p)                │
│  add_resource(GraphResource)                                 │
│  add_routing_entry(idx, from, to, gain)                      │
│                                                              │
│  build_ir(registry) → GraphIr                                │
│      │                                                       │
│      ▼                                                       │
│  graph_compiler::compile() → CompiledGraph                   │
│      │                                                       │
│      ▼                                                       │
│  CompiledGraphEngine<T, BUF_SIZE>  (in rill-lang)            │
└──────────────────────────────────────────────────────────────┘
```

### Nodes

All nodes are added through a unified `add_node` API — there are no separate
`add_source`/`add_processor`/`add_sink` methods. The `type_name` string
determines the node kind (matched against the built-in registry at `build_ir` time).

```rust
use rill_graph::GraphBuilder;

const BUF_SIZE: usize = 256;
let mut builder = GraphBuilder::<f32, BUF_SIZE>::new();

// Add nodes by their registry type name
let osc = builder.add_node("rill/sinosc", &[("freq", 440.0)].into());
let lpf = builder.add_node_with_name("rill/lpf", &[("cutoff", 800.0)].into(), 1, "filter");
let out = builder.add_node("rill/output", &[].into());
```

### Connections

Edges connect `(node_idx, port_idx)` pairs. Four edge kinds are supported:

| Kind | Purpose |
|------|---------|
| `connect_signal` | Forward signal flow — included in topological sort |
| `connect_control` | Modulation values (e.g. LFO → filter cutoff) |
| `connect_clock` | Timing signals (MIDI clock, transport) |
| `connect_feedback` | Feedback loops — excluded from topological sort, implicit 1-sample delay |

```rust
builder.connect_signal(osc, 0, lpf, 0);    // osc output → lpf input
builder.connect_signal(lpf, 0, out, 0);    // lpf output → output
```

### Resources

Named resources (tape loops, shared buffers) can be registered and referenced
by node parameters:

```rust
builder.add_resource(GraphResource {
    name: "tape_0".into(),
    kind: "tape".into(),
    capacity: 48000,
});
```

### Compilation pipeline

`build_ir(registry)` converts the builder's internal representation into a `GraphIr`
(rill-lang's multi-node intermediate representation):

1. **Node lookup** — each recipe's `type_name` is resolved in the `Registry`
2. **Topological sort** — Kahn's algorithm on signal edges; cycles are rejected
3. **Built-in compilation** — each node's built-in is compiled to an `Ir` (single-node program)
4. **Optimization** — dead-edge elimination, constant inlining, parallel node merging
   (`rill-lang/src/graph_optimize.rs`)
5. **Compiler** — `graph_compiler::compile()` flattens `GraphIr` into a `CompiledGraph`
   with a fixed-size `FixedBuffer` pool and ordered `NodeClosure` vector

The resulting `CompiledGraphEngine` implements both `Algorithm<T>` (SISO) and
`MultichannelAlgorithm<T>` (MIMO). It runs nodes in topological order with zero
heap allocation on the signal path.

### Per-crate registration

Each DSP crate provides a `register_lang_builtins<T>(&mut Registry<T>)`
function that registers all its built-in node types:

```rust
use rill_core::builtin::Registry;
use rill_adrift::lang_builtins::full_registry;

let mut reg: Registry<f32> = full_registry();
// reg now contains all DSP, router, effects, FFT, analog, sampler builtins
```

Node types are registered by their type-name string (e.g. `"rill/lpf"`, `"rill/gain"`)
with typed parameter signatures and factory closures.

### Actor interface

`CompiledGraphEngine::handle()` returns an `ActorRef<CommandEnum>`. Control-side
code sends `CommandEnum::SetParameter` through this handle:

```rust
use rill_core::queues::CommandEnum;
use rill_core::traits::ParamValue;

engine.handle().send(CommandEnum::SetParameter(SetParameter {
    anchor: "filter".into(),
    parameter: "cutoff".into(),
    value: ParamValue::Float(2000.0),
    port: String::new(),
    source: SignalOrigin::Manual,
    timestamp: 0,
    sample_pos: None,
})).unwrap();
```

The engine drains its mailbox at the start of each `process()` call, applying
parameter changes before processing the current block.

## Serialized graphs (GraphDef)

Graph topology can be serialized to JSON or CBOR via the `serialization` feature:

```rust
use rill_graph::serialization::{GraphDef, NodeDef, SourceDef, ConnectionDef, SignalKind};

let def = GraphDef {
    format_version: "rill/1".into(),
    sample_rate: 44100.0,
    block_size: 256,
    resources: vec![],
    description: None,
    nodes: vec![
        NodeDef::Source(SourceDef {
            id: 0,
            name: Some("osc".into()),
            type_name: "rill/sinosc".into(),
            parameters: [("freq", ParamValue::Float(440.0))].into(),
        }),
        NodeDef::Processor(ProcessorDef {
            id: 1,
            name: Some("filter".into()),
            type_name: "rill/lpf".into(),
            parameters: [("cutoff", ParamValue::Float(800.0))].into(),
        }),
        NodeDef::Sink(SinkDef {
            id: 2,
            name: Some("out".into()),
            type_name: "rill/output".into(),
            parameters: [].into(),
        }),
    ],
    connections: vec![
        ConnectionDef {
            from_node: 0, from_port: 0,
            to_node: 1, to_port: 0,
            kind: SignalKind::Signal,
        },
        ConnectionDef {
            from_node: 1, from_port: 0,
            to_node: 2, to_port: 0,
            kind: SignalKind::Signal,
        },
    ],
};
def.populate(&mut builder)?;
let engine = builder.build_ir(&reg, sample_rate)?;
```

`NodeDef` is an enum with four variants: `Source(SourceDef)`, `Processor(ProcessorDef)`,
`Router(RouterDef)`, `Sink(SinkDef)`.

## Bridge and feedback

Graph nodes carry optional bridge and feedback annotations (`is_bridge`,
`feedback_read`, `feedback_write` on `GraphNode`). A bridge node splits the
graph into left (recording) and right (playback) sub-graphs, connected through
named feedback buffers.

Feedback edges in `GraphIr` (marked `EdgeKind::Feedback`) are excluded from
topological sort and carry implicit 1-sample delay — they connect the current
tick's output back as the next tick's input.

## Key components

| Component | Location | Purpose |
|-----------|----------|---------|
| `GraphBuilder<T, BUF_SIZE>` | `rill-graph` | Mutable builder: adds nodes, connections, resources; `build_ir()` produces `GraphIr` |
| `GraphResource` | `rill-graph` | Named shared resource (tape loop, buffer) |
| `BuildError` | `rill-graph` | Error type for graph construction |
| `GraphDef` | `rill-graph::serialization` | Serializable graph topology (format_version, nodes, connections) |
| `NodeDef` | `rill-graph::serialization` | Enum: Source(SourceDef), Processor(ProcessorDef), Router(RouterDef), Sink(SinkDef) |
| `ConnectionDef` | `rill-graph::serialization` | Serializable connection: from_node/port → to_node/port + SignalKind |
| `GraphIr` | `rill-lang::graph_ir` | Multi-node IR — bridges GraphBuilder to rill-lang compilation |
| `GraphNode` | `rill-lang::graph_ir` | One graph node: arity, IR, params, bridge/feedback annotations |
| `GraphEdge` | `rill-lang::graph_ir` | Directed edge: node names + ports + EdgeKind |
| `EdgeKind` | `rill-lang::graph_ir` | Signal, Control, Clock, or Feedback |
| `CompiledGraphEngine<T, BUF_SIZE>` | `rill-lang::graph_engine` | Execution engine: flat NodeClosure vector + FixedBuffer pool; implements `Algorithm<T>` and `MultichannelAlgorithm<T>` |

## Integration

- `rill-core` — `BuiltinSig`, `Registry`, `Algorithm`, `MultichannelAlgorithm`, `ParamValue`
- `rill-core-actor` — `ActorRef<CommandEnum>` / `Mailbox` (parameter control)
- `rill-lang` — `GraphIr`, `GraphNode`, `GraphEdge`, `CompiledGraphEngine`, `graph_compiler::compile()`, `graph_optimize::optimize()`
- `rill-patchbay` — automation via `CommandEnum::SetParameter` through `engine.handle()`
- `rill-io` — input/output backends connect to graph through compiled engine
