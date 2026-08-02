# MonoToStereo: Multi-Output Builtins, FixedBuffer Migration & Closure-Based Compilation

**Date:** 2026-07-14
**Status:** Draft
**Scope:** `rill-core`, `rill-lang`, `rill-router`

## Motivation

Add a `MonoToStereo` graph node (mono → stereo, constant-power pan). The current graph engine uses `Vec<Vec<T>>` for buffer storage (no SIMD alignment, double indirection, runtime `buf_size`), has heap allocations on the real-time signal path (`steps.clone()`, `buf_slices()` collecting `Vec`s, `Instr::clone()` per sample), and lacks multi-output (SITO/MIMO) support for block builtins. This work fixes the infrastructure and adds the node.

## Design Decisions

### Architecture layers

`rill-lang` is the compiler + executor: it defines `GraphIr`, compiles it to `CompiledGraph`, and executes the result. `rill-graph` is an alternative frontend for the same compiler: reads JSON graph definitions, resolves node types through the registry, builds `GraphIr` with edge connections and topological ordering. For this feature, `rill-graph` only needs one addition: a name mapping for the new node type.

### One execution path

The closure-based `CompiledGraph` **replaces** the current `ScheduledGraph`/step-based graph engine entirely. No dual-path: the old `execute_siso`, `execute_step`, `buf_slices`, and `Step` enum in `graph_engine.rs` are **removed**. The per-program interpreter (`RillProgram::process` → `run_block_hybrid`) is a separate layer (standalone DSL program execution, not graph orchestration) and stays unchanged.

The graph is compiled at construction time into a flat `Vec<NodeClosure>` — each closure captures buffer indices/pointers and owns its algorithm. Execution is a single loop with zero heap allocation:

```rust
fn process_tick(&mut self) {
    for node in &mut self.nodes {
        node.execute(&mut self.buffers);  // one virtual call per node per tick
    }
}
```

JIT rationale: builtins are pre-compiled Rust code — always FFI calls, never inlineable across the boundary regardless of orchestration layer. DSL programs (rill-lang IR) have their own JIT compilation unit — the orchestration layer doesn't affect whether the JIT can fuse them. So closure-based orchestration doesn't sacrifice JIT potential over step-based dispatch.

### No interleaved buffers

Interleaved multi-channel buffers (`[L0,R0,L1,R1,...]`) break contiguous DSP access and prevent vectorization. Every signal port maps to an independent contiguous buffer. Multi-output block builtins use `MultichannelAlgorithm<T>` receiving `&[&[T]]` / `&mut [&mut [T]]` — separate slices per channel.

### `FixedBuffer<T, BUF_SIZE>` everywhere

Replace `Vec<Vec<T>>` buffer pool with `Vec<FixedBuffer<T, BUF_SIZE>>`. `FixedBuffer` is `#[repr(align(16))] [T; BUF_SIZE]` — stack-allocated inside the Vec, SIMD-aligned, compile-time sized. The `BUF_SIZE` const generic already exists on `rill-graph::GraphBuilder`.

### Zero-copy edges preserved

Downstream nodes reuse the same buffer index as the upstream output — reading via `&[T]` without copying. No `BufferCopy` steps for signal edges. Same mechanism as current `graph_lower.rs` (buffer index sharing via `edge_buffers` map).

### Parameter changes: algorithms accessible from outside

Algorithms are stored in `NodeClosure.algo` (not captured inside a `FnMut`). The engine drains parameter changes before the tick:

```rust
for (node_idx, param_idx, value) in self.pending_params.drain(..) {
    self.nodes[node_idx].set_param(param_idx, &value);
}
```

Same `?name` semantics as current — parameters are control-rate, changed via `ActorRef<CommandEnum>`.

## Architecture

### Phase 1: `MultichannelBlockBuiltin` infrastructure (`rill-core`)

**New trait** — `rill-core/src/builtin.rs`:

```rust
pub trait MultichannelBlockBuiltin<T: Transcendental>:
    MultichannelAlgorithm<T> + Send + Sync
{
    fn set_param(&mut self, _index: usize, _value: &ParamValue) {}
}
```

**New `BuiltinInst` variant**:

```rust
pub enum BuiltinInst<T: Transcendental> {
    Sample(Box<dyn SampleBuiltin<T>>),
    Block(Box<dyn BlockBuiltin<T>>),
    MultichannelBlock(Box<dyn MultichannelBlockBuiltin<T>>),  // NEW
}
```

**New registry method**:

```rust
impl<T: Transcendental> Registry<T> {
    pub fn register_multichannel_block(
        &mut self,
        sig: BuiltinSig,
        factory: fn(params: &[f64], sample_rate: f32) -> Box<dyn MultichannelBlockBuiltin<T>>,
    );
}
```

`BuiltinSig` already supports `signal_outs > 1` — no changes needed.

### Phase 2: FixedBuffer migration (`rill-lang`)

**`FixedBuffer` type** — from `rill-core::buffer::buffer_trait`:

```rust
#[repr(align(16))]
pub struct FixedBuffer<T, const SIZE: usize> {
    data: [T; SIZE],
}
```

Methods used: `new()` (fill with `T::ZERO`), `as_ptr()`, `as_mut_ptr()`, `as_slice()`, `as_mut_slice()`, `fill(T::ZERO)`.

**`graph_lower.rs`** — replace runtime `buf_size` with `BUF_SIZE` const generic. The buffer count and edge routing logic stays identical.

**`builtins/mixer.rs`** — `bus_buffers: Vec<Vec<T>>` → `Vec<FixedBuffer<T, BUF_SIZE>>`.

### Phase 3: Closure-based graph compilation (`rill-lang`)

Replaces the current `graph_lower.rs` + `ScheduledGraph`. Produces `CompiledGraph` — the single execution target.

**Runtime types** — `rill-lang/src/graph_engine.rs`:

**`NodeClosure`** — the compiled form of one graph node:

```rust
enum AlgorithmVariant<T: Transcendental> {
    Siso(Box<dyn BlockBuiltin<T>>),
    Mimo(Box<dyn MultichannelBlockBuiltin<T>>),
}

struct NodeClosure<T: Transcendental, const BUF_SIZE: usize> {
    /// Algorithm — accessible from outside for parameter changes.
    algo: AlgorithmVariant<T>,
    /// Buffer indices into the pool. Never changes after construction.
    input_indices: Vec<usize>,
    output_indices: Vec<usize>,
    /// Scratch vecs for MIMO slice construction (SISO nodes don't need them).
    /// Pre-allocated at construction, cleared+re-filled per tick — zero allocation.
    input_slices: Vec<&'static [T]>,
    output_slices: Vec<&'static mut [T]>,
}

impl<T: Transcendental, const BUF_SIZE: usize> NodeClosure<T, BUF_SIZE> {
    fn execute(&mut self, buffers: &mut [FixedBuffer<T, BUF_SIZE>]) {
        match &mut self.algo {
            AlgorithmVariant::Siso(algo) => {
                let input = buffers[self.input_indices[0]].as_slice();
                let output = buffers[self.output_indices[0]].as_mut_slice();
                Algorithm::process(algo.as_mut(), Some(input), output).ok();
            }
            AlgorithmVariant::Mimo(algo) => {
                // Rebuild slice vecs from FixedBuffer base pointers (no allocation)
                self.input_slices.clear();
                for &idx in &self.input_indices {
                    let ptr = buffers[idx].as_ptr();
                    let slice: &[T] = unsafe { std::slice::from_raw_parts(ptr, BUF_SIZE) };
                    self.input_slices.push(unsafe { std::mem::transmute(slice) });
                }
                self.output_slices.clear();
                for &idx in &self.output_indices {
                    let ptr = buffers[idx].as_mut_ptr();
                    let slice: &mut [T] =
                        unsafe { std::slice::from_raw_parts_mut(ptr, BUF_SIZE) };
                    self.output_slices.push(unsafe { std::mem::transmute(slice) });
                }
                MultichannelAlgorithm::process(
                    algo.as_mut(),
                    &self.input_slices,
                    &mut self.output_slices,
                )
                .ok();
            }
        }
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) {
        match &mut self.algo {
            AlgorithmVariant::Siso(algo) => algo.set_param(index, value),
            AlgorithmVariant::Mimo(algo) => algo.set_param(index, value),
        }
    }
}
```

**Safety of `transmute` to `'static`**: the actual borrow is valid because `FixedBuffer` storage in `buffers` doesn't move during processing. Same unsafe assumption as current `buf_slices()` which extracts raw pointers from distinct `Vec<T>` elements.

**Compilation** — from `GraphIr` to `Vec<NodeClosure>`:

The current `graph_lower.rs` already performs buffer index assignment and edge routing. The new `compile()` function reuses this logic but produces `NodeClosure`s instead of `Step`s:

1. Allocate `Vec<FixedBuffer>` for the buffer pool
2. For each node in topo order:
   - Determine `input_indices` from `edge_buffers` (same zero-copy sharing as current)
   - Allocate new buffer indices for each output port
   - Instantiate the algorithm via `Registry` factory
   - Create `NodeClosure` with pre-allocated scratch vecs (if MIMO)
3. Return `CompiledGraph { buffers, nodes }`

### Phase 4: `CompiledGraph` engine — the single execution path (`rill-lang`)

```rust
struct CompiledGraph<T: Transcendental, const BUF_SIZE: usize> {
    buffers: Vec<FixedBuffer<T, BUF_SIZE>>,
    nodes: Vec<NodeClosure<T, BUF_SIZE>>,
    // Parameter changes queued by control thread, drained before each tick
    pending_params: Vec<(usize, usize, ParamValue)>,
}

impl<T: Transcendental, const BUF_SIZE: usize> CompiledGraph<T, BUF_SIZE> {
    fn process_tick(&mut self) {
        // Phase 0: apply queued parameter changes
        for (node_idx, param_idx, value) in self.pending_params.drain(..) {
            self.nodes[node_idx].set_param(param_idx, &value);
        }
        // Phase 1: execute all nodes (topological order guaranteed by compilation)
        for node in &mut self.nodes {
            node.execute(&mut self.buffers);
        }
    }
}
```

**Zero allocations on RT path confirmed:**
- `Vec::drain(..)` — O(1), no allocation (moves elements out, sets len=0)
- `NodeClosure::execute()` — `Vec::clear()` + `push()` with pre-allocated capacity, no allocation
- No `steps.clone()`, no `buf_slices()` collecting, no `Instr::clone()`

**What the old per-program interpreter (`RillProgram`, `run_block_hybrid`) still does:**
- Continues to work for standalone DSL programs (non-graph usage)
- Not used in the graph engine path after this change
- The `steps.clone()` and `Instr::clone()` in that path remain as-is (non-graph path, lower priority)

### Phase 5: `MonoToStereo` node (`rill-router`)

**New module:** `rill-router/src/pan.rs`

```rust
/// Pan law — determines gain distribution between left and right.
pub enum PanLaw {
    /// sqrt(2)/2 * (cos(θ) + sin(θ)) per channel, constant perceived loudness.
    ConstantPower,
    /// Linear fade: left = 1-pan, right = pan. -3 dB dip at center.
    Linear,
}

impl PanLaw {
    /// Compute (left_gain, right_gain) for pan in [-1.0, 1.0].
    /// -1.0 = full left, 0.0 = center, 1.0 = full right.
    pub fn gains(pan: f32) -> (f32, f32) {
        let pan = pan.clamp(-1.0, 1.0);
        match self {
            PanLaw::ConstantPower => {
                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                (angle.cos(), angle.sin())
            }
            PanLaw::Linear => {
                let right = (pan + 1.0) * 0.5;
                (1.0 - right, right)
            }
        }
    }
}
```

```rust
/// Converts a mono signal to stereo with pan and exponential smoothing.
pub struct MonoToStereo<T: Transcendental> {
    pan_law: PanLaw,
    pan: f32,           // -1.0 (left) to 1.0 (right)
    smoothing: f32,     // 0.0 = instant, 1.0 = no change
    left_gain: f32,
    right_gain: f32,
}

impl<T: Transcendental> MonoToStereo<T> {
    pub fn new(pan_law: PanLaw, pan: f32, smoothing: f32) -> Self {
        let (lg, rg) = pan_law.gains(pan);
        Self { pan_law, pan, smoothing, left_gain: lg, right_gain: rg }
    }
    pub fn set_pan(&mut self, pan: f32) { self.pan = pan; }
    pub fn set_smoothing(&mut self, s: f32) { self.smoothing = s; }
}

impl<T: Transcendental> MultichannelAlgorithm<T> for MonoToStereo<T> {
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 2 }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        let mono  = inputs[0];
        let left  = outputs[0];
        let right = outputs[1];

        let (tg_l, tg_r) = self.pan_law.gains(self.pan);
        self.left_gain  += self.smoothing * (tg_l - self.left_gain);
        self.right_gain += self.smoothing * (tg_r - self.right_gain);

        let lg = T::from_f32(self.left_gain);
        let rg = T::from_f32(self.right_gain);

        for i in 0..mono.len() {
            let s = mono[i];
            left[i]  = s * lg;
            right[i] = s * rg;
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.left_gain = std::f32::consts::FRAC_1_SQRT_2;
        self.right_gain = std::f32::consts::FRAC_1_SQRT_2;
    }
}
```

Lang registration — `rill-router/src/lang.rs`:

```rust
reg.register_multichannel_block(
    BuiltinSig {
        name: "mono_to_stereo",
        params: vec![
            ParamType::Signal,
            ParamType::Float { default: Some(0.0), min: Some(-1.0), max: Some(1.0) },
            ParamType::Float { default: Some(0.1), min: Some(0.0), max: Some(1.0) },
        ],
        signal_outs: 2,
        kind: BuiltinKind::Block,
        param_names: vec!["input", "pan", "smoothing"],
    },
    |params, _sr| {
        Box::new(MonoToStereo::<T>::new(
            PanLaw::ConstantPower,
            params[0] as f32,
            params[1] as f32,
        ))
    },
);
```

Graph name mapping (`rill-graph/src/graph.rs`):

```rust
"rill/mono_to_stereo" => "mono_to_stereo",
```

## File Manifest

| File | Action | Description |
|---|---|---|
| `rill-core/src/builtin.rs` | Edit | `MultichannelBlockBuiltin` trait, `BuiltinInst::MultichannelBlock`, `register_multichannel_block` |
| `rill-core/src/lib.rs` | Edit | Re-export `MultichannelBlockBuiltin` |
| `rill-lang/src/graph_compiler.rs` | **New** | `compile(GraphIr, registry) -> CompiledGraph`: buffer allocation, NodeClosure construction, edge routing. Replaces `graph_lower.rs`. |
| `rill-lang/src/graph_engine.rs` | **Rewrite** | Replace `RillGraphEngine<T>` + `ScheduledGraph` + step-based execute entirely with `CompiledGraph<T, BUF_SIZE>` and `process_tick`. Remove `execute_siso`, `execute_step`, `execute_sub_schedule`, `buf_slices`, `Step` enum. |
| `rill-lang/src/graph_lower.rs` | **Delete** | Superseded by `graph_compiler.rs`. |
| `rill-lang/src/builtins/mixer.rs` | Edit | `bus_buffers: Vec<Vec<T>>` → `Vec<FixedBuffer<T, BUF_SIZE>>` |
| `rill-graph/src/graph.rs` | Edit | Add `"rill/mono_to_stereo"` name mapping only |
| `rill-router/src/pan.rs` | **New** | `PanLaw`, `MonoToStereo<T>`, `MultichannelAlgorithm` impl |
| `rill-router/src/lang.rs` | Edit | Register `mono_to_stereo` builtin |
| `rill-router/src/lib.rs` | Edit | `pub mod pan` |
| `rill-router/src/register.rs` | Edit | Wire `register_router_builtins` |

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| `const BUF_SIZE` propagation breaks `rill-io` callers | `rill-io` already parameterized on `BUF_SIZE` via const generics. |
| `transmute` to `'static` in scratch vecs | Same safety assumption as current `buf_slices()` — `FixedBuffer` storage is stable (Vec doesn't reallocate after construction). |
| `MultichannelBlockBuiltin` + new `BuiltinInst` variant breaks per-program interpreter | Per-program interpreter (`RillProgram::process` → `run_block_hybrid`) is a separate path. `MultichannelBlockBuiltin` builtins are graph-engine-only. |
| One virtual call per node per tick | Acceptable — typically 5–50 nodes per graph, one call per tick (not per sample). Overhead is dwarfed by DSP computation. |
| MIMO scratch vec rebuild each tick | `clear()` + `push()` on pre-allocated Vec — zero heap allocation. Only MIMO nodes incur the rebuild (rare). |

## Non-Goals

- StereoToMono — not in scope
- Refactoring `ChannelState` in mixer to reuse `PanLaw` — leave mixer unchanged
- Fan-in (N→1) — existing bug in `graph_lower.rs`, tracked separately
- Per-program interpreter (`RillProgram`, `run_block_hybrid`) RT allocations — separate path, not part of graph engine
- Debug telemetry RT allocation (`feature = "debug"`) — tracked separately
- `BufferPool` in `rill-core/src/buffer/pool.rs` — dead code
