# rill-graph

Static DAG signal graph builder and serializable graph format.
Processing is driven by `rill-lang`'s `CompiledGraphEngine` — this crate
provides topology description only, not an execution engine.

## Key components

- **`GraphBuilder<T, BUF_SIZE>`** — mutable builder: `add_node()`, `connect_signal/control/clock/feedback()`, `add_resource()`, `build_ir()`
- **`GraphResource`** — named shared resource (tape loop, buffer) referenced by node parameters
- **`BuildError`** — error type for graph construction (cycle detection, unknown node types)
- **`GraphDef` / `NodeDef`** — serializable graph topology (JSON/CBOR, behind `serialization` feature)
- **`build_ir(registry)`** — produces `rill_lang::graph_ir::GraphIr`, compiled by `graph_compiler::compile()` into `CompiledGraphEngine`

## Public API

```rust
use rill_graph::GraphBuilder;

const BUF_SIZE: usize = 256;
let mut builder: GraphBuilder<f32, BUF_SIZE> = GraphBuilder::new();

// Add nodes by type name
let osc = builder.add_node("rill/sinosc", &[("freq", 440.0)].into());
let lpf = builder.add_node_with_name("rill/lpf", &[("cutoff", 800.0)].into(), 1, "filter");
let out = builder.add_node("rill/output", &[].into());

// Wire connections
builder.connect_signal(osc, 0, lpf, 0);
builder.connect_signal(lpf, 0, out, 0);

// Compile via rill-lang
let engine = builder.build_ir(&registry, 44100.0)?;
```

## Hard-RT safe

`rill-graph` itself performs no heap allocation or syscalls — it's a pure
builder and topology description. The runtime execution (shared `FixedBuffer`
pool, flat `NodeClosure` vector) runs in `rill-lang`'s `CompiledGraphEngine`
with zero allocation on the signal path.

## Debug infrastructure (`debug` feature)

- `build_ir()` automatically inserts `ProbePoint` IR instructions at each node's
  output, enabling signal-level inspection via `rill-analyzer`
- Compiles graph nodes to complete `rill_lang::Ir` with builtins, params, and
  instructions — mirrors the rill-lang DSL compilation path

## Dependencies

- `rill-core` — `Registry`, `BuiltinSig`, `Params`
- `rill-lang` — `GraphIr`, `graph_compiler::compile()`, `CompiledGraphEngine`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-graph>
