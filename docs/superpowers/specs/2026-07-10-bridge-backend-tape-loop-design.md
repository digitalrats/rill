# Bridge Backend + `?name` Actor Parameters — Design

> **Status:** Design — awaiting user review, then implementation plan.
> **Date:** 2026-07-10
> **Scope:** Replace split-chain BFS detection with explicit bridge nodes. Unify write_head, read_heads, and tape_loop into a single `tape_bridge` node with decorator chains. Introduce `?name` syntax for actor-provided parameters (late-binding via engine mailbox). Simplify SetParameter addressing from `PortId` to `anchor: String`.

## Motivation

**Bridge backend**: The current split-chain architecture detects recording/playback chains via BFS from graph roots. This is fragile. Bridge nodes with `is_bridge = true` make the boundary explicit. One bridge per graph, N inputs, M outputs, internal state across callbacks.

**Tape unification**: `write_head` + `TapeLoop` + `read_head`s are a single logical unit — a tape delay with multiple taps. Unifying them into `tape_bridge` with per-head configuration (position, gain, decorator chains) reduces graph complexity and makes the duplex boundary explicit.

**`?name` parameters**: The current parameter model requires all modulatable values to be formal parameters of `main` or `param` definitions, producing large argument lists. `?name=default` is a late-binding slot — the value comes from the engine's actor mailbox at runtime, resolved after `drain()`. This eliminates argument list bloat and naturally supports hierarchically-named parameters.

**Simplified SetParameter**: The old `PortId (NodeId, port_index)` addressing is replaced by `anchor: String` + `param: String`. The engine routes based on anchor (node name in GraphIr) + param name (flat with dot separator convention).

## Confirmed decisions

| Dimension | Decision |
|---|---|
| **Bridge detection** | Explicit: `GraphNode.is_bridge = true`. No BFS heuristics |
| **Bridge count** | One per graph |
| **Feedback** | Node annotations: `feedback_read`/`feedback_write` with named buffers. Shadow copy for 1-tick delay. Global pool, per-node read/write |
| **Duplex** | `process_left()` in input callback, `process_right()` in output callback |
| **Tape unification** | `write_head` + `TapeLoop` + `read_head`s → `tape_bridge` node |
| **Decorators** | Per-head list of `signal → signal` expressions. Compiled to `Algorithm<T>` chain |
| **Decorator params** | `?name=default` — late-binding actor parameters, auto-prefixed by compiler |
| **SetParameter** | `{ anchor: String, param: String, value: ParamValue }` — no PortId |
| **Parameter range** | Built-ins: from `BuiltinSig`. UDF: `-inf..inf`. MVP: silent clamp on OOB |
| **Compositing** | Same `?name` in same scope → shared slot. Different scopes → unique prefixed names |

## Architecture

### GraphIr + bridge

```rust
pub struct GraphNode {
    pub arity: (usize, usize),
    pub ir: Ir,
    pub params: Vec<ParamDef>,
    pub keep: bool,
    pub inline: bool,
    /// Splits the graph into left/right sub-graphs.
    pub is_bridge: bool,
    /// Named feedback buffers to read BEFORE processing (mixed into input signal).
    pub feedback_read: Vec<String>,
    /// Named feedback buffers to write AFTER processing (capture output).
    pub feedback_write: Vec<String>,
}
```

### BridgeAlgorithm trait (`rill-core`)

```rust
/// A graph node that serves as a duplex boundary.
/// Feedback is handled externally via feedback_read/write annotations — not by the bridge.
pub trait BridgeAlgorithm<T: Transcendental>: Send {
    fn num_inputs(&self) -> usize;
    fn num_outputs(&self) -> usize;

    /// Input callback: write into bridge.
    fn process_left(&mut self, inputs: &[&[T]]) -> ProcessResult<()>;
    /// Output callback: read from bridge.
    fn process_right(&mut self, outputs: &mut [&mut [T]]) -> ProcessResult<()>;

    fn reset(&mut self);
}
```

## Feedback: graph-global named buffers with shadow copy

### Model

Feedback is NOT edges between nodes. It's node-level annotations:

- `feedback_read: Vec<String>` — named buffers whose content is **mixed into the input signal before processing**
- `feedback_write: Vec<String>` — named buffers that are **filled with the output signal after processing**

Buffers are graph-global — multiple nodes can read/write the same named buffer.

### Shadow copy for 1-tick delay

To guarantee a 1-tick delay between write and read (preventing instantaneous feedback loops), each buffer has two halves:

```
write_buf[name] — where WriteFeedback stores the current output
read_buf[name]  — where ReadFeedback loads from (previous tick's value)
```

At the **end of each tick**, `read_buf ← write_buf` (pointer swap or copy). This guarantees:

- **Same-tick write → next-tick read**: output stored at the end of tick N becomes available for input mixing at the start of tick N+1.
- **Parallel processing**: multiple feedback buffers per node are processed independently.
- **Order independence**: works regardless of whether read/write nodes are in the same sub-graph or cross-chain.

### Execution order (one tick)

```
1. ReadFeedback (ALL nodes, left + right):
   for each node with feedback_read:
       mix read_buf[name] into node's input signal

2. process_left (left sub-graph + bridge.process_left)

3. process_right (bridge.process_right + right sub-graph)

4. WriteFeedback (ALL nodes, left + right):
   for each node with feedback_write:
       write node's output into write_buf[name]

5. Shadow copy:
   for each feedback buffer name:
       read_buf[name] ← write_buf[name]
```

### Phase 4 executed AFTER process_right even for left-side WriteFeedback nodes. This means ALL writes (left or right) are visible next tick. The 1-tick delay is uniform across the entire graph.

### DuplexSchedule

```rust
pub struct DuplexSchedule {
    pub left: ScheduledGraph,
    pub right: ScheduledGraph,
    pub bridge: Box<dyn BridgeAlgorithm<f32>>,
    /// All unique feedback buffer names across the graph.
    pub feedback_names: Vec<String>,
    pub anchor: String,
    pub anchor_params: HashMap<String, usize>,
}
```

### RillGraphEngine

```rust
pub struct RillGraphEngine<T: Transcendental> {
    schedule: DuplexSchedule,
    left_engine: EngineInner<T>,
    right_engine: EngineInner<T>,
    /// Shadow buffers: read_buf[name_idx], write_buf[name_idx].
    feedback_read: Vec<Vec<T>>,
    feedback_write: Vec<Vec<T>>,
    name_to_idx: HashMap<String, usize>,
    param_map: HashMap<String, HashMap<String, f64>>,
}

impl<T: Transcendental> RillGraphEngine<T> {
    pub fn process_tick(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.drain_mailbox();

        // Phase 1: ReadFeedback — mix read_buf into node inputs
        self.apply_read_feedback();

        // Phase 2: process_left
        self.left_engine.execute_steps(&self.param_map);
        self.schedule.bridge.process_left(inputs)?;

        // Phase 3: process_right
        self.schedule.bridge.process_right(outputs)?;
        self.right_engine.execute_steps(&self.param_map);

        // Phase 4: WriteFeedback — capture node outputs into write_buf
        self.capture_write_feedback();

        // Phase 5: Shadow copy — read_buf ← write_buf
        for (name, &idx) in &self.name_to_idx {
            self.feedback_read[idx].copy_from_slice(&self.feedback_write[idx]);
        }

        Ok(())
    }
}
```

### Lowerer: extracting feedback annotations

For each `GraphNode`:

1. `feedback_read: Vec<String>` → insert `Step::ReadFeedback { name, target_bufs }` BEFORE the node's InlineProgram step in the schedule
2. `feedback_write: Vec<String>` → insert `Step::WriteFeedback { name, source_bufs }` AFTER the node's InlineProgram step
3. Collect all unique names → allocate `feedback_names` in DuplexSchedule

### EdgeKind (simplified)

```rust
pub enum EdgeKind {
    Signal,
    Feedback,  // ~ combinator — same-chain 1-sample delay (unchanged)
}
```

No `WriteFeedback`/`ReadFeedback` variants — feedback is node annotations, not edges.

## `?name` — actor-provided parameter reference

### Syntax

```rill
main = _ : lofi ?bitdepth=8 ?sr=44100 ?drywet=0.5 ?gain=1.0
```

`?name` — late-binding slot, value from engine mailbox. `?name=default` — fallback if no SetParameter received.

### AST

```rust
pub enum Expr {
    // ... existing variants ...
    /// Late-binding actor parameter: `?name` or `?name=default`.
    ActorParam {
        name: String,
        default: Option<Box<Expr>>,
        span: Span,
    },
}
```

Not affected by beta-reduction — stays as-is through `reduce.rs`.

### Composit naming

Compiler auto-prefixes `?name` based on nesting context:

```rill
tape_bridge {
    heads: {
        tap3: {
            decorators: [_ : lofi ?bitdepth=8 ?sr=44100],
        },
    },
}
```

→ `tap3.decorator[0].bitdepth`, `tap3.decorator[0].sr`

Generated param: `ParamDef { name: "tap3.decorator[0].bitdepth", default: 8.0, min: -inf, max: inf }`.

### Type checker

Validate as constant `(0→1)` expression — does not contribute to signal arity.

### Lowerer

`ActorParam { name, default }` → `intern_param(prefixed_name, default_val, min, max)` → `Instr::ReadActorParam { dst, param_idx }`.

`ReadActorParam` at runtime: `values[param_idx]` where `values` is populated from `param_map[anchor]`.

## SetParameter — simplified addressing

### Old

```rust
SetParameter {
    port: PortId { node: NodeId(u16), port: u16 },
    param: String,
    value: ParamValue,
    sample_pos: Option<usize>,
}
```

### New

```rust
SetParameter {
    anchor: String,       // node name in GraphIr (e.g., "tape_bridge", "myFilter")
    param: String,        // flat param name (e.g., "tap3.decorator[0].bitdepth")
    value: ParamValue,
    sample_pos: Option<usize>,
}
```

`anchor` matches the `param` keyword name or the GraphIr node name. `param` is the flat prefixed name.

## TapeBridgeAlgorithm

### Config record

```rill
tape_bridge {
    capacity: 96000,          // tape capacity in samples
    heads: {
        write: {
            position: 0,      // always 0
            gain: 0.8,
            decorators: [],
        },
        tap1: {
            position: 15840,
            gain: 0.7,
            decorators: [],
        },
        tap2: {
            position: 31680,
            gain: 0.6,
            decorators: [],
        },
        tap3: {
            position: 48000,
            gain: 0.5,
            decorators: [_ : lofi ?bitdepth=8 ?sr=44100 ?mix=0.5],
        },
    },
}
```

### Signal routing

- **Inputs**: 1 (mono) or 2 (stereo) — signal to record
- **Outputs**: `N_heads × 2` (stereo per read head)
- **Feedback inputs**: K — mixed with signal before writing

### Decorator chains

Each `decorators: [...]` is a list of `(1→1)` signal expressions. Applied sequentially:

- **Write head**: input → decorator[0] → ... → decorator[N] → tape
- **Read head**: tape → decorator[0] → ... → decorator[N] → output

Compiled to `Vec<Box<dyn Algorithm<T>>>`. Each decorator's `?name` params become bridge params with head prefix.

## Moonlight_delay: before → after

### Before (14 nodes)
```
input → stereo_sum → write_head ──tape── read_head_1,2,3 → tap_mixer → colorL/R → mixL/R → output
         stereo_sum → mixL(dry), mixR(dry)
         colorL → fb_crush → fb_lp ═══Feedback═══→ write_head(feedback_in)
```

### After (~8 nodes)
```

bridge: tape_bridge {
    capacity: 96000,
    heads: {
        write: { position: 0,     gain: 0.8 },
        tap1:  { position: 15840, gain: 0.7 },
        tap2:  { position: 31680, gain: 0.6 },
        tap3:  { position: 48000, gain: 0.5 },
    },
}

LEFT:
  input → stereo_sum → tape_bridge(signal)    // tape_bridge: feedback_read=["fb"]

RIGHT:
  tape_bridge(tap1..3) → tap_mixer → color → mix → output
                          tap_mixer → fb_crush → fb_lp   // fb_lp: feedback_write=["fb"]
```

fb_lp имеет `feedback_write=["fb"]` — после обработки его выход пишется в буфер `"fb"`. tape_bridge имеет `feedback_read=["fb"]` — перед обработкой содержимое `"fb"` (предыдущего тика) подмешивается к сигнальному входу. Shadow copy делает задержку ровно в 1 тик.
## What gets deleted

| Deleted | Reason |
|---------|--------|
| `rill/write_head`, `rill/read_head` node types | Replaced by `tape_bridge` |
| `GraphResource::TapeLoop` | Internal to `tape_bridge` |
| BFS chain detection (recording_set/playback_set) | Explicit `is_bridge` marker |
| `p_forward`, `p_pull`, `p_process_branch` | `process_left/right` |
| `Port::snapshot_feedback`, `Port::pre_process` | `feedback_read`/`feedback_write` annotations + shadow copy |
| `feedback_ptrs` on Port | ReadFeedback/WriteFeedback steps in ScheduledGraph |
| `PortId`, `NodeId` in SetParameter | `anchor: String` |

## Implementation phases

| # | Phase | Scope |
|---|-------|-------|
| 1 | **`?name` AST + type checker** | `Expr::ActorParam`, validation in `infer_apply`, composit name generation |
| 2 | **`?name` lowerer + runtime** | `Instr::ReadActorParam`, `RillGraphEngine::param_map`, `drain()` → anchor→param→value |
| 3 | **Simplified SetParameter** | `PortId` → `anchor: String`. Update `rill-patchbay` callers |
| 4 | **Feedback annotations** | `feedback_read`/`feedback_write` on `GraphNode`. Shadow buffers in engine. ReadFeedback/WriteFeedback steps in schedule |
| 5 | **BridgeAlgorithm trait** | `rill-core/src/traits/bridge.rs` — no feedback params, pure duplex |
| 6 | **GraphIR bridge support** | `is_bridge` on `GraphNode` |
| 7 | **Lowerer duplex split** | `DuplexSchedule`, left/right sub-graphs, feedback step insertion |
| 8 | **RillGraphEngine 5-phase tick** | ReadFeedback → process_left → process_right → WriteFeedback → shadow copy |
| 9 | **TapeBridgeAlgorithm** | Unified write/tape/read with head config + decorators |
| 10 | **Delete old split-chain + tape nodes** | Remove write_head, read_head, TapeLoop, BFS detection |
| 11 | **Migrate moonlight_delay** | Rewrite graph JSON to use `tape_bridge` with `feedback_read`/`feedback_write` |
