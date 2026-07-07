# rill-lang Graph Compilation — Design

**Date:** 2026-07-07
**Status:** Draft — pending implementation plan
**Scope:** Extend `rill-lang` from single-algorithm compiler to full graph compiler with optimization passes and a custom runtime engine.

## Motivation

Currently `rill-lang` compiles a single DSP algorithm into `RillProgram<T>: Algorithm<T>`. Graphs are built separately via `rill-graph::GraphBuilder` (programmatic) or JSON `GraphDef` (serialized), then executed by the `rill-graph` runtime.

This design extends `rill-lang` with the ability to define an entire signal graph as a single DSL program — named subexpressions with parameters become graph nodes, wiring is expressed via existing Faust-style combinators (`:`, `,`, `<:`, `:>`, `~`). The compiler performs graph-level optimizations (inlining, DCE, parallel merge, stereo lateral merge, LTI reordering) and produces a custom runtime engine (`RillGraphEngine`) that replaces `rill-graph`'s execution layer for compiled graphs.

## Design Decisions

| Decision | Rationale |
|----------|-----------|
| **Custom engine (`RillGraphEngine`)** instead of extending `rill-graph` | Full control over scheduling and buffer allocation; `rill-graph` remains for JSON-based graphs |
| **Graph IR layer** between typechecker and lowering | Clean separation of topology optimization from per-sample IR; expandable to resources/split-chain later |
| **`param` keyword** for named subgraph nodes | Unambiguous: `param` defines a named anchor for `SetParameter`; `def` is always inlined |
| **Engine implements `Algorithm<T>` + `ActorRef<CommandEnum>`** | Drop-in replacement for `rill-graph::Graph` from the perspective of `ModularSystem` and patchbay |
| **Linear execution** (no split-chain in v1) | Simplifies the initial implementation; split-chain can be added later without API changes |

## Architecture

```
rill-lang DSL source
    │
    ▼
Lex → Parse → Typecheck     ← existing pipeline
    │
    ▼
Graph IR                    ← NEW: graph-level IR
    │
    ▼
Optimizer passes            ← NEW: DCE, inline, parallel merge, lateral merge, LTI reorder
    │
    ▼
Lowering → ScheduledGraph   ← NEW: topo sort, liveness, buffer allocation
    │
    ▼
RillGraphEngine             ← NEW: runtime
    │
    ▼
ModularSystem / patchbay    ← unchanged consumers
```

## DSL Extensions

### Name disambiguation: `param` keyword vs `param()` built-in

`param` keyword and `param()` built-in are distinct constructs:

- **`param myNode = expr;`** — keyword. Declares a named subgraph anchor for `SetParameter`. Used at the *graph level*.
- **`param("name", default, min, max)`** — built-in function. Declares a runtime parameter slot with metadata. Used *inside* an algorithm expression.

A `param` keyword node may contain `param()` calls in its body:

```ocaml
param myFilter = _ : onepole(param("cutoff", 1000.0, 20.0, 20000.0), 0.707);
```

The parser distinguishes them by position: `param` at the start of a statement (followed by an identifier) is the keyword; `param(` with parentheses is the built-in function call.

### `param` — named subgraph node

```ocaml
param myFilter = _ : onepole(cutoff, q);       // 1→1, parameterized by "cutoff", "q"
param stereoReverb = _,_ : reverb(mix, size);   // 2→2
```

- Creates a named anchor for `SetParameter` messages. Parameters are applied at runtime via the engine's actor mailbox.
- May or may not remain a separate graph node — the optimizer decides based on whether parameters are dynamic (`param()` values) or fully constant.

### `keep param` — forced independent node

```ocaml
keep param myFilter = _ : onepole(cutoff, q);   // NEVER inlined
```

- Guaranteed to remain as an independent node with its own `ActorRef<CommandEnum>`. Use when the node's parameters will be modulated by patchbay at runtime.

### `inline param` — forced inlining

```ocaml
inline param myFilter = _ : onepole(1000, 0.707);  // ALWAYS inlined
```

- Inlines into the parent graph node. Useful when the node is a pure organizational abstraction with no dynamic parameters.

### `def` — always inlined function

```ocaml
def gain6(x) = x * 2.0;
def stereo_pair(l, r) = l + r, l - r;
```

- Never creates a graph boundary. Pure compile-time abstraction.

### `process` — root definition (unchanged)

```ocaml
process = osc : myFilter : _;
```

The compiler walks `process`, identifies `param` subexpressions as graph nodes, `def` calls as inline, and builds the Graph IR topology from combinator wiring.

### No `node(...)` factory references required

All factory node types (`rill/sine`, `rill/biquad`, etc.) are wrappers over algorithms already available as `rill-lang` built-in functions. DSL users reference them directly as function calls — no need for a separate `node("rill/sine")` syntax. If a factory type is missing a corresponding built-in, it can be registered via the existing `Registry<T>`.

## Graph IR

### Types (`src/graph_ir.rs`)

```rust
pub struct GraphIr {
    pub inputs: usize,                          // graph input channels (0 or 1 for process)
    pub outputs: usize,                         // graph output channels (1 for process)
    pub nodes: IndexMap<String, GraphNode>,     // node name → node
    pub edges: Vec<GraphEdge>,
    pub topo_order: Vec<String>,                // topological order of node names
}

pub struct GraphNode {
    pub arity: (usize, usize),                  // (inputs, outputs)
    pub ir: Ir,                                 // compiled IR for this node
    pub params: Vec<ParamDef>,                  // parameter slots
    pub keep: bool,                             // true if annotated with `keep`
    pub inline: bool,                           // true if annotated with `inline`
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
    Feedback,       // from `~` combinator
}
```

### Building (`src/graph_build.rs`)

The graph builder walks the typed AST of `process`:

1. When encountering a `param` reference, it creates a `GraphNode` with the parameter's already-typechecked body compiled to `Ir`.
2. Combinators (`:`, `,`, `<:`, `:>`, `~`) between param references become `GraphEdge`s.
3. `def` references are inlined — their body is substituted at the call site.
4. If `process` is a single expression with no `param` references, the result is a single-node graph (current behavior — no regression).

## Optimizer (`src/graph_optimize.rs`)

Optimizer passes are applied to `GraphIr` sequentially:

| Pass | Condition | Action |
|------|-----------|--------|
| **Dead edge elimination** | Edge destination port is never read | Remove edge |
| **Node inlining** | Node has `inline: true`, OR has no dynamic params and `keep: false` | Merge node's IR into parent, rename registers, remove node + edges |
| **Parallel merge** | Two nodes with identical IR but different constant params | Merge into one node with multiplexed parameter set; duplicate references in topo order |
| **Lateral (stereo) merge** | Two identical mono chains in parallel composition AND a stereo built-in variant exists | Replace pair with single stereo node (2→2) |
| **LTI reorder** | Two adjacent feed-forward linear-time-invariant nodes | Swap order if it reduces buffer pressure or enables further merges |

### Inlining rules

- Node's IR instructions are appended to the parent's IR with register renaming (offset by parent's `num_regs`).
- State slots (`ReadState`/`WriteState`) of the inlined node are appended to parent's state layout.
- Delay slots (`ReadDelay`/`WriteDelay`) are appended to parent's delay rings.
- Parameters of the inlined node are appended to parent's param list.

### Lateral merge rules

- Detects `l_chain, r_chain` where `l_chain` and `r_chain` have identical IR (same instructions, same constants, same structure).
- Checks if the built-in registry has a stereo variant of each called block built-in (same name, doubled arity).
- If yes: replaces both chains with a single stereo node. If no: leaves as two mono nodes.
- Stereo node: input ports doubled, output ports doubled, Algorithm processes both channels in one call.

## Lowering (`src/graph_lower.rs`)

### Feedback — delayed data path, not an activation edge

In `rill-graph`, feedback edges are **excluded from topological sort** — they do not participate in activation propagation. The graph is acyclic for activation: nodes are activated strictly from sources to sinks via signal edges only. Feedback carries **data** with an implicit 1-callback delay: `Port::pre_process()` mixes the previous tick's feedback buffer into the current input before processing, and `Port::snapshot_feedback()` saves the current output for the next tick.

`ScheduledGraph` uses the same model: `~` combinator creates a pair of `ReadDelay`/`WriteDelay` steps that bracket the participating nodes in the schedule. Activation flows linearly; the delay buffer bridges across callback boundaries.

```ocaml
param filt = _ : onepole(cutoff, q);
process = osc : filt ~ _;    // feedback from filt.out to filt.in
```

Both `osc` and `filt` **can be `keep param`** — no cycles in the activation graph.

```
Schedule (1 callback tick):
  Step 1: ReadDelay(delay_0)  → buffer[1]     // reads previous tick's filt output
  Step 2: InlineProgram(osc)  → buffer[0]
  Step 3: InlineProgram(filt) → buffer[0], buffer[1] → buffer[2]
  Step 4: WriteDelay(buffer[2]) → delay_0       // saves for next tick
  Step 5: output = buffer[2]
```

Mismatch with `read()` timing is irrelevant — `ReadDelay` fills buffer[1] *before* filt reads it, `WriteDelay` saves *after* filt writes. From filt's perspective, buffer[1] always contains the previous tick's output.

### Topological sort

Already computed during Graph IR construction (Kahn's algorithm on signal edges). Feedback edges are excluded from the sort — they connect output ports to delay inputs, handled at the IR level (existing `~` → `ReadDelay`/`WriteDelay`).

### Buffer liveness analysis

For each intermediate edge `(from_node, from_port) → (to_node, to_port)`:

1. The edge requires a buffer of size `BUF_SIZE` between the two nodes.
2. Liveness: buffer is live from `from_node`'s position in topo order until `to_node` completes.
3. Interference graph: two buffers interfere if their liveness intervals overlap.

### Buffer allocation

Register-allocation-style graph coloring: allocate the minimum number of `FixedBuffer<T, BUF>` slots. Fan-in edges (multiple sources → one destination) sum into the same buffer. Fan-out edges (one source → multiple destinations) share the same source buffer via pointer aliasing (zero-copy). The allocator reuses buffer slots whose liveness intervals do not overlap.

**Zero-copy aliasing rule:** For any edge where `out_degree(source_port) == 1` AND `in_degree(target_port) == 1`, the output buffer of the source step IS the input buffer of the target step — they are the same buffer slot. No copy instruction is emitted. Data is mutated in-place by the target step.

```rust
pub struct ScheduledGraph {
    pub inputs: usize,
    pub outputs: usize,
    pub steps: Vec<Step>,
    pub buffers: usize,                         // total buffer slots needed
    pub output_mapping: Vec<usize>,              // which buffer → which output channel
}
```

### Execution model — two-phase activation + processing

The engine mirrors `rill-graph`'s two-phase approach as a linear schedule with explicit copy/buffer-alias information pre-computed at compile time:

```rust
pub enum Step {
    /// Execute an inline program. Input and output buffers are logical indices.
    /// For zero-copy edges, input_buf[k] == some previous step's output_buf[j] —
    /// the same buffer slot, mutated in-place.
    InlineProgram {
        node_idx: usize,                        // index into programs vec
        input_bufs: Vec<usize>,                 // input buffer indices (may alias previous outputs)
        output_bufs: Vec<usize>,                // output buffer indices
        param_indices: Vec<usize>,              // param indices within the program
    },
    /// Explicit copy step — used only for non-zero-copy edges
    /// (fan-out: one source → multiple targets, fan-in: multiple sources → one target)
    BufferCopy {
        from: usize,
        to: usize,
        gain: f32,                              // 1.0 for pass-through, or mix gain for fan-in
        add: bool,                              // false = overwrite, true = accumulate (fan-in)
    },
    /// Read delay buffer containing previous tick's value → target buffer.
    /// Precedes the node that consumes the feedback.
    ReadDelay {
        slot: usize,                            // delay slot index
        target: usize,                          // buffer to fill
    },
    /// Save buffer content → delay slot for next tick.
    /// Follows the node that produces the feedback.
    WriteDelay {
        source: usize,                          // buffer to save
        slot: usize,                            // delay slot index
    },
}
```

**How zero-copy works in the engine:**

At compile time, the buffer allocator assigns buffer slots. For exclusive 1:1 edges, the source step's output buffer and the target step's input buffer reference the **same** slot index. The engine simply passes the same `&mut [T; BUF]` to both steps — the first writes, the second reads (and may overwrite in place). No data movement.

At runtime, the engine walks `steps` sequentially:

```
for step in steps:
    match step:
        ReadDelay {slot, target} → buffer[target] = delay[slot]
        InlineProgram {..} → prog.process(input_bufs, output_bufs)
            // input_bufs and output_bufs are &mut slices into the buffer pool.
            // If input_bufs[k] aliases a previous output, they point to the same memory.
        BufferCopy {..} → explicit copy into target buffer
        WriteDelay {source, slot} → delay[slot] = buffer[source]
```

## Engine (`src/graph_engine.rs`)

```rust
pub struct RillGraphEngine<T, const BUF: usize> {
    schedule: ScheduledGraph,
    programs: Vec<RillProgram<T>>,              // one per node (pre-compiled)
    buffers: Vec<FixedBuffer<T, BUF>>,
    actor: Actor<CommandEnum>,
    actor_ref: ActorRef<CommandEnum>,
    param_values: Vec<Vec<f64>>,                // per-node param values
    delay_buffers: Vec<FixedBuffer<T, BUF>>,     // feedback delay slots
    current_tick: ClockTick,
}

impl<T: Transcendental, const BUF: usize> RillGraphEngine<T, BUF> {
    pub fn new(
        schedule: ScheduledGraph,
        programs: Vec<RillProgram<T>>,
        system: &ActorSystem,
    ) -> Self;

    pub fn handle(&self) -> ActorRef<CommandEnum>;
    pub fn tick(&self) -> ClockTick;
}

impl<T: Transcendental, const BUF: usize> Algorithm<T> for RillGraphEngine<T, BUF> {
    fn process(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
    ) -> ProcessResult<()> {
        // 1. Drain actor mailbox → update param_values
        self.actor.drain(|cmd| self.apply_command(cmd));

        // 2. Write input samples to input buffers
        // 3. Execute steps sequentially — buffers are mutated in-place:
        //    - Zero-copy: input_bufs[k] of step N aliases output_bufs[j] of step N-1
        //    - Explicit copy: BufferCopy steps only for fan-out/fan-in edges
        //    - Feedback: ReadDelay fills buffer from delay slot, WriteDelay saves to slot
        for step in &self.schedule.steps {
            match step {
                Step::ReadDelay { slot, target } => {
                    self.buffers[*target].copy_from_slice(&self.delay_buffers[*slot]);
                }
                Step::InlineProgram { node_idx, input_bufs, output_bufs, param_indices } => {
                    let prog = &mut self.programs[*node_idx];
                    for (pi, pidx) in param_indices.iter().enumerate() {
                        prog.set_param(pi, self.param_values[*node_idx][*pidx]);
                    }
                    // prog reads directly from buffer pool slices (may alias upstream outputs)
                    // prog writes directly to buffer pool slices (may alias downstream inputs)
                    // No buffer copies for 1:1 edges — data is mutated in place.
                    prog.process_block_from_buffers(
                        &mut self.buffers, input_bufs, output_bufs,
                    );
                }
                Step::BufferCopy { from, to, gain, add } => {
                    if *add {
                        // Fan-in: accumulate
                        for (dst, src) in self.buffers[*to].iter_mut()
                            .zip(self.buffers[*from].iter()) {
                            *dst += *src * gain;
                        }
                    } else {
                        // Fan-out: copy to each branch
                        self.buffers[*to].copy_from_slice(&self.buffers[*from]);
                        if (*gain - 1.0).abs() > f32::EPSILON {
                            for v in &mut self.buffers[*to] { *v *= *gain; }
                        }
                    }
                }
                Step::WriteDelay { source, slot } => {
                    self.delay_buffers[*slot].copy_from_slice(&self.buffers[*source]);
                }
            }
        }
        // 4. Copy output buffers to output slice
    }
}
```

### Actor protocol

`SetParameter` messages use string anchors instead of `PortId`:

```rust
// rill-patchbay sends:
CommandEnum::SetParameter {
    anchor: "myFilter",       // matches `param myFilter = ...`
    param: "cutoff",
    value: ParamValue::F32(0.5),
}
```

The engine routes the message to the corresponding program's param slot. No `PortId` — the engine manages internal routing.

## Integration Points

### `rill-lang` public API additions

```rust
pub fn compile_graph<T: Transcendental>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
    block_size: usize,
    system: &ActorSystem,
) -> Result<RillGraphEngine<T, BLOCK_SIZE>, CompileError>;
```

### `rill-adrift` changes

`LangNode` gains support for `RillGraphEngine`:

```rust
// Registration:
factory.register_fn("rill/graph_lang", |id, params| {
    let source = params.get_str("source").unwrap();
    let engine = rill_lang::compile_graph(source, ...)?;
    // Wrap in NodeVariant::Processor or NodeVariant::Source
});
```

### `rill-patchbay` changes

`SetParameter` gains an `anchor: String` field. Existing `target_node: NodeId` path works unchanged for JSON-based graphs.

### `rill-graph` — no changes

JSON-based graphs continue to use the existing `Graph`/`ProcessingState` runtime. Both runtimes coexist.

## File Layout

### New files in `rill-lang`

| File | Purpose |
|------|---------|
| `src/graph_ir.rs` | `GraphIr`, `GraphNode`, `GraphEdge`, `EdgeKind` |
| `src/graph_build.rs` | AST → Graph IR construction |
| `src/graph_optimize.rs` | Optimizer pass runner + individual passes |
| `src/graph_lower.rs` | Graph IR → `ScheduledGraph` (topo sort, liveness, buffer alloc) |
| `src/graph_schedule.rs` | `ScheduledGraph`, `Step` |
| `src/graph_engine.rs` | `RillGraphEngine<T, BUF>` runtime |

### Modified files in `rill-lang`

| File | Changes |
|------|---------|
| `src/lexer.rs` | Tokens: `param`, `keep`, `inline` |
| `src/ast.rs` | `Expr::ParamNode { name, params, body, keep, force_inline }` |
| `src/parser.rs` | Parse `param`, `keep param`, `inline param` prefix + binding |
| `src/types/infer.rs` | Typecheck param nodes (arity inference for ports) |
| `src/lib.rs` | `compile_graph()` entry point |
| `src/serde_def.rs` | Optional: `RillGraphDef` for serialization |

### Modified files in `rill-adrift`

| File | Changes |
|------|---------|
| `src/lang_node.rs` | Detect graph source vs single-algorithm source; route accordingly |
| `src/registration.rs` | Register `rill/graph_lang` node type |

### Modified files in `rill-patchbay`

| File | Changes |
|------|---------|
| `src/engine.rs` | `SetParameter` variant with `anchor: String` |
| `src/module_def.rs` | `ServoDef.target_anchor: Option<String>` |
| `src/servo_constructor.rs` | Handle string anchors in servo construction |

## Non-Goals (v1)

- Split-chain (recording/playback) execution mode
- Shared graph resources (tape loops) — handled via built-in functions + delay rings
- `NodeFactory` references from DSL — everything is built-in functions
- Cranelift JIT backend integration
- Nested graph definitions (graphs within graphs)

## Future Extensions

- **Split-chain support**: Add `RecordingStep`/`PlaybackStep` to the schedule, wire `rill/input`/`rill/output` backends into `RillGraphEngine`
- **Resource support**: `GraphResource` allocation in engine, resolve via built-in function signatures
- **Multi-graph programs**: Multiple `process` definitions → multiple `RillGraphEngine` instances from one source file
- **Profiling annotations**: `trace param myNode = ...` for runtime performance counters
- **Stereo auto-detection heuristics**: Automatically detect stereo chains without explicit `param` annotations
