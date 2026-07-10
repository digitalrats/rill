# Execution System Unification — Design

> **Status:** Design — awaiting user review, then implementation plan.
> **Date:** 2026-07-10
> **Scope:** Replace the old graph execution engine (`ProcessingState`, `Port::propagate`, `NodeVariant`) with rill-lang IR compilation. Graph nodes become rill-lang built-in functions. `GraphBuilder` produces `GraphIr`, compiled and executed by `RillGraphEngine`.

## Motivation

rill has two parallel execution systems:

| | Old engine (rill-graph) | Lang engine (rill-lang) |
|---|---|---|
| **Input** | `GraphBuilder` (programmatic) or JSON `GraphDef` | rill-lang DSL source |
| **IR** | `NodeVariant` + port pointers | `Ir` (register machine) |
| **Execution** | `ProcessingState::process_block` → `Port::propagate` (recursive DAG) | `RillProgram::process` (flat interpreter) |
| **Multi-IO** | Via `Router` trait + N×M ports | Via `MultichannelAlgorithm<T>` |
| **Optimization** | None (opaque nodes) | Schedule + regalloc (within one program) |

The old engine is ~2000 lines of hand-written DAG traversal with raw pointers, zero-copy aliasing logic, split-chain recording/playback, and four node variant types. The new engine is a flat IR with a linear schedule — simpler, optimizable, and already supports the JIT path.

In the per-crate self-registration refactor, every DSP crate now exports a `register_lang_builtins()` function. Every graph node already has (or will have) a corresponding lang built-in. The two registration systems (`NodeFactory` and `Registry`) register the same DSP primitives. They should be unified.

**Goal**: `NodeFactory` and `ProcessingState` are deleted. `GraphBuilder` produces `GraphIr` (rill-lang graph IR), which is optimized and lowered to a `ScheduledGraph`, executed by `RillGraphEngine`. The graph execution path becomes:

```
GraphBuilder → GraphIr → Optimizer → Lowerer → RillGraphEngine(MultichannelAlgorithm)
```

## Confirmed decisions

| Dimension | Decision |
|---|---|
| **GraphBuilder output** | `GraphIr` (nodes + edges), not `Graph`/`ProcessingState` |
| **Node registration** | `Registry<T>` (rill-lang built-ins) replaces `NodeFactory` |
| **`register_graph_nodes()`** | Deleted. Only `register_lang_builtins()` remains per crate |
| **Missing built-ins** | Create 8 new `BlockBuiltin<T>` wrappers for remaining graph nodes |
| **IR** | `GraphIr` → optimizer → `ScheduledGraph` via lowering in rill-lang |
| **Engine** | `RillGraphEngine` implements `MultichannelAlgorithm<T>`, executes `ScheduledGraph` |
| **What's deleted** | `Port<T, BUF_SIZE>`, `NodeVariant`, `ProcessingState`, `NodeConstructor`, `NodeFactory`, `Port::propagate`, `PortAction`, split-chain mode |
| **Backward compat** | `GraphDef` JSON format preserved — `populate()` feeds same `GraphBuilder`, which now produces `GraphIr` |
| **Algorithm<T>** | Kept for standalone DSP use. `MultichannelAlgorithm<T>` for graphs |

## Architecture

### Old (deleted)

```
NodeFactory<T, BUF_SIZE>        ←  register_graph_nodes() per crate
         │
    construct(name, params) → NodeVariant (Source/Processor/Router/Sink)
         │                              │
    GraphBuilder::build()               │ port wiring, raw pointers
         │                              │
         ▼                              ▼
    Graph<UnsafeCell<Vec<NodeVariant>>> → ProcessingState
                                              │
                                    process_block() → Port::propagate() (recursive)
```

### New

```
Registry<T>                     ←  register_lang_builtins() per crate
    │
    get(name) → BuiltinSig + factory: &[f64] → Box<dyn Algorithm<T>>
    │
    GraphBuilder::build_ir(registry)
    │
    ▼
GraphIr { nodes: Vec<GraphNode>, edges: Vec<GraphEdge> }
    │
    ▼
Optimizer (inlining, DCE, parallel merge, lateral merge, LTI reorder)
    │
    ▼
Lowerer (topo sort, liveness analysis, buffer allocation)
    │
    ▼
ScheduledGraph { steps: Vec<Step>, buffers: usize }
    │
    ▼
RillGraphEngine<T, BUF> : MultichannelAlgorithm<T>
    │  execute steps linearly over buffer pool
    │  drain actor mailbox for SetParameter
```

## Missing built-ins

The following graph nodes have no corresponding lang built-in yet:

| Graph node | Crate | Param count | Type |
|---|---|---|---|
| `rill/biquad` | digital-filters | 4 (filter_type, cutoff, q, gain_db) | Block |
| `rill/delay` | digital-effects | 3 (time_ms, feedback, mix) | Block |
| `rill/distortion` | digital-effects | 2 (drive, mix) | Block |
| `rill/limiter` | digital-effects | 2 (threshold, release) | Block |
| `rill/graphic_eq` | router | 1 (bands config? or fixed 31-band) | Block |
| `rill/sampler` | sampler | 2 (file_path?, playback_rate) | Block |
| `rill/convolver` | fft | 2 (impulse_len, mix) | Block |
| `rill/cassette_deck` | analog-effects | 3 (tape_speed, bias_level, noise_floor) | Block |

`write_head`/`read_head` are tape-loop infrastructure — graph-only concepts, not needed as lang built-ins. They can be handled as `GraphResource` references in the IR.

Each new built-in is a `BlockBuiltin<T>` wrapper around the existing `Algorithm<T>` implementation in its crate:

```rust
// Example: rill-digital-effects/src/lang/delay.rs
pub struct DelayBuiltin<T: Transcendental> {
    inner: rill_digital_effects::Delay<T>,
}

impl<T: Transcendental> BlockBuiltin<T> for DelayBuiltin<T> { ... }
impl<T: Transcendental> Algorithm<T> for DelayBuiltin<T> { ... }
```

Registered in the crate's existing `register_lang_builtins()` function.

## GraphIr data model

```rust
// rill-lang/src/graph_ir.rs

pub struct GraphIr {
    pub inputs: usize,
    pub outputs: usize,
    pub nodes: IndexMap<String, GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub topo_order: Vec<String>,
}

pub struct GraphNode {
    pub arity: (usize, usize),   // (signal_ins, signal_outs)
    pub ir: Ir,                  // pre-compiled IR for this node
    pub params: Vec<ParamDef>,   // parameter slots
    pub keep: bool,              // forced independent node
    pub inline: bool,            // forced inlined
}

pub struct GraphEdge {
    pub from_node: String,
    pub from_port: usize,
    pub to_node: String,
    pub to_port: usize,
    pub kind: EdgeKind,
}

pub enum EdgeKind {
    Signal,
    Feedback,
}
```

## GraphBuilder changes

### Old `build()` signature

```rust
impl GraphBuilder<T, BUF_SIZE> {
    pub fn build(self, system: &ActorSystem) -> Result<Graph<T, BUF_SIZE>, BuildError>;
}
```

### New `build_ir()` signature

```rust
impl GraphBuilder {
    pub fn build_ir(self, registry: &Registry<T>) -> Result<GraphIr, BuildError>;
}
```

`GraphBuilder` no longer needs `BUF_SIZE` or `ActorSystem`. It delegates compilation to `Registry`:

1. For each `NodeRecipe`:
   - Look up `registry.sig(type_name)` → `BuiltinSig` (for arity)
   - Look up `registry.get_block(type_name)` → factory closure
   - Call factory with folded params → `Box<dyn BlockBuiltin<T>>`
   - Compile the `BlockBuiltin` to `Ir` (pre-compile step, or lower inline)
   - Create `GraphNode { arity, ir, params }`

2. For each edge:
   - Create `GraphEdge { from_node, from_port, to_node, to_port, kind }`

3. Topological sort (Kahn's on signal edges, excluding feedback)

4. Return `GraphIr`

`GraphBuilder` itself moves from `rill-graph` to `rill-lang`? Or stays in `rill-graph` but depends on `rill-lang`?

**Decision**: `GraphBuilder` stays in `rill-graph`. `rill-graph` gains an optional dependency on `rill-lang` (for `Registry`, `Ir`). The `graph` feature in each DSP crate becomes `rill-graph + rill-lang`.

## Optimizer passes

From the [graph-compilation spec](2026-07-07-rill-lang-graph-compilation-design.md), applied to `GraphIr`:

| Pass | Condition | Action |
|------|-----------|--------|
| **Dead edge elimination** | Edge destination never read | Remove edge |
| **Node inlining** | `inline: true`, or no dynamic params and `keep: false` | Merge node IR into parent, rename registers, remove node |
| **Parallel merge** | Two nodes with identical IR, different params | Merge into one with multiplexed params |
| **Lateral (stereo) merge** | Two identical mono chains in parallel | Replace with stereo node if variant exists |
| **LTI reorder** | Adjacent feed-forward linear-time-invariant nodes | Swap to reduce buffer pressure |

### Inlining rules

- Node's IR instructions appended to parent with register renaming
- State slots (`ReadState`/`WriteState`) appended to parent
- Delay slots appended to parent
- Parameters appended to parent's param list
- Only when node has **no dynamic params** (all compile-time constants) **or** `inline: true`

## Lowering: GraphIr → ScheduledGraph

### Topological sort

Already computed during `build_ir()`. Feedback edges excluded — handled via `ReadDelay`/`WriteDelay` steps.

### Buffer liveness analysis

For each intermediate edge `(from, from_port) → (to, to_port)`:
- Buffer required between the two nodes
- Live from `from`'s schedule position until `to` finishes
- Interference graph: two buffers overlap if liveness intervals intersect

### Buffer allocation

Register-allocation-style graph coloring. Minimum number of `FixedBuffer<T, BUF>` slots. Fan-in edges sum into same buffer. Fan-out edges share via pointer aliasing (zero-copy).

Zero-copy rule: when `out_degree(source_port) == 1` AND `in_degree(target_port) == 1`, source output buffer IS target input buffer — same slot, no copy.

### ScheduledGraph

```rust
pub struct ScheduledGraph {
    pub inputs: usize,
    pub outputs: usize,
    pub steps: Vec<Step>,
    pub buffers: usize,
    pub output_mapping: Vec<usize>,
}

pub enum Step {
    InlineProgram {
        node_idx: usize,
        input_bufs: Vec<usize>,
        output_bufs: Vec<usize>,
        param_indices: Vec<usize>,
    },
    BufferCopy {
        from: usize,
        to: usize,
        gain: f32,
        add: bool,
    },
    ReadDelay { slot: usize, target: usize },
    WriteDelay { source: usize, slot: usize },
}
```

## RillGraphEngine

```rust
pub struct RillGraphEngine<T: Transcendental, const BUF: usize> {
    schedule: ScheduledGraph,
    programs: Vec<RillProgram<T>>,
    buffers: Vec<FixedBuffer<T, BUF>>,
    delay_buffers: Vec<FixedBuffer<T, BUF>>,
    param_values: Vec<Vec<f64>>,
    actor: Actor<CommandEnum>,
    actor_ref: ActorRef<CommandEnum>,
}

impl<T, const BUF: usize> MultichannelAlgorithm<T> for RillGraphEngine<T, BUF> {
    fn num_inputs(&self) -> usize { self.schedule.inputs }
    fn num_outputs(&self) -> usize { self.schedule.outputs }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) {
        // 1. Drain actor mailbox → update param_values
        // 2. Write inputs[0..N] to input buffers
        // 3. Execute steps sequentially:
        //    ReadDelay: delay→buffer
        //    InlineProgram: prog.process(input_bufs, output_bufs) — zero-copy aliased
        //    BufferCopy: copy/accumulate between buffers
        //    WriteDelay: buffer→delay  
        // 4. Read output buffers → outputs[0..M]
    }
}
```

## What gets deleted

| Deleted | Crate | LOC affected |
|---------|-------|------|
| `NodeVariant` (Source/Processor/Router/Sink) | `rill-core` | ~30 |
| `Port<T, BUF_SIZE>` (propagate, zero-copy, downstream pointers) | `rill-core` | ~800 |
| `PortAction`, `Algorithm` on Port | `rill-core` | ~100 |
| `Processable::process_block` on NodeVariant | `rill-core` | ~50 |
| `NodeConstructor` trait | `rill-graph` | ~20 |
| `NodeFactory` (HashMap of NodeConstructor) | `rill-graph` | ~150 |
| `Graph` (UnsafeCell<Vec<NodeVariant>>, raw pointers) | `rill-graph` | ~400 |
| `ProcessingState` (process_block, wire_backends, run_with_driver) | `rill-graph` | ~200 |
| Split-chain mode (p_forward, p_pull, p_process_branch) | `rill-graph` | ~200 |
| `register_graph_nodes()` in every DSP crate | 10 crates | ~400 |
| `registration.rs` (rill-adrift) | `rill-adrift` | ~400 |
| `io::output::Output`, `io::input::Input` (old I/O nodes) | `rill-io` | ~200 |

**Total: ~3000 lines deleted.**

## What stays

| Stays | Why |
|-------|-----|
| `Algorithm<T>` trait | Standalone DSP, not graph-specific |
| `MultichannelAlgorithm<T>` trait | Multi-IO graph engine |
| `GraphBuilder` | Programmatic graph construction API |
| `GraphDef` | JSON serialization, `populate(GraphBuilder)` unchanged |
| `RillProgram<T>` | Single-algorithm execution |
| `RillGraphEngine` | Multi-node graph execution (enhanced) |
| `BuiltinSig`, `Registry<T>`, `BlockBuiltin<T>` | Lang built-in infrastructure |
| `register_lang_builtins()` per crate | Self-sufficient registration |

## Implementation phases

| # | Phase | Scope |
|---|-------|-------|
| 1 | **Missing built-ins** | Create `BlockBuiltin<T>` wrappers for biquad (general), delay, distortion, limiter, graphic_eq, sampler, convolver, cassette_deck |
| 2 | **GraphIr data model** | `GraphIr`, `GraphNode`, `GraphEdge` in `rill-lang/src/graph_ir.rs` |
| 3 | **GraphBuilder::build_ir()** | New method on GraphBuilder, uses Registry instead of NodeFactory |
| 4 | **Optimizer** | `graph_optimize.rs` — inlining, DCE, merge passes |
| 5 | **Lowerer** | `graph_lower.rs` — topo sort, liveness, buffer allocation → ScheduledGraph |
| 6 | **RillGraphEngine** | Enhanced engine with ScheduledGraph + buffer pool |
| 7 | **Delete old engine** | Remove Port, ProcessingState, NodeVariant, NodeFactory, NodeConstructor |
| 8 | **Unify registration** | Delete `register_graph_nodes()`, delete `registration.rs` in rill-adrift |
| 9 | **Update GraphDef/IO** | Wire GraphDef → build_ir(), update IO backends for new engine |
