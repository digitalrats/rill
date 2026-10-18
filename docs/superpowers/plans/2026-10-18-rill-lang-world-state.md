# rill-lang: WorldState as the single execution state — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split rill-lang's object model into a compiled `Ir` (pure code) and a living `WorldState` (all execution state), so a world is externally creatable/observable, `main : IO ()` receives initial state from outside, `set_param` works between runs, and `test_eval` runs a compiled IR against a world.

**Architecture:** Move every runtime field off `RillProgram` into `WorldState<T, BUF>`; `RillProgram` shrinks to `{ ir, world }` and is then deleted. Execution functions take `(ir: &Ir, world: &mut WorldState)`. A `seat(&mut self, ir)` operation assembles the world's structural tables from exact IR layout — including the arena, sized from `ir.value_state.capacity` — plus caller-supplied `WorldConfig` (growable policy, FFI registry owned by value, `rill_core::Params`). Params move to `world.params: Params` addressed **by name** (`set_param(name, …)`), with `?sample_rate` living in `Params::sample_rate` and `sample_rate` leaving the compiler/API entirely (runner pre-seeds the world). `test_eval(ir, &mut world)` seats+runs; `ProgramRunner { ir, world }` runs `main : IO ()` with a compile-time gate; `RillProgram` and the legacy builtin `Registry`/graph path are deleted.

**Tech Stack:** Rust, `rill-lang` (~18-crate workspace), `rill-core` (`Params`/`ParamValue`/`ParameterWrite`, `Arena`/`ChunkedVec`, `FixedBuffer`), git-flow (`feature/rill-lang-arrays`).

**Spec:** `docs/superpowers/specs/2026-10-18-rill-lang-world-state-design.md` (authoritative). Ancestors: `2026-10-17-rill-lang-io-state-world-streams-design.md`, `2026-10-18-rill-lang-single-run-design.md`. This plan supersedes Tasks 4–14 of `docs/superpowers/plans/2026-10-18-rill-lang-single-run-main-io.md` (in-flight; Tasks 1–3 committed, task 3 working-tree changes finalized in Task 0 below).

## Global Constraints

- Gate for every task: `cargo test -p rill-lang --features dsp` green; `cargo test -p rill-adrift` green (lanes that touch it); `cargo clippy -p rill-lang --lib --tests` zero warnings; `cargo fmt --check`. Commits on `feature/rill-lang-arrays` in **single quotes**.
- `main : IO ()` stays a **static compile-time gate** on the compile entry (`"main"`), not a runtime latch: entry named `main` whose type is not `IO ()` is a compile error. The eval surface never selects `main` as an entry (internal marker `__rill_eval`); `main` in `defs` is a plain binding.
- `RillProgram` is **removed**. The executable surface: `compile<T, BUF>(src, ffi) -> Result<ProgramRunner<T, BUF>, CompileError>` (Complete default, gate on `ir.main_io_unit`); `ProgramRunner { ir: Ir, world: WorldState<T, BUF> }`; `compile_eval(defs, expr, mode) -> Result<Ir, CompileError>`; `test_eval(ir, &mut world) -> Result<(), CompileError>` (seat + run; a run `ProcessError` maps to `CompileError::Unsupported`).
- **`sample_rate` leaves the compiler and the API.** No `sample_rate` parameter on `compile`/`compile_eval`/lower lowering functions. `?sample_rate` reads `world.params.sample_rate` at run time; a caller needing a rate pre-seeds the world (`world.params.sample_rate = 48_000.0`) before `run()`/`test_eval`. `smooth` computes its coefficient at run time from the world rate (a `ms <= 0 || sr <= 0` guard yields identity `a = 1.0`).
- **Params addressed by name.** `world.params: rill_core::Params` (field `sample_rate: f32` + `parameters: HashMap<String, ParamValue>`). `set_param(name, value)` clamps via the seated `ParamDef` range, writes the world map by name, marks the IR-index dirt. `ValueReadParam { idx }` keeps its IR index; run-time reads resolve the name (`ir.params[idx].name == "sample_rate"` → the field, else the map). Seat seeds declared names **only if absent** (external pre-seed survives = initial-state injection) and never seeds `"sample_rate"` (field owns it).
- **The FFI registry is owned by the world** (`WorldConfig { ffi: ForeignRegistry<T>, .. }`). Built-ins resolve at seat time from `world.ffi`; an unresolvable name is a `CompileError` at seat. The legacy builtin `Registry<T>` (`src/builtin.rs`) and `graph/` are deleted once consumers are gone (final task).
- **Arena sized from the compiled IR.** The world (via `seat`) allocates the
  value arena with exactly `Arena::with_capacity(ir.value_state.capacity)` —
  the IR's build-time capacity bound, computed during lowering. The page door
  (`ChunkedVec`) appends pages only when a run exceeds that bound. The
  `growable-arena` cargo feature becomes the world's `growable: bool`
  memory-policy flag. No storage size comes from world config — structural
  tables (`value_regs`, `block_state`, `buffers`, `tapes`, `delays`,
  `value_state`) are seeded from exact IR layout, and the arena's initial size
  comes from `ir.value_state.capacity`.
- **Test-deletion/migration policy (user directive, 2026-10-18):** no compat shims. Class 1 — mechanical migration onto the new API in the same commit it breaks; Class 2 — rewrite to compile, then `#[ignore = "…; re-expressed in task N"]` or delete with a ledger record. Every ignored/deleted test is recorded in the ledger.
- rill language conventions: English docs/comments, "automatons" not "automata", no blanket `#[allow]`, no new external crates. `param_index` is deleted (name addressing only); commit messages in single quotes; no `main_fragment`/`eval_run`/`run_tick` phrasing in commit messages.
- Channel-collapse is **not** in scope (deferred): `Ir::num_inputs`/`num_outputs`/`output_channels`/`has_value_out`/`outs` stay put.

## Review Focus

1. **Non-`IO ()` mains are ill-typed at the compile front-end** — `compile::<f32, 256>("main = 1.0 + 2.0", ffi)` returns `Err` (the gate on the compile entry via `ir.main_io_unit`); `main = do { write … stdout }` and effect-final mains still run to completion with the `IO ()` payload contract. Pinned in Task 5 (`tests/compile_gate.rs`).
2. **`?sample_rate` and `smooth` must honor the world rate** — `smooth (square 0.0 1.0 0.0) 5.0` at `world.params.sample_rate = 48_000.0` produces the same coefficient as the baked constant did at 48 kHz; with `sample_rate = 0.0` (a world seeded by a non-signal program) `smooth` degenerates to identity instead of dividing by zero. Pinned in Task 3 (`tests/smooth.rs`).
3. **Seat leaves pre-seeded params untouched (initial-state injection)** — `world.set_param("level", 0.5)` or a pre-seeded `Params` entry before `test_eval`/`run` survives the seat (the program's run reads 0.5, not the declaration default). Pinned in Task 3 (`tests/param_seed.rs`) and Task 5 (`tests/compile_gate.rs` runner path).
4. **Repeated runs in one world neither exhaust the arena nor lose params** — running a program twice in the same world succeeds (arena sized from the IR; the page door covers runs that outgrow the bound), and `set_param("cutoff", 0.3)` between the runs is what the second run reads (buffers/builtins re-seat, params are absent-only seeded). Pinned in Task 3 (`tests/param_seed.rs` multi-run) and Task 5 runner.
5. **`copy_buffer` on a non-buffer ref returns `false` (not panic)** and **unknown built-in names fail at seat time with a `CompileError`** (no registry at compile time anymore). Pinned in Task 4/5.

---

## File Structure

**`src/program.rs`** — `WorldState<T, const BUF>` becomes the single runtime state (all fields that are on `RillProgram` today: arena, value registers/scratch, block state, delays, buffers, block_len, builtin instances, params store, value state, cell stack, frag cells, drops scratch, `main` result ref, value_error, ST regions/cells, streams/stdout/stderr) plus `ffi: ForeignRegistry<T>` and `params: rill_core::Params`; `WorldConfig<T>`; `seat(&mut self, ir: &Ir)`; the world accessors (`result`, `arena`, `copy_buffer`, `set_param(name,…)`, `param(name)`, `params_meta`, `inject_stdin`, `init`, `reset`, `swap_block_state`, `clear_value_regs`). `RillProgram` starts as `{ ir, world }` delegating every method, then is deleted.

**`src/backend/interp.rs`** — all functions move to `(ir: &Ir, world: &mut WorldState<T, BUF>)` signatures: `exec_entry`, `run_main_io`, `exec_value_instr`, `force`, `apply_callable`, `run_call`, `run_blocks`, effect/builtin helpers. `push_builtin_params` reads values from the world `Params` by name; `ValueReadParam` resolves via `world.params`; a new instruction `ValueReadSampleRate { dst }` and buffer-coefficient instruction serve `smooth`.

**`src/ir.rs`** — add `ValueInstr::ValueReadSampleRate { dst: usize }` (and the smooth coefficient fill described in Task 3 if lowering chooses it); nothing else changes structurally.

**`src/lower.rs`** — drop the `sample_rate` parameter from every lowering entry (`lower_with_cafs_mode_entry`, `lower_program_with_channels`, `lower_with*`, the `LowerCtx::sample_rate` field) and the two `smooth` bake sites (`1944`, `6598`): emit a runtime-read coefficient. `intern_param` unchanged (declaration + conflict error stays compile-time).

**`src/lib.rs`** — `test_eval(defs, expr, ffi)` → `compile_eval`; `compile_eval(defs, expr, mode) -> Result<Ir, CompileError>`; `test_eval(ir, &mut world) -> Result<(), CompileError>`; `compile<T, BUF>(src, ffi: ForeignRegistry<T>) -> Result<ProgramRunner<T, BUF>, CompileError>` with the `IO ()` gate; `compile_runner`/`compile_with*`/`compile_source_to_program` deleted. Prelude exports update.

**`src/program_runner.rs`** — `ProgramRunner { ir: Ir, world: WorldState<T, BUF> }`; `run()` = `seat` (idempotent for params) then `run_main_io(&self.ir, &mut self.world)`; `world()`/`world_mut()`; `stdout`/`stderr`/`set_stdin` delegate to the world; `run_on_stack` stays.

**`src/ffi.rs`** — unchanged shape; the world owns a `ForeignRegistry<T>` by value.

**`src/builtin.rs` / `src/register.rs`** — legacy `Registry<T>` and `register_core_dsp_builtins` deleted in the final task with their last consumers (graph, `full_registry` in rill-adrift, legacy downstream registers).

**`src/graph/`** — deleted in the final task (`compile.rs`, `builder.rs`, `partition.rs`, `reconstruct.rs`, `serialization.rs`, `spec.rs`, `mod.rs`), the `graph` feature and `graph_partition` test target removed.

**Tests** — `rill-lang/tests/common/mod.rs` world/eval helpers; migration of the value/buffer/runner suites onto the world API by groups (Tasks 4–6); `tests/compile_gate.rs`, `tests/param_seed.rs`, `tests/world_seat.rs` new; rill-adrift suites and examples migrate in Task 6.

**Downstream** — `rill-adrift` (`lang_builtins.rs`, `modular/mod.rs`, `examples/program.rs`, `tests/*`, `registration.rs` docs), `rill-fft`/`rill-lofi`/`rill-sampler` legacy register functions (final task).

---

### Task 0: Finalize the in-flight Task 3 (main is a pure `IO ()`)

**Files:**
- Modify: `rill-adrift/tests/modular_launch.rs` (2 unused-import warnings), then commit the pending Task-3 working tree.

**Interfaces:**
- Produces: a clean committed baseline (`main : IO ()` landed; `set_args`/main-cell machinery gone) that every later task starts from.

- [ ] **Step 1: Fix the two unused-import warnings in `rill-adrift/tests/modular_launch.rs`** (drop the now-unused imports introduced by the Task-3 deletion).
- [ ] **Step 2: Run the workspace gate**

Run: `cargo test -p rill-lang --features dsp && cargo test -p rill-adrift`
Expected: green. Then `cargo clippy -p rill-lang --lib --tests` (zero warnings) and `cargo fmt --check`.

- [ ] **Step 3: Grep-sweep for leftover launch-arg machinery**

Run: `grep -rn "set_args\|process_ty\|run_tick" rill-lang/src rill-lang/tests rill-adrift/src rill-adrift/tests`
Expected: no matches (or only intentional ones in docs referencing history).

- [ ] **Step 4: Commit**

```bash
git add rill-lang rill-adrift
git commit -m 'feat(rill-lang): main is a pure IO () value; drop String launch args'
```

---

### Task 1: `WorldState` absorbs all runtime state

The structural move that makes the world-outside-program possible. `WorldState<T, BUF>` grows from streams-only to the whole runtime state; `RillProgram` becomes `{ ir, world: WorldState<T, BUF> }` with every method delegating; `Rc<RefCell<…>>` indirection is removed (effects now hold `&mut prog.world` directly). Public API unchanged — all tests stay green without edits.

**Files:**
- Modify: `rill-lang/src/program.rs` (`WorldState` 140-149 → full state; `RillProgram` 184-265 → `{ ir, world }`; impl methods 539-1070 delegate; `build` 627-716 allocates into the world)
- Modify: `rill-lang/src/backend/interp.rs` (all field/borrow accesses: `prog.arena` → `prog.world.arena`, `prog.world.borrow().streams` → `prog.world.streams`, `prog.ir.X` stays `prog.ir.X`), `rill-lang/src/program_runner.rs` (`program.world.borrow()` → `program.world`)

**Interfaces:**
- Produces: `WorldState<T, const BUF>` with `pub(crate)` fields for everything `RillProgram` had, including `streams`, `stdout`, `stderr`; `pub fn new()` (fresh state, empty arena config unchanged for now). `RillProgram { ir: Ir, world: WorldState<T, BUF> }` with delegating methods: `result()`, `arena()`, `copy_buffer`, `param_index`/`set_param(idx,…)`/`param`/`params_meta`, `inject_stdin`, `init`, `reset`, `swap_block_state`, `clear_value_regs`. No public signature changes.

- [ ] **Step 1: Move the value/memory fields into `WorldState`**

`arena`, `value_regs`, `value_regs_top`, `value_state`, `cell_stack`, `frag_cells`, `frag_cells_base`, `drops_scratch`, `main`, `value_error`, `regions`, `st_cells`, `block_state`, `block_state_next`, `delays`, `buffers`, `block_len`, `builtins`, `params`, `params_dirty`, `params_meta` become `pub(crate)` fields of `WorldState<T, BUF>` (alongside the existing `streams`/`stdout`/`stderr`). `WorldState::new()` initializes them empty/zero-sized; `RillProgram::build` fills them on the world it owns.

- [ ] **Step 2: Rewrite `RillProgram` as the `{ ir, world }` delegator and `build` into the world**

`RillProgram::build` (currently `program.rs:627-716`) becomes: `let mut world = WorldState::new(); <allocations>` where every `self.X` is `world.X`. The struct keeps `ir` plus `world`. Public methods delegate (`self.world.arena()`, `self.world.set_param(…)`, …). The `Rc<RefCell<WorldState>>` field is deleted; `inject_stdin`/`reset` no longer `borrow_mut` (they hold `&mut self.world`).

- [ ] **Step 3: Update the interpreter field accesses**

In `interp.rs`, replace:
- `prog.<value/memory field>` → `prog.world.<field>` (a mechanical sweep over the ~100 hits found at `interp.rs:28-4560`);
- `prog.world.borrow()` / `prog.world.borrow_mut()` → `&prog.world` / `&mut prog.world` (the stream effect sites at `3962-4553` become direct field access);
- `prog.ir.X` stays unchanged.

Keep `program_runner.rs`'s `run()` console snapshot reading `world.stdout`/`world.stderr` directly.

- [ ] **Step 4: Full suite gate**

Run: `cargo test -p rill-lang --features dsp`
Expected: green, no test edits. Then `cargo clippy -p rill-lang --lib --tests` (zero warnings), `cargo fmt --check`.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/program.rs rill-lang/src/backend/interp.rs rill-lang/src/program_runner.rs
git commit -m 'refactor(rill-lang): WorldState owns the runtime state; RillProgram delegates'
```

---

### Task 2: `WorldConfig` + `seat` — the code-meets-memory boundary

`WorldState` gains caller-supplied config and the `seat` operation that assembles itself for an IR. The arena is sized **from the IR** (`ir.value_state.capacity`), not from world config; the page door only appends pages when a run outgrows the bound. The `growable-arena` feature becomes the world's `growable: bool` memory-policy flag. Built-in resolution, buffers, delays, tapes, and structural tables all happen in `seat`.

**Files:**
- Modify: `rill-lang/src/program.rs` (`WorldConfig<T>`, `WorldState::new(config)`, `seat(&mut self, ir, …)`; `build` → `new` + `seat`)
- Modify: `rill-lang/src/lib.rs` (`compile_source_to_program`/`test_eval` assemble via `new(config)` + `seat`), `rill-lang/tests/ffi_extended.rs` (`run_spectral`)
- Test: `rill-lang/tests/world_seat.rs` (new)

**Interfaces:**
- Consumes: the world-state move from Task 1.
- Produces:
  - `pub struct WorldConfig<T> { pub growable: bool, pub ffi: Option<ForeignRegistry<T>>, pub params: rill_core::Params }` with `Default` (growable true for the non-RT interpreter path, `ffi: None`, params default). **No sizes in the config** — the seat derives every storage size from the IR.
  - `impl<T: Transcendental, const BUF: usize> WorldState<T, BUF> { pub fn new(config: WorldConfig<T>) -> Self; pub fn seat(&mut self, ir: &Ir, foreign: &ForeignRegistry<T>, sample_rate: f32) -> Result<(), CompileError> }` — until Task 4 the seat still receives `foreign`/`sample_rate` as arguments (the world does not own the registry yet; ownership lands in Task 4); the arena is `Arena::with_capacity(ir.value_state.capacity)` (ChunkedVec's internal page is `capacity.max(1).next_power_of_two()`; append-pages only under `config.growable`), and `BufferPool::new(ir.value_state.buffer_budget, config.growable)`.
- `RillProgram::build` behavior moves wholesale into `seat`: tapes, delays, buffers, builtins (`build_builtin(bi, foreign, &mut resources, &tapes, sr, Some(&mut allocator))`), `value_regs` sized `ir.num_value_regs + ir.max_call_regs`, `block_state` per `ir.state.block_state_slots`, params defaults, pools. `RillProgram::new_with` becomes `RillProgram { ir, world: WorldState::new(config) }` + `world.seat(&ir, foreign, sr)`.

- [ ] **Step 1: Write the seat tests first**

`tests/world_seat.rs`:
- seat an `Ir` from `compile_to_ir_entry("main = 1.0", 44_100.0, Mode::Complete, "main")` into a fresh world → `world.value_regs.len() == ir.num_value_regs + ir.max_call_regs`, `world.block_state.len() == ir.state.block_state_slots`, `world.buffers.len() == ir.num_buffers`.
- arena sized from the IR: `world.arena().capacity() == ir.value_state.capacity` (the exact build-time bound from lowering — no config guess), and ChunkedVec's page is `capacity.max(1).next_power_of_two()`.
- unknown built-in name: seat of an IR with `foreign name : …` but no registration → `Err(CompileError)` at seat (running the pre-seat program must not reach effects).

- [ ] **Step 2: Implement `WorldConfig`, `WorldState::new(config)`, and `seat`**

Move `build`'s allocations into `seat`, sizing the arena and `BufferPool` from `ir.value_state` (capacity/buffer_budget) and applying only the growable policy from config. Keep `RillProgram`'s public surface (`new_with(ir, registry, sr, foreign)` can temporarily construct the default config and pass `foreign`/`sr` through to the seat) so no other callers change yet.

- [ ] **Step 3: Route `lib.rs` and `ffi_extended::run_spectral` through `new(config)` + `seat`**

`compile_source_to_program` and `test_eval` now call `RillProgram::new_with` on the same path (no semantic change). Confirm `tests/world_seat.rs` passes.

- [ ] **Step 4: Full suite gate**

Run: `cargo test -p rill-lang --features dsp`
Expected: green. Then `cargo clippy -p rill-lang --lib --tests` (zero warnings), `cargo fmt --check`.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src rill-lang/tests/world_seat.rs
git commit -m 'feat(rill-lang): WorldConfig + seat assembles a world from IR layout and external config'
```

---

### Task 3: Params by name in `world.params`; `sample_rate` moves into the world

The named-params map (`rill_core::Params`) becomes the world's param store; `?sample_rate` lives in `Params::sample_rate`; `sample_rate` leaves the lowering functions and the compiler surfaces; `smooth` computes its coefficient at run time. Public entry signatures stay shape-stable through this task (`test_eval`/`compile_runner` keep their `sample_rate` parameter; they translate it into a world seed so the lower-layer loses it without a test-diff).

**Files:**
- Modify: `rill-lang/src/program.rs` (field `params: Vec<ParamValue>` → `params: rill_core::Params`; `set_param(name,…)`/`param(name)`/`param_index` deletion, `set_param_index` moves; `params_meta` stays `Vec<ParamDef>` sourced at seat; seat seeds by name absent-only; `"sample_rate"` never seeded — field owns it)
- Modify: `rill-lang/src/backend/interp.rs` (`push_builtin_params` reads `world.params` by name; `ValueReadParam` resolves by name with the `"sample_rate"` field special case; new `ValueReadSampleRate` execution)
- Modify: `rill-lang/src/ir.rs` (add `ValueInstr::ValueReadSampleRate { dst: usize }`)
- Modify: `rill-lang/src/lower.rs` (drop `sample_rate` from `LowerCtx` and all entry signatures; `smooth` bakes at `1944`/`6598` become a runtime coefficient via `ValueReadSampleRate` + scalar `ValueDiv`/`ValueMul`/`ValueSub`/`ValueConstFloat`, or a single new buffer-coefficient instruction — either way `ms <= 0 || sr <= 0 → a = 1.0`; user-level `?sample_rate` resolves to the same field read)
- Modify: `rill-lang/src/lib.rs` (`compile_to_ir_entry`/`lower_with_cafs_mode_entry` call sites lose `sample_rate`; `test_eval`/`compile_runner` keep their public `sample_rate` and seed `world.params.sample_rate` before the run)
- Test: `rill-lang/tests/param_seed.rs` (new)

**Interfaces:**
- Produces:
  - `WorldState` accessors: `pub fn set_param(&mut self, name: &str, value: ParamValue)` (clamps against the seated `ParamDef` range; for `"sample_rate"` writes `self.params.sample_rate`); `pub fn param(&self, name: &str) -> Option<ParamValue>`; `params_meta()` stays. `set_param(idx,…)`/`param(idx)`/`param_index` deleted.
  - `ValueReadParam { dst, idx }` reads `world.params` at run time: `name = ir.params[idx].name; if name == "sample_rate" { world.params.sample_rate } else { world.params.parameters.get(name) → Float/Int }`.
  - `ValueInstr::ValueReadSampleRate { dst }` → `Value::Float(world.params.sample_rate as f64)` (used by `smooth` so `?sample_rate` and the operator share the field).
  - Seat seeds: for each `ir.params[i]` with `name != "sample_rate"`, insert into `world.params.parameters` **only if absent**; never seed `"sample_rate"`.
  - `sample_rate` removed from `lower_with_cafs_mode_entry`/`lower_program_with_channels`/`lower_with*`/`LowerCtx`; the runtime coefficient path for `smooth`.

- [ ] **Step 1: Write the param tests first**

`tests/param_seed.rs`:
- pre-seed survives: `let mut world = WorldState::new(config); world.set_param("level", ParamValue::Float(0.5)); seat(ir with `square 440.0 0.0 ?level 0.0`)` → `world.param("level") == Float(0.5)` (not the default 0.0); run via test_eval → the builtin receives 0.5 (assert through amplitude or a recorded side channel).
- absent name gets default: no pre-seed → `world.param("level") == Float(0.0)`.
- `?sample_rate`: `world.params.sample_rate = 48_000.0`; a program reading `?sample_rate` evaluates the field value (e.g. define `main`… via `test_eval` read with `arena()`).
- `set_param("sample_rate", …)` writes the field.
- `smooth` parity: `smooth (square 0.0 1.0 0.0) 5.0` at `sample_rate = 48_000.0` equals the old baked result (assert sample equality through `copy_buffer` with a fixed seed value); `sample_rate = 0.0` → identical output (identity coefficient, no NaN/inf).
- multi-run: `test_eval` twice in one world with `set_param("level", …)` between → second run observes the new value (world params persist; re-seat is absent-only).

- [ ] **Step 2: Implement the world param store and name addressing**

Replace `params: Vec<ParamValue>` + `params_dirty` with `params: rill_core::Params` + `params_dirty: Vec<bool>` (parallel to `ir.params`) + `param_names: HashMap<String, usize>` built at seat. `set_param(name,…)`/`param(name)` above. Update `push_builtin_params` and `ValueReadParam` execution. Delete `param_index`/index accessors. Add `ValueReadSampleRate` to `ir.rs` and its execution.

- [ ] **Step 3: Move `sample_rate` out of lowering and into the world**

Remove `sample_rate` from `LowerCtx` and all lowering entry signatures; translate call sites in `lib.rs`. Replace the two `smooth` bake sites with runtime coefficient emission. Keep `test_eval`/`compile_runner` public signatures (they seed `world.params.sample_rate` from their `sample_rate` argument before the run — `tests/param_seed.rs` and `tests/smooth.rs` must pass without edits).

- [ ] **Step 4: Full suite gate**

Run: `cargo test -p rill-lang --features dsp`
Expected: green (smooth/s’ params tests pass with runtime coefficient; `smooth.rs` unchanged). Then `cargo clippy -p rill-lang --lib --tests` (zero warnings), `cargo fmt --check`.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src rill-lang/tests/param_seed.rs
git commit -m 'feat(rill-lang): params addressed by name in world.params; sample_rate moves into the world'
```

---

### Task 4: Executor on `(ir, &mut world)`; explicit compile; `test_eval(ir, world)`; world-owning runner

The API break. Every interpreter function moves to `(ir: &Ir, world: &mut WorldState)`; `RillProgram` is deleted; `test_eval` becomes `test_eval(ir, &mut world)`; `compile_eval(defs, expr, mode) -> Ir`; `compile(src, ffi) -> ProgramRunner` owns `{ ir, world }` (world owns the `ForeignRegistry` by value); `run()` seeds-runs. The legacy `compile_runner`/`compile_with*` entries are deleted; tests migrate in the same commit (Class-1 mechanics, Class-2 ledger).

**Files:**
- Modify: `rill-lang/src/backend/interp.rs` (signature sweep to `(ir, world)`), `rill-lang/src/program.rs` (delete `RillProgram`; `WorldState::seat(&mut self, ir: &Ir)` gains the registry from the world now: `WorldConfig { ffi: ForeignRegistry<T>, … }` is required, `seat` resolves built-ins from `world.ffi` and reads `sample_rate` from `world.params.sample_rate`), `rill-lang/src/lib.rs` (entries), `rill-lang/src/program_runner.rs`
- Test: `rill-lang/tests/common/mod.rs` (new helpers), migration of `rill-lang/tests/*` (~29 files, per the migration rules below), `rill-lang/src/program_runner.rs` doc example, `rill-lang/README.md`, `rill-lang/src/lib.rs` doc example

**Interfaces:**
- Consumes: Tasks 1–3 (world-state, seat, name-addressed params).
- Produces:
  - `pub fn compile_eval<T: Transcendental, const BUF: usize>>(defs: &str, expr: &str, mode: Mode) -> Result<Ir, CompileError>` (no `sample_rate`).
  - `pub fn test_eval<T: Transcendental, const BUF: usize>(ir: &Ir, world: &mut WorldState<T, BUF>) -> Result<(), CompileError>` — `world.seat(ir)?` then `exec_entry(ir, world)`; run `ProcessError` → `CompileError::Unsupported("eval run failed: …")`.
  - `pub fn compile<T: Transcendental, const BUF: usize>(src: &str, ffi: ForeignRegistry<T>) -> Result<ProgramRunner<T, BUF>, CompileError>` — Complete default (pragma overrides); after lowering, `if !ir.main_io_unit { return Err("`main` must have type `IO ()`") }`; constructs `WorldState::new(WorldConfig { ffi, ..Default::default() })` and returns `ProgramRunner { ir, world }`.
  - `ProgramRunner { ir: Ir, world: WorldState<T, BUF> }`; `run()` = `world.seat(&self.ir)? ; run_main_io(&self.ir, &mut self.world)` (re-seat is idempotent for params and re-instantiates builtins, so `set_param` between runs lands); `world()` / `world_mut()`; `stdout()`/`stderr()` read `world.stdout`/`world.stderr`; `set_stdin` → `world.inject_stdin`.
  - Executor signatures in `interp.rs` become `pub(crate) fn exec_entry(ir: &Ir, world: &mut WorldState<T, BUF>) -> Result<(), ProcessError>` and `run_main_io(ir, world)`; every helper (`exec_value_instr`, `force`, `apply_callable`, `run_call`, `run_blocks`, builtin/effect helpers, internal test fns) follows the same `(ir, world)` shape, reading `world.X` and `ir.X`.

**Migration rules for tests (apply mechanically per file):**
- `let prog = test_eval::<f32, 256>(defs, expr, &ffi, sr)` → the new helpers (below); `prog` becomes a `WorldState<f32, 256>` named `world`.
- `prog.result()` → `world.result()`; `prog.arena()` → `world.arena()`; `prog.copy_buffer(r, dst)` → `world.copy_buffer(r, dst)`; structural asserts `arena().get(v)` unchanged.
- `set_param(idx, …)`/`param(idx)`/`param_index(name)` → `set_param(name, …)`/`param(name)`; where a test pre-computed an index via `param_index`, use the name directly.
- `compile_runner::<f32, 256>(src, &Registry::new(), sr)` → `compile::<f32, 256>(src, ForeignRegistry::new())`, then pre-seed `r.world.params.sample_rate = sr` where the program is signal-aware; `r.run()`, `r.stdout()`/`r.stderr()` unchanged. The legacy zero-arg-ffi form `compile::<T>(src)` (e.g. `tests/compile_errors.rs`) becomes `compile::<T, 256>(src, ForeignRegistry::new())`.
- `compile_with_ffi::<f32>(src, ffi, sr)` (value form) → `compile_eval` split (`defs`/`expr`) + world; `compile_with_mode::<f32>(src, Mode::X)` → `compile_eval(defs, expr, Mode::X)`.
- Class 2 (multi-channel legacy, `set_param`-between-runs without the new surface, `value_outputs`-era asserts): rewrite to compile against the new API, then `#[ignore = "…; re-expressed in the world-state plan"]` or delete with a ledger record.

- [ ] **Step 1: Add the shared world/eval helpers**

`tests/common/mod.rs`:
```rust
pub fn eval(defs: &str, expr: &str) -> WorldState<f32, 256>          // default config, empty ffi
pub fn eval_ffi(defs: &str, expr: &str, ffi: ForeignRegistry<f32>) -> WorldState<f32, 256>
pub fn eval_mode(defs: &str, expr: &str, ffi: ForeignRegistry<f32>, mode: Mode) -> WorldState<f32, 256>
pub fn out_value(world: &WorldState<f32, 256>) -> Value              // debug clone of result
pub fn buffers(world: &WorldState<f32, 256>, dst: &mut [f32]) -> bool // result + copy_buffer
```
(`eval` compiles via `compile_eval(defs, expr, Mode::Strict)`, creates the world with `WorldConfig { ffi, ..Default::default() }`, `test_eval(&ir, &mut world)`, returns the world.)

- [ ] **Step 2: Convert the interpreter to `(ir, &mut world)`**

Sweep `interp.rs` so every function's `prog` parameter becomes `(ir: &Ir, world: &mut WorldState<T, BUF>)`; `prog.ir.X` → `ir.X`, `prog.world.X` → `world.X`. Keep behavior byte-identical. Run the internal interp tests (`program.rs` test module) repointed to the new helpers.

- [ ] **Step 3: Rewrite `lib.rs` entries and `ProgramRunner`**

`compile_eval`, `test_eval(ir, world)`, `compile(src, ffi)` + gate; delete `compile_with*`/`compile_runner`/`compile_source_to_program`; `ProgramRunner { ir, world }` with lazy-seating `run()`. `WorldState::seat` now takes only `&Ir` (registry from `self.ffi`, rate from `self.params.sample_rate`).

- [ ] **Step 4: Migrate the value/structure test files (Class 1, by the rules above)**

Files: `bool_values, branching, arrow, atan2_lang_op, bang_patterns, head_arg_dispatch, sig_typed_params, data_records, data_sums, type_newtype, func_values, closures, currying_wires, hof, categories, match_patterns, array_immutable, array_matrix, typeclass, hkt_typeclass, constrained_defs, constrained_signatures, collections_list, collections_map_set, collections_maybe_either, collections_capacity, lazy_mode, thunk_unify, integration` (in `rill-lang/tests/*.rs`).

- [ ] **Step 5: Migrate the FFI/buffer/ST files**

Files: `ffi, ffi_extended, ffi_partial_application, compile_errors, smooth, graph_compile, main_cells, st_real, st_array, ix, array_buffer, array_storage, caf_free_variables`. `ffi_extended::run_spectral` keeps its 64-BUF pin via `eval_mode::<f32, 64>`. Multi-tick `set_param`-persistence value tests: Class 2 (`#[ignore]` with the world-state task name).

- [ ] **Step 6: Migrate the runner/IO files**

Files: `io_runner, io_stdio, io_streams_memory, haskell2010_syntax, deep_recursion_parity` → `compile::<f32, 256>(src, ForeignRegistry::new())` + `run()`; pre-seed `world_mut().params.sample_rate` where the program uses signal ops. (`st_after_unification` does not exist — the runner-file set is exactly the five files above.)

- [ ] **Step 7: Delete `RillProgram` and update docs/tests-in-crate**

Remove the type and its now-unused delegator; fix `src/program.rs` internal tests, `src/serde_def.rs` test 49-53, `src/lib.rs` doc examples, `README.md`.

- [ ] **Step 8: Full suite gate**

Run: `cargo test -p rill-lang --features dsp`
Expected: green (Class 2 ignored/deleted per the ledger). Then `cargo clippy -p rill-lang --lib --tests` (zero warnings), `cargo fmt --check`.

- [ ] **Step 9: Commit**

```bash
git add rill-lang/src rill-lang/tests rill-lang/README.md
git commit -m 'feat(rill-lang): world-owned execution; test_eval(ir, world); compile(src, ffi) runner'
```

---

### Task 5: Compile gate, unknown-builtin-at-seat, `copy_buffer` pins

Behavioral pins from the Review Focus that Task 4's refactor must not regress.

**Files:**
- Create: `rill-lang/tests/compile_gate.rs`, `rill-lang/tests/world_errors.rs`
- Modify: (only if a pin surfaces a hole in Task 4)

**Interfaces:**
- Produces the pinned behaviors as tests.

- [ ] **Step 1: Write `tests/compile_gate.rs`**

- `compile::<f32, 256>("main = 1.0 + 2.0", ForeignRegistry::new())` → `Err`.
- `compile::<f32, 256>("main = sine 440.0 0.5 0.0", ffi_with_generators)` → `Err` (Buffer-valued main).
- `compile::<f32, 256>("main = do { write (Just \"a\") stdout; return () }", ffi)` → `Ok`; `run()`; `stdout() == ["a"]`.
- effect-final `main = do { write (Just "ok") stdout }` → runs, `["ok"]`; `main = do { write (Just "a") stdout; return 1.0 }` → `Err` at run with the `IO ()` payload message.
- runner set_param between runs: `run()` once, `world_mut().set_param("level", 0.5)`, `run()` again in the same world, second run observes 0.5 (pins Review Focus 3/4).

- [ ] **Step 2: Write `tests/world_errors.rs`**

- unknown built-in name: `compile_eval` succeeds on `foreign`-declared source; building the world and `test_eval` seats → `CompileError` (pins Review Focus 5).
- `copy_buffer` on a `Float` result → `false`.

- [ ] **Step 3: Gate**

Run: `cargo test -p rill-lang --features dsp`
Expected: green. `cargo clippy -p rill-lang --lib --tests` zero warnings; `cargo fmt --check`.

- [ ] **Step 4: Commit**

```bash
git add rill-lang/tests/compile_gate.rs rill-lang/tests/world_errors.rs
git commit -m 'test(rill-lang): pin compile gate, seat-time builtin errors, copy_buffer'
```

---

### Task 6: rill-adrift + examples + docs migration

**Files:**
- Modify: `rill-adrift/tests/lang_builtins.rs`, `rill-adrift/tests/lang_builtins_stress.rs`, `rill-adrift/tests/engine_params.rs`, `rill-adrift/tests/modular_launch.rs` (already on new `compile` shape), `rill-adrift/src/modular/mod.rs`, `rill-adrift/examples/program.rs`, `rill-adrift/src/lang_builtins.rs`, `rill-adrift/src/registration.rs`, `rill-lang/examples/*` (per-crate examples, if any still compile against `compile_runner`)
- Modify: `rill-lang/README.md`, `rill-lang/src/lib.rs` crate docs, prelude exports

**Interfaces:**
- Consumes: Task 4 API.

- [ ] **Step 1: Migrate rill-adrift**

`full_ffi::<f32>()` stays the registry builder; tests migrate per Task 4 rules (SR pin 48_000.0 preserved by pre-seeding `world.params.sample_rate`). `modular/mod.rs`: drop `set_args` remnants, use `compile` + Prerunner `world_mut` pre-seed; `lang_builtins.rs`: keep `register_foreign_*`; drop legacy `register_lang_builtins(&mut Registry)` calls (they break when the legacy API is gone — see Task 7) but keep the foreign path working.

- [ ] **Step 2: Migrate examples and docs**

`examples/program.rs` → `compile::<f32, 256>(src, ffi)` + `run()`; crate docs and README updated to name `WorldState`, `compile`, `compile_eval`, `test_eval`.

- [ ] **Step 3: Gate**

Run: `cargo test -p rill-lang --features dsp && cargo test -p rill-adrift`
Expected: green. Clippy + fmt for both crates.

- [ ] **Step 4: Commit**

```bash
git add rill-adrift rill-lang/README.md rill-lang/examples
git commit -m 'feat(rill-adrift): migrate to world-owned compile/runner; docs updated'
```

---

### Task 7: Delete the legacy `Registry` and the `graph` path

The last consumers of the legacy builtin `Registry<T>` and the graph module are gone after Tasks 4–6; delete them and every downstream legacy register function.

**Files:**
- Delete: `rill-lang/src/graph/` (all 7 files), `rill-lang/tests/graph_partition.rs`
- Modify: `rill-lang/src/lib.rs` (`pub mod graph`), `rill-lang/Cargo.toml` (graph feature + `graph_partition` test target), `rill-lang/src/builtin.rs` (delete `Registry`), `rill-lang/src/register.rs` (delete `register_core_dsp_builtins`), `rill-lang/src/lib.rs`/`prelude.rs` exports
- Modify: `rill-fft/src/register.rs`, `rill-lofi/src/register.rs`, `rill-sampler/src/register.rs`, `rill-sampler/src/tape/lang.rs` (legacy `register_lang_builtins(&mut Registry)`/`register_tape_builtins` deleted; foreign variants stay), `rill-adrift/src/lang_builtins.rs` (delete `full_registry`/`full_registry_f32`)
- Test: `rill-lang/tests/graph_parse.rs` (parser-only, no graph-API imports) must still compile.

**Interfaces:**
- Produces: no `Registry`, no `register_core_dsp_builtins`, no graph module/feature/target, no `full_registry*`; `register_foreign_*` is the only factory surface.

- [ ] **Step 1: Delete the graph path and the legacy registry**

Remove `src/graph/`, `pub mod graph`, the `graph` feature, the `graph_partition` test target, and `tests/graph_partition.rs`. Delete `Registry<T>` + impls, `register_core_dsp_builtins`, and the downstream legacy registers listed above. Update exports.

- [ ] **Step 2: Gate**

Run: `cargo test -p rill-lang --features dsp && cargo test -p rill-adrift`
Expected: green (`graph_parse.rs` still compiles; no `Registry` references remain). `cargo clippy -p rill-lang --lib --tests` zero warnings; `cargo fmt --check`.

- [ ] **Step 3: Grep-sweep**

Run: `grep -rn "\bRegistry\b\|compile_runner\|compile_with" rill-lang/src rill-lang/tests rill-adrift/src rill-adrift/tests rill-fft/src rill-lofi/src rill-sampler/src`
Expected: no matches.

- [ ] **Step 4: Commit**

```bash
git rm -r rill-lang/src/graph rill-lang/tests/graph_partition.rs
git add rill-lang rill-fft rill-lofi rill-sampler rill-adrift
git commit -m 'refactor(rill-lang): delete legacy Registry and graph path; foreign-only factory surface'
```

---

### Task 8: Final cleanup and ledger close

**Files:**
- Modify: rill-lang crate docs/`README.md` doc-comment reads, `rill-lang/src/prelude.rs`, any lingering comments referencing `RillProgram`/`sample_rate` params, and the ignored/ledgered tests' `#[ignore]` reasons.

**Interfaces:**
- Produces: a fully green world-state sandbox with zero stragglers.

- [ ] **Step 1: Sweep docs and names**

`grep -rn "RillProgram\|sample_rate: f32" rill-lang/src rill-lang/tests rill-lang/README.md rill-adrift` → update remaining references. Verify no `main_fragment`/`eval_run`/`run_tick` phrasing in commit history of this branch.

- [ ] **Step 2: Full workspace gate**

Run: `cargo test -p rill-lang --features dsp && cargo test -p rill-adrift && cargo clippy -p rill-lang --lib --tests && cargo fmt --check`
Expected: all green, zero warnings.

- [ ] **Step 3: Record the ledger**

Ensure every Class-2 `#[ignore]`/deleted test is recorded (file, test, cause, resolving task) in the plan's sibling ledger file (`docs/superpowers/ledgers/2026-10-18-world-state.md`).

- [ ] **Step 4: Commit**

```bash
git add rill-lang rill-adrift docs/superpowers/ledgers/2026-10-18-world-state.md
git commit -m 'docs(rill-lang): close world-state migration ledger and sweep docs'
```

---

## Self-Review notes (executor checklist)

- Spec intent (external world, IR/world split) is materialized by Task 1 (state move), Task 2 (seat/config), Task 3 (params/sr), Task 4 (API break). Non-goals preserved: no ST-magic lifetime, no new allocator research (page door reused), no channel collapse, no trampoline.
- The FFI-ownership requirement (spec decision 4 + answer "world owns the registry") lands at the Task 4 break — Tasks 1–3 keep `foreign: &ForeignRegistry` passing so the suite stays green mid-refactor.
- `smooth`'s runtime coefficient (spec consequence) is pinned in Task 3 (`tests/param_seed.rs` + untouched `tests/smooth.rs`).
- `?sample_rate` field semantics, seat absent-only seeding, and name-addressed `set_param` are pinned in Task 3 tests and re-pinned on the runner path in Task 5.