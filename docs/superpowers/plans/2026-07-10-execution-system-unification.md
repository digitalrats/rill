# Execution System Unification — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace old graph engine (ProcessingState, Port::propagate, NodeVariant) with rill-lang IR compilation. GraphBuilder produces GraphIr → optimizer → lowerer → RillGraphEngine.

**Architecture:** 9 sequential phases. Phase 1 creates 8 missing lang built-ins. Phases 2-5 build the graph compilation pipeline (GraphIr, build_ir, optimizer, lowerer). Phase 6 enhances RillGraphEngine. Phases 7-9 delete old code and unify registration.

**Spec:** `docs/superpowers/specs/2026-07-10-execution-system-unification-design.md`

**Workspace:** `/home/mikek/Projects/digitalrats/rill`, create feature branch `feature/execution-unification`.

---

## Phase 1: Missing built-ins

### Task 1.1: biquad (general), delay, distortion, limiter

**Files:**
- Create: `rill-digital-effects/src/lang/mod.rs`
- Create: `rill-digital-effects/src/lang/delay.rs`
- Create: `rill-digital-effects/src/lang/distortion.rs`
- Create: `rill-digital-effects/src/lang/limiter.rs`
- Modify: `rill-digital-effects/src/register.rs`
- Modify: `rill-digital-filters/src/lang/biquad.rs` (add general biquad filter type param)

- [ ] **Step 1: Read existing graph node constructors for arity info**

Read `rill-digital-effects/src/register.rs` and `rill-digital-filters/src/register.rs`. Note the exact `NodeConstructor` closure bodies — they show the param order and types.

- [ ] **Step 2: Create wrapper structs + BlockBuiltin impls**

For each: wrap `Algorithm<T>` type, implement `BlockBuiltin<T>`, add to `register_lang_builtins()`.

Biquad (general):
```rust
// rill-digital-filters/src/lang/biquad.rs — extend existing BiquadBuiltin to accept filter_type param
// BuiltinSig: simple("biquad", 1, 1, 4, Block)  // signal_in=1, signal_out=1, params: type, cutoff, q, gain_db
// FilterType: LowPass=0, HighPass=1, BandPass=2, Notch=3, Peak=4, LowShelf=5, HighShelf=6
```

Delay:
```rust
// rill-digital-effects/src/lang/delay.rs
// BuiltinSig: simple("delay", 1, 1, 3, Block)  // time_ms, feedback, mix
```

Distortion:
```rust
// BuiltinSig: simple("distortion", 1, 1, 2, Block)  // drive, mix
```

Limiter:
```rust
// BuiltinSig: simple("limiter", 1, 1, 2, Block)  // threshold, release_ms
```

- [ ] **Step 3: Build, test, commit each crate**

```bash
cargo build -p rill-digital-filters -p rill-digital-effects
cargo test -p rill-digital-filters -p rill-digital-effects
git add rill-digital-filters/ rill-digital-effects/
git commit -m "feat: add lang builtins for biquad, delay, distortion, limiter"
```

---

### Task 1.2: graphic_eq, sampler, convolver, cassette_deck

- [ ] **graphic_eq** (`rill-router`): wrap `GraphicEqProcessor` as `BlockBuiltin`. Params: 0 (uses default 31-band third-octave). BuiltinSig: simple("graphic_eq", 1, 1, 0, Block). Or accept `output_gain`: simple("graphic_eq", 1, 1, 1, Block).

- [ ] **sampler** (`rill-sampler`): wrap `SamplePlayer` or equivalent. Need to check what Algorithm impl exists. BuiltinSig: likely simple("sampler", 0, 1, 1, Block) — source node.

- [ ] **convolver** (`rill-fft`): wrap `ConvolverNode`'s inner algorithm. BuiltinSig: simple("convolver", 1, 1, 1, Block). Check if `rill-fft` has an `Algorithm<T>` type.

- [ ] **cassette_deck** (`rill-analog-effects`): wrap `CassetteDeckProcessor`. BuiltinSig: simple("cassette_deck", 1, 1, 3, Block). Params: tape_speed, bias_level, noise_floor.

```bash
git add rill-router/ rill-sampler/ rill-fft/ rill-analog-effects/
git commit -m "feat: add lang builtins for graphic_eq, sampler, convolver, cassette_deck"
```

---

## Phase 2: GraphIr data model

### Task 2.1: Create GraphIr, GraphNode, GraphEdge

**Files:**
- Create: `rill-lang/src/graph_ir.rs`

- [ ] **Implement types exactly as specified** — `GraphIr`, `GraphNode`, `GraphEdge`, `EdgeKind` from the spec.

- [ ] **Build and commit**

```bash
cargo build -p rill-lang
git add rill-lang/src/graph_ir.rs
git commit -m "feat(rill-lang): add GraphIr, GraphNode, GraphEdge types"
```

---

## Phase 3: GraphBuilder::build_ir()

### Task 3.1: Add build_ir method to GraphBuilder

**Files:**
- Modify: `rill-graph/src/graph.rs`
- Modify: `rill-graph/Cargo.toml` (add optional rill-lang dep)

- [ ] **Step 1: Add rill-lang as optional dependency**

```toml
# rill-graph/Cargo.toml
rill-lang = { version = "0.5", path = "../rill-lang", optional = true }

[features]
lang = ["rill-lang"]
```

- [ ] **Step 2: Implement GraphBuilder::build_ir()**

```rust
#[cfg(feature = "lang")]
impl<T: Transcendental> GraphBuilder<T> {
    pub fn build_ir(self, registry: &rill_lang::builtin::Registry<T>) -> Result<GraphIr, BuildError> {
        // 1. For each recipe: lookup BuiltinSig, create GraphNode with pre-compiled Ir
        // 2. Convert edges to GraphEdges
        // 3. Kahn's topological sort on signal edges
        // 4. Return GraphIr
    }
}
```

The key: for each recipe, `registry.get_block(type_name)` returns a factory. Call factory with folded params to get `Box<dyn BlockBuiltin<T>>`. Store it (or pre-compile to Ir if applicable).

For now: store `Box<dyn BlockBuiltin<T>>` in each GraphNode. The optimizer/lowerer will inline them.

- [ ] **Step 3: Build and commit**

```bash
cargo build -p rill-graph --features lang
git add rill-graph/
git commit -m "feat(rill-graph): add GraphBuilder::build_ir with Registry support"
```

---

## Phase 4: Optimizer

### Task 4.1: Implement graph optimizer

**Files:**
- Create: `rill-lang/src/graph_optimize.rs`

- [ ] **Implement 5 passes**: DCE, inlining, parallel merge, lateral merge, LTI reorder. Each pass mutates `GraphIr` in-place.

- [ ] **Test each pass** with simple 2-3 node test cases.

```bash
cargo test -p rill-lang
git add rill-lang/src/graph_optimize.rs
git commit -m "feat(rill-lang): add graph optimizer (DCE, inlining, merge, LTI)"
```

---

## Phase 5: Lowerer

### Task 5.1: GraphIr → ScheduledGraph

**Files:**
- Create: `rill-lang/src/graph_lower.rs`

- [ ] **Implement**: topo sort (reuse from GraphIr), buffer liveness analysis, buffer allocation (graph coloring), ScheduledGraph construction.

- [ ] **Existing ScheduledGraph/Schedule types**: extend if needed for multi-node support.

```bash
cargo test -p rill-lang
git add rill-lang/src/graph_lower.rs
git commit -m "feat(rill-lang): add graph lowerer (GraphIr → ScheduledGraph)"
```

---

## Phase 6: RillGraphEngine

### Task 6.1: Enhance for multi-node schedules

**Files:**
- Modify: `rill-lang/src/graph_engine.rs`

- [ ] **Replace single program with schedule + buffer pool**. Execute `Step::InlineProgram`, `Step::BufferCopy`, `Step::ReadDelay`, `Step::WriteDelay` sequentially.

- [ ] **Implement MultichannelAlgorithm<T>** (already behind feature gate — make it work with scheduled graph).

```bash
cargo build -p rill-lang --features router
cargo test -p rill-lang --features router
git add rill-lang/src/graph_engine.rs
git commit -m "feat(rill-lang): enhance RillGraphEngine for multi-node schedules"
```

---

## Phase 7: Delete old engine

### Task 7.1: Remove Port, ProcessingState, NodeVariant, NodeConstructor, NodeFactory

**Files:**
- Delete/refactor: `rill-core/src/traits/port.rs` (or large sections)
- Delete/refactor: `rill-graph/src/graph.rs` (ProcessingState, Graph)
- Delete: `rill-graph/src/factory.rs`
- Delete: `rill-core/src/traits/processable.rs` (NodeVariant dispatch)

- [ ] **Step 1: Search all usages**

```bash
rg 'ProcessingState' --type rust -l
rg 'Port::propagate' --type rust -l
rg 'NodeVariant' --type rust -l
rg 'NodeFactory' --type rust -l
```

- [ ] **Step 2: Remove/refactor each**

Delete methods, types, and trait impls. Update callers to use new API.

- [ ] **Step 3: Fix IO backends**

`rill-io` backends call `ProcessingState::process_block()`. These need to call `RillGraphEngine::process()` instead (via `MultichannelAlgorithm`).

```bash
cargo build --workspace 2>&1 | head -50
# Fix all errors
cargo test -p rill-lang -p rill-core -p rill-graph
```

- [ ] **Commit**

```bash
git commit -m "refactor: remove old graph engine (Port, ProcessingState, NodeVariant, NodeFactory)"
```

---

## Phase 8: Unify registration

### Task 8.1: Delete register_graph_nodes, registration.rs

**Files:**
- Delete `register_graph_nodes()` from each DSP crate's `register.rs`
- Delete `rill-adrift/src/registration.rs` (or reduce to just `register_io` for IO backends)

- [ ] **Search for callers** and update to use `register_lang_builtins()` instead.

```bash
git add -A && git commit -m "refactor: remove register_graph_nodes, unify on register_lang_builtins"
```

---

## Phase 9: GraphDef + IO updates

### Task 9.1: Wire GraphDef to new path

**Files:**
- Modify: `rill-graph/src/serialization.rs`
- Modify: `rill-adrift/src/modular/mod.rs`

- [ ] **GraphDef::populate()** → feeds `GraphBuilder`, which now calls `build_ir()` instead of `build()`. `RillGraphEngine` replaces `ProcessingState`.

- [ ] **IO backends**: `set_process_callback` now receives `RillGraphEngine` instead of `ProcessingState`.

```bash
cargo build --workspace --features "lang,io,fft,lofi,analog,sampler"
cargo test --workspace --features "lang"
git commit -m "feat: wire GraphDef and IO to new RillGraphEngine execution"
```
