# rill-lang Graph Anchor Refactor — Implementation Plan

**Goal:** Refactor `compile_graph()` to use the correct model: `param` is a named anchor for a parameter slot inside a flat `RillProgram`, not a graph node boundary. Execution is a thin wrapper over `RillProgram` + mailbox drain + anchor→param_index map.

**Current (wrong):** Multi-node `GraphIr` → `ScheduledGraph` → `RillGraphEngine` with buffer pool
**Target (correct):** Single flat `RillProgram` → slim `RillGraphEngine` with anchor_map + mailbox

---

## Simplified Architecture

```
DSL source
  ├── parse
  ├── substitute param bodies into process
  ├── collect anchor → param_name → param_def mappings
  │
  ▼
compile_with() → flat RillProgram
  │
  ▼
RillGraphEngine { program, anchor_map, mailbox }
  │
  process():
    1. drain mailbox → anchor+param_name → index → set_param(idx, value)
    2. program.process(input, output)
```

---

### Task 1: Simplify RillGraphEngine to thin wrapper

**Files:**
- Modify: `rill-lang/src/graph_engine.rs`

Replace the multi-program engine with a single-program wrapper:

```rust
pub struct RillGraphEngine<T: Transcendental> {
    program: RillProgram<T>,
    /// anchor_name → { param_name → param_index }
    anchor_map: HashMap<String, HashMap<String, usize>>,
    mailbox: Arc<Mailbox<CommandEnum>>,
}
```

Method changes:
- `process()`: drain mailbox → lookup anchor→param_name→index → `program.set_param(idx, val)` → `program.process(input, output)`
- `reset()`: delegate to `program.reset()`
- Remove: `buffers`, `delay_buffers`, `schedule`, `programs` (vec), all step execution logic

### Task 2: Simplify compile_graph() — inline param bodies, build anchor_map

**Files:**
- Modify: `rill-lang/src/lib.rs`

New flow:
1. Parse → Def map
2. For each `param`/`keep param`/`inline param` def: collect (anchor_name, param_names → slots) but do NOT create GraphIr
3. Substitute param body expressions into `process` body (replace `Ref(name)` with the body of the matching def)
4. Compile the resulting single expression via `compile_with()`
5. After compilation, build anchor_map by matching `param()` call strings to anchor names
6. Wrap in slim RillGraphEngine

The substitution step needs to:
- Walk the `process` body expression
- When encountering `Ref(name)` that matches a param/keyword def, replace with the def's body
- Recurse into the substituted body (param defs may reference each other)

### Task 3: Delete unused graph infrastructure

**Files:**
- Delete: `rill-lang/src/graph_ir.rs`
- Delete: `rill-lang/src/graph_build.rs`
- Delete: `rill-lang/src/graph_optimize.rs`
- Delete: `rill-lang/src/graph_lower.rs`
- Delete: `rill-lang/src/graph_schedule.rs`
- Modify: `rill-lang/src/lib.rs` (remove `pub mod` declarations)
- Delete: `rill-lang/tests/graph_build.rs`
- Delete: `rill-lang/tests/graph_optimize.rs`

### Task 4: Update graph_compile test

**Files:**
- Modify: `rill-lang/tests/graph_compile.rs`

Write test that verifies:
1. `param` body is inlined — `"keep param kf = _ * 0.5; process = _ : kf : _;"` compiles and produces correct output
2. Anchor map is built — `param("gain", 0.5)` inside a param node is addressable by anchor+param name
3. GraphSetParameter correctly changes program output
4. Multiple param defs produce single flat program

### Task 5: Clean up rill-adrift GraphLangNode

**Files:**
- Modify: `rill-adrift/src/lang_node.rs` — simplify `GraphLangNode` to use the new slim engine
- Update: `rill-adrift/tests/graph_dsl_integration.rs` — adjust to new API

### Task 6: Clippy + fmt + workspace test pass
