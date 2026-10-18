# rill-lang: single run + `main : IO ()` + eval-entry test migration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish `feature/rill-lang-arrays` by unifying every execution path into one `run_blocks`/`force` mechanism, making `main : IO ()` a STATIC typing rule — the only legal program form behind a single ForeignRegistry-only `compile` — and migrating the value-test surface onto an expression evaluator (`test_eval`) with full arena + buffer observability.

**Architecture:** (1) extract the duplicated top-level/fragment block-walk into one `run_blocks` walker; run the top-level track as the program's entry fragment at base 0 through the same `run_call`-family entry with a no-unwind frame flag, capturing the terminal register into the single `RillProgram::result: Option<ArenaRef>` field; fold the eager/lazy/effect call sites into `force` as the one execution point (borrowed head return; `force_owned` as the standard adapter). (2) Drop the `main : (String…) -> IO ()` launch-argument machinery (main cells, `ValueReadMainCell`/`ValueBufferMainCellRead`, the String special-case in inference); the compiler's PROGRAM surface treats `main` as a free variable constrained to a value of type `IO ()` at the compile entry (Hindley–Milner erasure: types exist only during compilation, so the check is a compile-time gate keyed to the entry-name parameter `"main"`, never a runtime latch); a value-`main` (Float/Record/Buffer/…) is ill-typed in the compile front-end. Infer itself is blind to `main` — it types all top-level defs and returns their scheme map. (3) Replace the six compile entry points with one `compile::<T, BUF>(src, &ForeignRegistry<T>, sr) -> ProgramRunner` whose entry gate enforces the `IO ()` unification; the eval surface is `test_eval::<T, BUF>(defs, expr, &ForeignRegistry<T>, sr)` — it evaluates the EXPRESSION given to it, synthesizing the entry under a non-`main` name (the `IO ()` rule fires only when the entry is literally named `main`), so `main` has exactly one meaning compiler-wide and the evaluator never selects `main` as an entry — a def named `main` in `defs` is a plain binding. Delete the legacy builtin `Registry`, `register_core_dsp_builtins`, and the dead `graph` module. (4) Migrate all value/buffer tests to the evaluator (`eval_run` + `result()` + `copy_buffer(r, dst)`) and the runner tests to the new `compile`; buffer samples are read through `copy_buffer`, which resolves the arena ref → `Value::Buffer(idx)` → the separate `buffers: Vec<RegionHandle<T>>` table (buffer data is NOT stored in the arena).

**Ordering principle:** Rust has no function overloading, so `compile_runner` → `compile` renaming and the deletion of `compile`/`compile_with*`/`compile_with_ffi*` are atomic with the migration of every consumer. The sequence is therefore: interpreter unification and `main`-simplification first (all tests still green on the old entries) → add `test_eval` next to the old entries → migrate value tests to `test_eval` → only then rename `compile_runner` → `compile`, delete the legacy entries, and migrate the last runner files together → delete `Registry`/graph → sweep docs.

**Tech Stack:** Rust, `rill-lang` (~18-crate workspace), `rill-core` (`Arena`/`RegionHandle`/`Transcendental`), git-flow (`feature/rill-lang-arrays`).

**Spec:** `docs/superpowers/specs/2026-10-18-rill-lang-single-run-design.md` (authoritative on the interpreter unification). User decisions confirmed 2026-10-18: test_eval entry (variant 2, also a future diagnostics/rill-analyzer foundation); `compile` is ForeignRegistry-only (variant 1); `compile_runner` renames to `compile`; graph path deleted; main is a pure `IO ()` value; no value-output channels — program surface is `result` only. **Correction (user, same day):** `main : IO ()` is a STATIC typing rule — `main` is a free variable unified with a value of type `IO ()` — enforced at the compile-time entry, not at runtime. **Hindley–Milner erasure (user, same day):** types exist only during compilation — after `lower` the `TypedProgram` is gone and `RillProgram` carries only the `Ir` plus type-derived flags (`main_io_unit`). The type of `main` is fixed at compile time or does not exist at all; there is no runtime type to check. Consequences:

- **Infer is blind to `main`.** It types every top-level definition and returns their scheme map; no `main`-existence requirement, no retry-against-`IO ()`, no `process_ty`. A real `main`'s monad is fixed structurally by the concrete IO actions in its `do` chain (`write :: Maybe a -> OutputStream a -> IO ()`, `fromList :: List a -> IO (InputStream a)`, `makeInputStream :: IO (Maybe a) -> IO (InputStream a)`, …); a bare `main = return ()` is meaningless and needs no repair.
- **Entry name is a compile-time pipeline parameter** (default `"main"`; `compile` → `"main"`, `test_eval` → an internal non-`main` marker). The `IO ()` gate fires only when a def selected as the entry is literally named `main` — in `compile`, after lowering via `ir.main_io_unit` (Task 14). `main = 1.0 + 2.0` in the compile front-end is a compile error; no runtime latch exists.
- **In a `test_eval` input, `main` is an ordinary name.** `defs` may contain `main = 42` — it is just a binding (not the entry), legal, typed, and usable from `expr`; no `IO ()` check applies to it. `main` as a *program entry* has exactly one interpretation compiler-wide (the compile path), and the eval surface never selects `main` as an entry.

## Global Constraints

- Gate for every task: `cargo test -p rill-lang --features dsp` green; `cargo clippy -p rill-lang --lib --tests` zero warnings; `cargo fmt --check`. Commits on `feature/rill-lang-arrays` in **single quotes** (`git commit -m '…'`).
- **`main : IO ()` is a STATIC typing rule, not a runtime latch.** Hindley–Milner erasure: types exist only during compilation — after `lower` no type information survives into the program, so the check must happen at compile time or nowhere. **Infer is blind to `main`**: it types all top-level defs and outputs their scheme map; no `main`-existence requirement, no retry-against-`IO ()`, no `process_ty`. The `IO ()` gate lives in the compile entry (entry-name parameter, default `"main"`), enforced via `ir.main_io_unit`; a value-`main` (`main = 1.0 + 2.0`, `main = Point { … }`, `main = sine …`) is ill-typed in the compile front-end and fails there.
- **The eval surface `test_eval` never spells `main` as an entry.** Its input is `(defs, expr)`: an optional definitions/declarations context and the expression to evaluate. The entry is synthesized under an internal non-`main` name (a reserved marker that cannot collide with user identifiers), so the `IO ()` rule does not apply — the expression may be Float/Int/Bool/String/Record/Sum/Newtype/Callable/Cons/List/Array/Buffer, evaluated to WHNF and inspected via `arena()`. A def literally named `main` in `defs` is a plain binding (`main = 42` → the map carries `main`, usable from `expr`); only the compile front-end gives `main` entry semantics. This is an expression evaluator, not a program front-end; `main` retains exactly one interpretation compiler-wide.
- The program surface has exactly one output field: `RillProgram::result: Option<ArenaRef>` (the terminal value of the entry fragment after the last run). `value_outputs`, `run_value()`, `copy_output_samples()` are removed from the public API and replaced by `eval_run()`/`result()`/`copy_buffer(r, dst)`.
- Buffer samples live in `RillProgram::buffers: Vec<RegionHandle<T>>` + `block_len` — never in the arena (the arena holds only `Value::Buffer(idx)`).
- No parser/AST/prelude changes beyond the pre-plan KwMain step (commit `6fe3310`: `main` lexes as an ordinary identifier; no parser-level `main` requirement — the entry name is a compile-time pipeline parameter). IR keeps top-level blocks; the entry-fragment view is a wiring decision at program assembly. `--features graph` is pre-existing broken and the whole graph path is deleted (it references the nonexistent `program_engine`); `graph_partition` stays dead.
- Builtin `Registry<T>` (`src/builtin.rs`) and `register_core_dsp_builtins` (`src/register.rs:280`) are deleted after consumers migrate; `ForeignRegistry` is the only factory registry.
- rill-adrift: `modular` moves to `full_ffi` + new `compile`; `full_registry`/`full_registry_f32` deleted.
- English docs/comments (rill convention). No blanket `#[allow]`. No new external crates. Buffer-inspection tests migrate via `copy_buffer` and are retained (not deferred to the signal-I/O redesign).
- Value-typed `main` through `test_eval` may be any expression (Float/Int/Bool/String/Record/Sum/Newtype/Callable/Cons/List/Array/Buffer); structural assertion stays exact via `arena()`.

## Review Focus

1. **Non-`IO ()` mains are ill-typed at the compile front-end** — `compile::<f32, 256>("main = 1.0 + 2.0", …)` returns `Err` (the `IO ()` gate on the compile entry, keyed to the entry-name parameter). Infer itself is blind to `main` (it types all defs and returns the scheme map), so the rejection lives in the compile entry, not in `infer`. Pinned in Task 14.
2. **Deep non-tail recursion** must produce the same result after the `run_blocks`/`force` unification as before (behavior parity, not stack safety — stack safety is the deferred trampoline step). Pinned in Task 1.
3. **`copy_buffer` on a non-buffer ref** must return `false` (not panic): the arena ref is a `Float`/`Record`/dangling — no `buffers` lookup crash. Pinned in Task 11.
4. **Repeated `eval_run()` ticks** must not exhaust the fixed arena for multi-tick value programs (`live < capacity` invariant, "value output survives across ticks" semantics). Pinned in Task 11.
5. **`main = do { …; return () }` and effect-final `main = do { write … stdout }`** still run to completion through the new `compile`+`run`, with the `IO ()` payload contract enforced (non-`()` main results rejected). Pinned in Task 14.

---

## File Structure

**Interpreter core (`src/backend/interp.rs`):**
- `run_blocks(prog, blocks: &[ValueBlock], entry, base, drops)` — the one block-walker (§1). Consumed by `run_call`'s body phase and by the main-fragment runner.
- `run_value_tick` (existing, `interp.rs:54`) — stays as the per-tick orchestration (renamed `run_tick`): push params → release previous `prog.main` → run the main fragment (no-unwind) → `swap_block_state` → force+copy the terminal into `prog.main` → clear registers.
- `run_value_track` (existing, `interp.rs:139`) — DELETED; its ~40-line loop becomes `run_blocks`.
- `run_main_io` (existing, `interp.rs:213`) — reads `prog.main` instead of `value_outputs.first()`.
- `run_call` (existing, `interp.rs:4762`) — body-walk replaced by `run_blocks`; gains the `no_unwind: bool` frame flag.
- `force` (existing, `interp.rs:4932`) — the single execution entry (borrowed head return); eager apply folded in; `ValueCallLazy` callee-kind special case preserved; `force_owned` (`interp.rs:3311`) becomes the standard owned adapter.
- `apply_callable` (`interp.rs:3255`) + `ArgsSource` (`interp.rs:3224`) — rewritten against the unified entry.

**`src/program.rs` (RillProgram):** add `main: Option<ArenaRef>`; remove `value_outputs` field, `main_cells`, `run_value()`, `value_outputs()`, `copy_output_samples()`, `set_main_arg()`, `num_main_args()`, `alloc_main_cells()`; add `eval_run()`, `result()`, `copy_buffer(r, dst)`. `new_with` becomes ForeignRegistry-only. `arena()`, params surface, `inject_stdin`, `init`, `reset` stay.

**`src/program_runner.rs`:** `set_args` removed (Decision 6); `run`/`set_stdin`/`stdout`/`stderr`/`program`/`program_mut` stay.

**`src/lib.rs`:** delete `compile`, `compile_with_mode`, `compile_with`, `compile_with_mode_with`, `compile_with_ffi`, `compile_with_ffi_mode`; rename `compile_runner` → `compile::<T, BUF>(src, &ForeignRegistry<T>, sr) -> ProgramRunner` (Complete mode default, IO-enforced); add `test_eval::<T, BUF>` (Strict default) and `test_eval_with_mode::<T, BUF>`; `compile_source_to_program` becomes ForeignRegistry-only. Prelude exports updated.

**`src/ir.rs`:** remove `num_main_cells` (`ir.rs:916`) and `value_output_regs` (`ir.rs:924`) — the main fragment carries the output. Keep `main_io_unit` (`ir.rs:900`).

**`src/lower.rs`:** remove main-cell binding (`lower.rs:7758-7763`); the value-out/signal-out split collapses to the single value result; `main_result_is_io_unit` (`lower.rs:244`) drops the `Thunk` arm. `lower_program_with_channels` (`7686-7689`) replaces `program.main_def()` with an entry-name parameter (compile: `"main"`, test_eval: the internal marker) and reads the entry's scheme from `TypedProgram::def_schemes` instead of `process_ty` (`7780` `has_value_out`, `7906` `main_io_unit`).

**`src/types/infer.rs`:** remove the `main` → `String` λ-parameter special-cases (`infer.rs:2176-2178`, `2213-2217`, `2265-2267`); DELETE the `main` retry-against-`IO ()` (`2259-2276`) — infer is blind to `main`, the monad of a real `main` is fixed by concrete `do` actions, and `main = return ()` is meaningless. Replace `TypedProgram::process_ty` (`infer.rs:26`, `2037`) with `def_schemes: HashMap<String, Scheme>` (the full top-level scheme map); drop the `main`-existence error (`1931-1938`) and the arity check (`1946-1953`).

**`src/builtin.rs` / `src/register.rs`:** delete `Registry`, `register_core_dsp_builtins`; keep `ForeignRegistry` and `register_foreign_*`.

**`src/graph/`:** delete entirely (`compile.rs`, `builder.rs`, `partition.rs`, `reconstruct.rs`, `serialization.rs`, `spec.rs`, `mod.rs`); remove `#[cfg(feature = "graph")] pub mod graph` (`lib.rs:25-26`), the `graph` feature and `graph_partition` test target in `Cargo.toml`.

**Downstream:** `rill-adrift/src/lang_builtins.rs` (drop `full_registry`/`full_registry_f32`), `rill-adrift/src/modular/mod.rs` (full_ffi + new compile, drop `set_args`), `rill-adrift/examples/program.rs`, `rill-fft/src/register.rs`, `rill-lofi/src/register.rs`, `rill-sampler/src/register.rs` + `rill-sampler/src/tape/lang.rs` (legacy `register_lang_builtins` deleted; foreign variants stay).

**Tests:** ~41 value-path files in `rill-lang/tests` migrate by pattern (Tasks 7-11); 4 runner files + `deep_recursion_parity.rs` + `st_after_unification.rs` migrate to new `compile` in Task 14; `graph_partition.rs` deleted (Task 13); `io_runner::main_string_args_bind` deleted (Task 3). rill-adrift tests migrate in Task 14 (runner) and Task 12 (value/buffer via test_eval).

---

### Task 1: `run_blocks` — the single block-walker

**Files:**
- Modify: `rill-lang/src/backend/interp.rs` (`run_value_track` 139-199, `run_call` 4840-4876)
- Test: `rill-lang/tests/deep_recursion_parity.rs` (new)

**Interfaces:**
- Produces: `fn run_blocks<T, const BUF>(prog: &mut RillProgram<T, BUF>, blocks: &[ValueBlock], entry: usize, base: usize, drops: &mut Vec<ArenaRef>)` — executes each block's `b.instrs` via `exec_value_instr(prog, i, base, drops)`, then the terminator dispatch (`Fallthrough`/`Branch`/`BranchCtor`/`Halt`) with the same `debug_assert!` fall-off-the-end guard. Owns no drops discipline and no register cleanup — those stay at callers.

- [ ] **Step 1: Write the failing parity test**

`tests/deep_recursion_parity.rs` — a deep non-tail recursion carrying state through cons cells, run through the runner (this is behavior parity, not stack safety):

```rust
// tests/deep_recursion_parity.rs
use rill_lang::builtin::Registry;
use rill_lang::compile_runner;

#[test]
fn deep_non_tail_recursion_same_result() {
    let src = r"{-# MODE complete #-}
take 0.0 xs = [];
take n xs = cons (head xs) (take (n - 1.0) (tail xs));
from n = cons n (from (n + 1.0));
main = do { ys <- toList (take 3.0 (from 10.0));
    write (Just (fold (\acc -> \x -> concat_string acc (show_float x)) "" ys)) stdout;
    return () };
";
    let mut r = compile_runner::<f32, 256>(src, &Registry::new(), 44100.0).unwrap();
    r.run().unwrap();
    assert_eq!(r.stdout(), ["10.011.012.0"]);
}
```

- [ ] **Step 2: Run the test to verify it passes on the current code (baseline)** — must be green before the refactor.
- [ ] **Step 3: Extract `run_blocks`**

Move the exact loop body from `run_value_track` (lines 154-186, minus the `mem::take` of `prog.ir.value_blocks`) and from `run_call` (lines 4841-4876, with `base` applied) into `run_blocks(prog, blocks, entry, base, drops)`. Call it from both sites. In `run_value_track`, keep the `mem::take`/restore of `prog.ir.value_blocks`, the mark/drain drops, and the `value_error` latch exactly as-is, passing `&blocks`, `prog.ir.value_entry`, `0`. In `run_call`, keep the frame plumbing (capture cells, arg binding, watermark, result copy, unwind) exactly as-is, passing `&frag.value_blocks`, `frag.entry`, `base`. No behavior change.

- [ ] **Step 4: Run the full suite**

Run: `cargo test -p rill-lang --features dsp`
Expected: all pass, including `deep_recursion_parity`.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/backend/interp.rs rill-lang/tests/deep_recursion_parity.rs
git commit -m 'refactor(rill-lang): extract shared run_blocks block walker'
```

---

### Task 2: Program as main fragment — no-unwind mode, `prog.main` capture

**Files:**
- Modify: `rill-lang/src/backend/interp.rs` (`run_value_tick` 54-127, `run_value_track`, `run_call` 4762+, `run_main_io` 213+)
- Modify: `rill-lang/src/program.rs` (fields 183-249, `new_with` 609+, `value_outputs` 948-950)
- Modify: `rill-lang/src/ir.rs`, `rill-lang/src/lower.rs` (main-fragment wiring, `value_output_regs`)

**Interfaces:**
- Consumes: `run_blocks` from Task 1.
- Produces: `prog.main: Option<ArenaRef>` field; `run_call(…, no_unwind: bool)`; main fragment executed at base 0 with empty env through the `run_call`-family entry. A temporary 1-element `value_outputs` compat shim (`vec![prog.main]`) keeps every existing value test green until Task 15; `value_outputs()` accessor still compiled.

- [ ] **Step 1: Add the main-fragment view**

In `Ir` (or at program assembly), expose the top-level track as the main fragment: reuse `ir.value_blocks` + `ir.value_entry` + `ir.value_output_regs` (first entry) as the main fragment's body/entry/output-view. Prefer a wiring that keeps `lower.rs`'s top-level block emission unchanged (the design: "existing IR shapes keep lowering unchanged").

- [ ] **Step 2: Add `no_unwind` to `run_call`**

Add `no_unwind: bool` to `run_call`'s signature (existing call sites pass `false`). When `true`, skip step 6's register/cell drain (lines 4897-4906) — the caller owns cleanup.

- [ ] **Step 3: Rewrite `run_value_tick`'s track phase and capture**

Replace the `run_value_track` call with an invocation of the main-fragment runner at base 0 with an empty env record, `no_unwind: true`; the fragment's output register is forced and `copy`-owned into `prog.main` (replacing the multi-output copy loop, which collapses to one capture). Keep the release-old-`prog.main` step before the run, `swap_block_state`, `clear_value_regs`, and the latched-`value_error` ordering exactly. Delete `run_value_track`. Keep `value_outputs` as the shim for compatibility this task.

- [ ] **Step 4: Repoint `run_main_io`**

Change `run_main_io` to read `prog.main` (not `value_outputs.first()`), keeping the `unIO` projection → World-token apply → pair-force → `main : IO ()` payload contract exactly.

- [ ] **Step 5: Run the full suite**

Run: `cargo test -p rill-lang --features dsp`
Expected: green (behavior parity; value tests still read the shim).

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/backend/interp.rs rill-lang/src/program.rs
git commit -m 'refactor(rill-lang): run main as a no-unwind fragment, capture prog.main'
```

---

### Task 3: `main` is a pure `IO ()` — drop String launch arguments (Decision 6)

**Files:**
- Modify: `rill-lang/src/types/infer.rs` (2176-2178, 2213-2217, 2265-2267)
- Modify: `rill-lang/src/lower.rs` (`main_result_is_io_unit` 244-252, main-cell binding 7758-7763)
- Modify: `rill-lang/src/ir.rs` (`num_main_cells` 916)
- Modify: `rill-lang/src/program.rs` (`main_cells` 214, `alloc_main_cells` 740, `set_main_arg` 884, `num_main_args` 897)
- Modify: `rill-lang/src/program_runner.rs` (`set_args` 68)
- Modify: `rill-lang/src/backend/interp.rs` (`ValueReadMainCell`/`ValueBufferMainCellRead` arms)
- Modify: `rill-lang/tests/io_runner.rs` (delete `main_string_args_bind` 54-60)
- Modify: `rill-adrift/src/modular/mod.rs` (206) — minimal: drop the `set_args` call

**Interfaces:**
- Produces: `ProgramRunner` without `set_args`; `RillProgram` without `set_main_arg`/`num_main_args`; `main_result_is_io_unit` = direct `IO ()` only; a `main` with λ-parameters now infers as a plain function and is rejected by the compile gate (Task 14).

- [ ] **Step 1: Remove the `main → String` inference special-cases**

In `infer.rs`, replace the three `def.name() == "main"` branches binding `ValueTy::String` with the ordinary parameter path (signature-typed or default `Float`). DELETE the `main` retry-against-`IO ()` (2259-2276) — infer is blind to `main`; the monad of a real `main` is pinned by the concrete `IO …` actions in its `do` chain, and `main = return ()` is a meaningless program (no expected type to fix). Replace the `process_ty: main_scheme.ty` build (`2037`) with `def_schemes: ctx.defs.clone()` and drop the `main`-existence/arity errors.

- [ ] **Step 2: Delete the main-cell machinery**

Remove `Ir::num_main_cells`, the `lw.main_cell_locals`/main-cell binding in `lower.rs`, `RillProgram::main_cells`/`alloc_main_cells`/`set_main_arg`/`num_main_args`, the `main_cells` init in `build` (`program.rs:688`), the `ValueReadMainCell`/`ValueBufferMainCellRead` instruction arms, and `ProgramRunner::set_args`. Update the arena-capacity bound comment (drop the "main λ-parameter cells" line).

- [ ] **Step 3: Drop the `Thunk` arm of `main_result_is_io_unit`**

`lower.rs:249`: parameterless `main` types directly as `IO ()` — remove the `Thunk(_, rets)` arm.

- [ ] **Step 4: Delete `io_runner::main_string_args_bind` and the rill-adrift `set_args` call**

Remove `tests/io_runner.rs:54-60`. In `rill-adrift/src/modular/mod.rs`, drop `runner.set_args(&args)` and the `args` plumbing from the rack launch closure.

- [ ] **Step 5: Run the workspace gate**

Run: `cargo test -p rill-lang --features dsp && cargo test -p rill-adrift`
Expected: green.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src rill-lang/tests/io_runner.rs rill-adrift/src/modular/mod.rs
git commit -m 'feat(rill-lang): main is a pure IO () value; drop String launch args'
```

---

### Task 4: `force` as the single execution point (borrowed return, eager fold-in)

**Files:**
- Modify: `rill-lang/src/backend/interp.rs` (`force` 4932-5047, `apply_callable` 3255-3303, `force_owned` 3311+, `ValueCallEager`/`ValueCallLazy` arms)
- Test: `rill-lang/tests/st_after_unification.rs` (new)

**Interfaces:**
- Consumes: `run_blocks`, no-unwind `run_call` (Tasks 1-2).
- Produces: `force()` returning a BORROWED head ref (unchanged semantics); `force_owned()` as the standard owned adapter; `ArgsSource` as the only frame-materialization difference; `ValueCallLazy` eager special case preserved (thunk/BuiltinCallable/Bottom force+apply eagerly; only an eager `Value::Callable` callee suspends).

- [ ] **Step 1: Write the ST-parity guard test**

`tests/st_after_unification.rs` — a runST test (mirroring `st_real.rs`) proving an ST action reached through a `let` still runs eagerly (the load-bearing `ValueCallLazy` invariant, `interp.rs:1314-1321`): `main = do { …; r <- runST …; write (Just (show_float (…))) stdout; return () }` with an expected number.

- [ ] **Step 2: Fold eager dispatch into `force`**

Restructure callable dispatch so every way to reach a fragment body goes through one function: `force` keeps the `cell: Some` FSM chase and the `cell: None` already-WHNF return; the eager application path (`ValueCallEager`'s `run_call(ArgsSource::Registers)` and `apply_callable`'s `CallableKind::Func`) becomes the same "materialize frame + run body" invoked from the unified function. `BuiltinCallable` effects stay strict (apply via `exec_effect` eagerly) — the unification is the call boundary, not deferral.

- [ ] **Step 3: Make `force_owned` the standard adapter**

Update the 7 `apply_callable` call sites and `dst`-write paths to the borrowed-return contract with explicit `copy_owned` where fresh ownership is required (`ThunkState::Forced` memo storage, result-register writes, effect results).

- [ ] **Step 4: Run the full suite + parity tests**

Run: `cargo test -p rill-lang --features dsp` (all `st_*`/`lazy_mode`/`deep_recursion_parity`/`st_after_unification`).
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/backend/interp.rs rill-lang/tests/st_after_unification.rs
git commit -m 'refactor(rill-lang): force is the single execution point, borrowed return'
```

---

### Task 5: ForeignRegistry-only `RillProgram::new_with` + `test_eval` entry

**Files:**
- Modify: `rill-lang/src/program.rs` (`new_with` 609-616, `new_with_resources` 622-630, `build` 632+)
- Modify: `rill-lang/src/lib.rs` (new `test_eval` entries; `compile_*` callers of `new_with`)
- Modify: `rill-lang/tests/ffi_extended.rs` (the `run_spectral` helper uses `new_with` directly)

**Interfaces:**
- Consumes: `RillProgram` internals from Tasks 2-3.
- Produces:
  - `RillProgram::new_with(ir: Ir, ffi: &ForeignRegistry<T>, sample_rate: f32) -> Result<Self, CompileError>` — builtins resolve from `foreign` only; an unresolvable name is a `CompileError`. `new_with_resources` keeps its `ResourceRegistry` param but becomes ffi-only (sole current user is the dead graph path; kept for tape until Task 13 decides).
  - `pub fn test_eval<T: Transcendental, const BUF: usize>(defs: &str, expr: &str, ffi: &ForeignRegistry<T>, sample_rate: f32) -> Result<RillProgram<T, BUF>, CompileError>` — compiles `defs` as the definitions context and `expr` as the entry (synthesized under an internal non-`main` marker; the `IO ()` rule never fires), Strict default, pragma overrides.
  - `pub fn test_eval_with_mode<T, const BUF>(defs: &str, expr: &str, ffi: &ForeignRegistry<T>, sample_rate: f32, mode: Mode) -> …`.
  - `RillProgram::eval_run(&mut self) -> Result<(), ProcessError>` (the renamed per-tick runner); `RillProgram::result(&self) -> Option<ArenaRef>` (= `prog.main`, `None` before first run); `RillProgram::copy_buffer(&self, r: ArenaRef, dst: &mut [T]) -> bool`.

- [ ] **Step 1: Resign `new_with`/`build` to foreign-only**

Drop `registry: &Registry<T>` from `new_with`/`build`/`build_builtin`; resolve from `foreign` only. Update every in-tree caller (`lib.rs` `compile_with*`/`compile_runner`, `tests/ffi_extended.rs::run_spectral`) to the new signature at once.

- [ ] **Step 2: Add `eval_run`/`result`/`copy_buffer` and the `test_eval` entries**

Rename the internal per-tick function `run_value_tick` → `run_tick` (private); expose `eval_run()` and `result()`. `copy_buffer` resolves `Value::Buffer(idx)` from the ref then copies `min(dst.len(), block_len)` samples from `buffers[idx]`, `false` on non-buffer/dangling/out-of-range. `test_eval`/`test_eval_with_mode` build `defs [+ internal entry = expr]` and compile via the FFI-only pipeline, returning `RillProgram` directly.

- [ ] **Step 3: Write the eval-entry test**

`tests/eval_entry.rs` (entry synthesized as a non-`main` def; `main` in defs is a plain binding):
- `test_eval("", "1.0 + 2.0", &ForeignRegistry::new(), 44100.0)` → `eval_run()` → `arena().get(result().unwrap()) == Float(3.0)`.
- Structure: `data Point = { x: Float, y: Float }` defs + `Point { x: 2.0, y: 3.0 }` expr → `Value::Record(_)` via `result()`.
- Mode: `test_eval_with_mode(…, …, Mode::Complete)` on a recursive `countup` → `Float(3.0)`.
- Multi-tick: `expr = ?gain` with `set_param` between `eval_run()`s → persists.
- Plain binding: `test_eval("main = 42", "main * 2.0", …)` → `Float(84.0)` — `main` in defs is inert outside the compile front-end.

- [ ] **Step 4: Suite gate**

Run: `cargo test -p rill-lang --features dsp`
Expected: green (old entries still present and untouched).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lib.rs rill-lang/src/program.rs rill-lang/tests/ffi_extended.rs rill-lang/tests/eval_entry.rs
git commit -m 'feat(rill-lang): test_eval entry + eval_run/result/copy_buffer; FFI-only new_with'
```

---

### Task 6: Shared eval-entry test helpers

**Files:**
- Create: `rill-lang/tests/common/mod.rs`

**Interfaces:**
- Produces, for use by Tasks 7-11 (helpers keep the current `src`-form, splitting it internally):
  - `fn eval<T: Transcendental, const BUF: usize>(src: &str, ffi: &ForeignRegistry<T>, sample_rate: f32) -> RillProgram<T, BUF>` — parses `src`, separates the `main`-less defs from `main`'s body (the classic `main = expr` value test becomes `defs` + `expr`), and calls `test_eval(defs, expr, ffi, sr)`; panics with a message on compile error (mirrors existing helpers).
  - `fn eval_mode<T, const BUF>(src: &str, ffi: &ForeignRegistry<T>, sr: f32, mode: Mode) -> RillProgram<T, BUF>` — same split with an explicit `mode`.
  - `fn run_tick<T, const BUF>(prog: &mut RillProgram<T, BUF>) -> Result<(), ProcessError>` — thin `eval_run` wrapper.
  - `fn out_value<T, const BUF>(prog: &RillProgram<T, BUF>) -> Value` — debug clone of `arena().get(result().unwrap())`.
  - `fn buffers<T, const BUF>(prog: &mut RillProgram<T, BUF>, dst: &mut [T]) -> bool` — runs one tick, gets `result()`, `copy_buffer`s.

- [ ] **Step 1: Write `tests/common/mod.rs`** exactly as specified.
- [ ] **Step 2: Smoke-use it in `tests/array_storage.rs`** (largest helper user) to prove it compiles against the new API.
- [ ] **Step 3: Commit**

```bash
git add rill-lang/tests/common rill-lang/tests/array_storage.rs
git commit -m 'test(rill-lang): shared eval-entry test helpers'
```

---

### Task 7: Migrate pure-value tests (part 1 — scalars + simple structures)

**Files (migration rule):
- Modify: `tests/bool_values.rs`, `tests/branching.rs`, `tests/arrow.rs`, `tests/atan2_lang_op.rs`, `tests/bang_patterns.rs`, `tests/head_arg_dispatch.rs`, `tests/sig_typed_params.rs`

**Migration rule (applies to all value tests):**
- `compile::<f32>(src)` → `eval(src, &ForeignRegistry::<f32>::new(), 44100.0)`; `compile_with_ffi::<f32>(src, ffi, sr)` → `eval(src, ffi, sr)`; `compile_with_mode::<f32>(src, Mode::X)` → `eval_mode(src, …, sr, Mode::X)`.
- `prog.run_value()` → `prog.eval_run()` (or `run_tick`).
- `prog.value_outputs()[0].unwrap()` → `prog.result().unwrap()`.
- Structural asserts on `arena().get(v)` stay byte-for-byte identical.
- Delete now-dead imports (`compile`, `compile_with`, `compile_with_ffi`, `builtin::Registry`).

- [ ] **Step 1: Migrate the seven files** by the rule.
- [ ] **Step 2: Run the migrated files only**

Run: `cargo test -p rill-lang --features dsp --test bool_values --test branching --test arrow --test atan2_lang_op --test bang_patterns --test head_arg_dispatch --test sig_typed_params`
Expected: green.

- [ ] **Step 3: Commit**

```bash
git add rill-lang/tests
git commit -m 'test(rill-lang): migrate scalar value tests to test_eval'
```

---

### Task 8: Migrate pure-value tests (part 2 — records, sums, newtypes, HOFs, closures)

**Files:**
- Modify: `tests/data_records.rs`, `tests/data_sums.rs`, `tests/type_newtype.rs`, `tests/func_values.rs`, `tests/closures.rs`, `tests/currying_wires.rs`, `tests/hof.rs`, `tests/categories.rs`, `tests/match_patterns.rs`, `tests/array_immutable.rs`, `tests/array_matrix.rs`

**Migration rule:** same as Task 7. `Value::Record`/`Sum`/`Newtype`/`Callable { cell: None }`/`Cons` asserts are preserved exactly — `result()` gives the same ref `value_outputs()[0]` did.

- [ ] **Step 1: Migrate the eleven files** by the rule; keep structural asserts verbatim.
- [ ] **Step 2: Run the migrated files only**

Run: `cargo test -p rill-lang --features dsp --test data_records --test data_sums --test type_newtype --test func_values --test closures --test currying_wires --test hof --test categories --test match_patterns --test array_immutable --test array_matrix`
Expected: green.

- [ ] **Step 3: Commit**

```bash
git add rill-lang/tests
git commit -m 'test(rill-lang): migrate structural value tests to test_eval'
```

---

### Task 9: Migrate collections + lazy-mode tests (pins Review Focus 4)

**Files:**
- Modify: `tests/collections_list.rs`, `tests/collections_map_set.rs`, `tests/collections_maybe_either.rs`, `tests/collections_capacity.rs`, `tests/lazy_mode.rs`, `tests/thunk_unify.rs`, `tests/constrained_defs.rs`, `tests/constrained_signatures.rs`, `tests/hkt_typeclass.rs`, `tests/typeclass.rs`, `tests/integration.rs`

**Migration rule:** as Task 7; `Mode::Complete` calls → `eval_mode(…, Mode::Complete)`. Multi-tick arena-survival asserts ("survives across ticks", `live < capacity`) keep working via `eval_run()` — the per-run release/capture semantics are identical to `run_value`.

- [ ] **Step 1: Migrate the eleven files.**
- [ ] **Step 2: Run the migrated files only**

Run: `cargo test -p rill-lang --features dsp --test collections_list --test collections_map_set --test collections_maybe_either --test collections_capacity --test lazy_mode --test thunk_unify --test constrained_defs --test constrained_signatures --test hkt_typeclass --test typeclass --test integration`
Expected: green.

- [ ] **Step 3: Commit**

```bash
git add rill-lang/tests
git commit -m 'test(rill-lang): migrate collections/lazy tests to test_eval'
```

---

### Task 10: Migrate FFI/buffer tests, part 1 (pins Review Focus 3)

**Files:**
- Modify: `tests/smooth.rs`, `tests/graph_compile.rs`, `tests/main_cells.rs`, `tests/st_real.rs`, `tests/st_array.rs`, `tests/ix.rs`, `tests/array_buffer.rs`, `tests/array_storage.rs`

**Migration rule for buffer asserts:**
- Value reads as Task 7.
- `prog.copy_output_samples(0, &mut out)` → `let r = prog.result().unwrap(); assert!(prog.copy_buffer(r, &mut out));`
- `main_cells.rs`/`st_real.rs` param modulation + multi-tick: `set_param` between `eval_run()`s; keep cell-persistence semantics.
- One test asserts Review Focus 3: `copy_buffer` on a `result()` that is a `Float` returns `false`.

- [ ] **Step 1: Migrate the eight files** by the rule.
- [ ] **Step 2: Run the migrated files only**

Run: `cargo test -p rill-lang --features dsp --test smooth --test graph_compile --test main_cells --test st_real --test st_array --test ix --test array_buffer --test array_storage`
Expected: green.

- [ ] **Step 3: Commit**

```bash
git add rill-lang/tests
git commit -m 'test(rill-lang): migrate buffer tests to eval + copy_buffer'
```

---

### Task 11: Migrate FFI/buffer tests, part 2 — the big `ffi.rs` + extended

**Files:**
- Modify: `tests/ffi.rs`, `tests/ffi_extended.rs`, `tests/ffi_partial_application.rs`, `tests/compile_errors.rs`

**Migration rule:** as Task 10. `ffi_extended::run_spectral` keeps its 64-BUF pin via `test_eval_with_mode::<f32, 64>(defs, expr, ffi, sr, Mode::Strict)`. For `compile_errors.rs`: a value-`main` case is a value-semantics check and stays `Ok` through the `eval` helper (defs/expr split — `main` in defs is a plain binding); if a case specifically asserts the compile front-end rejects non-`IO ()` mains, it moves to the future `compile` gate test (`tests/compile_gate.rs`, Task 14) — decide each case by what it is actually testing: compile rejection vs value semantics.

- [ ] **Step 1: Migrate the four files.**
- [ ] **Step 2: Run the migrated files only**

Run: `cargo test -p rill-lang --features dsp --test ffi --test ffi_extended --test ffi_partial_application --test compile_errors`
Expected: green.

- [ ] **Step 3: Commit**

```bash
git add rill-lang/tests
git commit -m 'test(rill-lang): migrate ffi test suites to eval + copy_buffer'
```

---

### Task 12: rill-adrift value/buffer tests → eval

**Files:**
- Modify: `rill-adrift/tests/lang_builtins.rs`, `rill-adrift/tests/lang_builtins_stress.rs`, `rill-adrift/tests/engine_params.rs`

**Interfaces:**
- Consumes: `test_eval`/`copy_buffer` (Task 5), `full_ffi` (still available in `lang_builtins.rs` this task — `full_registry` deletion happens in Task 14).
- Produces: `lang_builtins`/`lang_builtins_stress`/`engine_params` use `test_eval` + `copy_buffer` (SR pin 48_000.0 preserved).

- [ ] **Step 1: Migrate the three files** by the Task 10 rule (`compile_with_ffi::<f32>(src, &full_ffi::<f32>(), sr)` → `eval(src, &full_ffi::<f32>(), sr)`; `copy_output_samples(0, &mut out)` → `copy_buffer`).
- [ ] **Step 2: Run the rill-adrift gate**

Run: `cargo test -p rill-adrift`
Expected: green (runner-based `modular_launch.rs` untouched — still on `compile_runner` until Task 14).

- [ ] **Step 3: Commit**

```bash
git add rill-adrift/tests
git commit -m 'test(rill-adrift): migrate lang builtins tests to eval + copy_buffer'
```

---

### Task 13: Delete the `graph` path

**Files:**
- Delete: `rill-lang/src/graph/` (all 7 files)
- Modify: `rill-lang/src/lib.rs` (`pub mod graph` 25-26), `rill-lang/Cargo.toml` (graph feature + `graph_partition` test target)
- Delete: `rill-lang/tests/graph_partition.rs`

**Interfaces:**
- Produces: no `graph` module/feature; no `graph_partition` test target. The legacy builtin `Registry` and `register_core_dsp_builtins` are NOT touched here — they are still consumed by the runner files and rill-adrift, which migrate in Task 14 (deleting an API is atomic with migrating its consumers).

- [ ] **Step 1: Delete the graph path**

Remove `src/graph/`, `pub mod graph`, the `graph = []` feature, the `graph_partition` Cargo test entry, `tests/graph_partition.rs`. Verify `tests/graph_parse.rs` (pure parser tests, no graph-API import) still compiles: `cargo test -p rill-lang --features dsp --test graph_parse`.

- [ ] **Step 2: Suite check**

Run: `cargo test -p rill-lang --features dsp`
Expected: green. (`Registry` still exists; only `graph_partition.rs`, the sole Registry-user deleted here, is gone.)

- [ ] **Step 3: Commit**

```bash
git rm -r rill-lang/src/graph rill-lang/tests/graph_partition.rs && git add rill-lang/src rill-lang/Cargo.toml
git commit -m 'refactor(rill-lang): delete dead graph path'
```

---

### Task 14: Unify `compile` — rename, `IO ()` gate, legacy entries + `Registry` deleted, runner migration (pins Review Focus 1, 5)

**Files:**
- Modify: `rill-lang/src/lib.rs` (60-212)
- Modify: `rill-lang/src/program_runner.rs` (doc example 149-163)
- Modify: `rill-lang/tests/io_runner.rs`, `tests/io_stdio.rs`, `tests/io_streams_memory.rs`, `tests/haskell2010_syntax.rs`, `tests/deep_recursion_parity.rs`, `tests/st_after_unification.rs`
- Modify: `rill-lang/src/builtin.rs` (delete `Registry`), `rill-lang/src/register.rs` (280+, delete `register_core_dsp_builtins`), `rill-lang/src/lib.rs`/`prelude.rs` exports
- Modify: `rill-fft/src/register.rs`, `rill-lofi/src/register.rs`, `rill-sampler/src/register.rs`, `rill-sampler/src/tape/lang.rs` (legacy `register_lang_builtins(&mut Registry)` + `register_tape_builtins` deleted; foreign variants stay)
- Modify: `rill-adrift/src/lang_builtins.rs` (delete `full_registry`/`full_registry_f32`), `rill-adrift/src/modular/mod.rs` (178-206), `rill-adrift/examples/program.rs`, `rill-adrift/src/registration.rs` (doc), `rill-adrift/tests/modular_launch.rs`
- Test: `rill-lang/tests/compile_gate.rs` (new)

**Interfaces:**
- Consumes: FFI-only assembly (Task 5), `main_io_unit` (lower). No remaining consumer of the legacy entries or `Registry` (Tasks 7-12 migrated the value/buffer tests; the runner files + downstream legacy registers migrate here).
- Produces:
  - `pub fn compile<T: Transcendental, const BUF: usize>(src: &str, ffi: &ForeignRegistry<T>, sample_rate: f32) -> Result<ProgramRunner<T, BUF>, CompileError>` — the former `compile_runner`, Complete-mode default (pragma overrides), plus an `IO ()` gate: after lowering, `if !ir.main_io_unit { return CompileError }` ("`main` must have type `IO ()`").
  - Deleted: `compile`, `compile_with_mode`, `compile_with`, `compile_with_mode_with`, `compile_with_ffi`, `compile_with_ffi_mode`, `compile_runner`, `Registry`, `register_core_dsp_builtins`, `full_registry`, `full_registry_f32`, downstream legacy `register_lang_builtins(&mut Registry)`/`register_tape_builtins`.

- [ ] **Step 1: Write the compile-gate tests (pins Review Focus 1)**

`tests/compile_gate.rs`:
- `compile::<f32, 256>("main = 1.0 + 2.0", &ForeignRegistry::new(), 44100.0)` → `Err`.
- `compile::<f32, 256>("main = square 440.0 ?level 0.0", &ffi_with_generators, 44100.0)` → `Err` (Buffer-valued main is not `IO ()`).
- `compile::<f32, 256>("main = do { write (Just \"a\") stdout; return () }", …)` → `Ok`, run, `stdout == ["a"]`.
- Pins Review Focus 5: effect-final `main = do { write (Just "ok") stdout }` → runs, `["ok"]`; `main = do { write (Just "a") stdout; return 1.0 }` → clean `Err` at run with the `IO ()` payload message.

- [ ] **Step 2: Rename and gate**

Move `compile_runner`'s body to `compile` (new signature). Add the `!ir.main_io_unit → Err` gate. Delete the six legacy entry points. Update the prelude export.

- [ ] **Step 3: Delete `Registry`, the legacy register functions, and `full_registry`**

Remove `Registry<T>` + impls in `builtin.rs`; delete `register_core_dsp_builtins` and every downstream legacy `register_lang_builtins(&mut Registry)`/`register_tape_builtins` (`register_foreign_*` remains the only surface; `register_model`/analog unaffected). Update `lib.rs`/`prelude.rs` exports. Delete `full_registry`/`full_registry_f32` in rill-adrift. If `new_with_resources` keeps no consumer after `tape_loop` moves fully to ffi/resource declarations, delete it; verify the tape path still assembles (one live consumer check).

- [ ] **Step 4: Migrate the runner test files + rill-adrift in the same commit**

`io_runner`/`io_stdio`/`io_streams_memory`/`haskell2010_syntax`/`deep_recursion_parity`/`st_after_unification`: `compile_runner::<f32, 256>(src, &Registry::new(), sr)` → `compile::<f32, 256>(src, &ForeignRegistry::new(), sr)`, drop `builtin::Registry` imports. rill-adrift: `modular/mod.rs` → `compile::<f32, BUF>(src, &full_ffi::<f32>(), sr)` (lofi variant uses `full_ffi_f32`), delete `set_args` remnants; delete `full_registry`/`full_registry_f32`; `examples/program.rs` and `modular_launch.rs` migrate to the new `compile`; fix `program_runner.rs` and `lib.rs` doc examples.

- [ ] **Step 5: Full workspace gate**

Run: `cargo test -p rill-lang --features dsp && cargo test -p rill-adrift && cargo check -p rill-fft -p rill-lofi -p rill-sampler --features lang`
Expected: green.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src rill-lang/tests rill-adrift/src rill-adrift/tests rill-adrift/examples rill-fft/src rill-lofi/src rill-sampler/src
git commit -m 'feat(rill-lang): unified ForeignRegistry-only compile; IO()-gate; drop legacy entries, Registry'
```

---

### Task 15: Finalize `RillProgram` public surface

**Files:**
- Modify: `rill-lang/src/program.rs`

**Interfaces:**
- Produces the final public surface: `main` (field), `eval_run()`, `result()`, `copy_buffer()`, `arena()`, `set_param`/`param`/`params_meta`/`param_index`, `init`, `reset`, `inject_stdin`. Removed: `value_outputs` field/accessor, `run_value()`, `copy_output_samples()`, the Task 2 compat shim.

- [ ] **Step 1: Remove the last value-path remnants** — `value_outputs` field, `run_value()`, `value_outputs()` accessor, the 1-element shim. `run_tick` stays private, exposed only via `eval_run`.
- [ ] **Step 2: Full suite gate**

Run: `cargo test -p rill-lang --features dsp && cargo test -p rill-adrift`
Expected: green — proves no test still uses the removed API.

- [ ] **Step 3: Commit**

```bash
git add rill-lang/src/program.rs
git commit -m 'refactor(rill-lang): final RillProgram surface — eval_run/result/copy_buffer only'
```

---

### Task 16: Value-test final sweep

**Files:** sweep + stragglers

- [ ] **Step 1: Sweep for leftovers**

Run: `grep -rn "run_value\|value_outputs\|copy_output_samples\|compile_with_ffi\|compile_with_mode\|compile_runner\|builtin::Registry" rill-lang rill-adrift --include='*.rs'`
Expected: no matches (except intended doc strings). Fix stragglers.
- [ ] **Step 2: clippy + fmt + full gate**

Run: `cargo test -p rill-lang --features dsp && cargo test -p rill-adrift && cargo clippy -p rill-lang --lib --tests && cargo fmt --check`
Expected: green, zero clippy warnings.

- [ ] **Step 3: Commit**

```bash
git add rill-lang rill-adrift
git commit -m 'test(rill-lang): final value-path sweep'
```

---

### Task 17: `test_eval` diagnostics probe (foundation for rill-analyzer)

**Files:**
- Modify: `rill-lang/src/lib.rs` (doc), `rill-lang/src/program.rs` (doc)

**Interfaces:**
- Produces: a documented building block showing the eval pipeline is queryable — `test_eval` + `result()` + `arena()` documented as the intended foundation for diagnostics and `rill-analyzer` repair.

- [ ] **Step 1: Write the doc** (no behavior change; a compile-touched example asserts `test_eval("", "2.0 * 3.0", …)` → `Float(6.0)`).
- [ ] **Step 2: Gate** — `cargo test -p rill-lang --features dsp && cargo clippy -p rill-lang --lib --tests && cargo fmt --check`.
- [ ] **Step 3: Commit**

```bash
git add rill-lang/src/lib.rs rill-lang/src/program.rs
git commit -m 'docs(rill-lang): document test_eval as diagnostics/eval foundation'
```

---

### Task 18: Docs + public-surface consistency

**Files:**
- Modify: `rill-lang/README.md`, `rill-adrift/README.md`, `docs/src/guides/…`, `program_runner.rs` module doc (10), `lib.rs` crate doc

- [ ] **Step 1: Sweep docs**

Run: `grep -rn "set_args\|compile_runner\|run_value\|value_outputs\|copy_output_samples\|builtin::Registry\|compile_with" rill-lang rill-adrift docs --include='*.md' --include='*.rs'`
Update every hit to the new API. Delete `set_args` references (a future `System.Env` replaces launch args, out of scope).

- [ ] **Step 2: clippy + fmt**

Run: `cargo clippy -p rill-lang --lib --tests && cargo fmt --check`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add rill-lang rill-adrift docs
git commit -m 'docs: update to unified compile + test_eval API'
```

---

### Task 19: Whole-branch review gate

**Files:** none (verification only)

- [ ] **Step 1: Full workspace check**

Run: `cargo test -p rill-lang --features dsp && cargo test -p rill-adrift && cargo clippy -p rill-lang --lib --tests && cargo fmt --check`
Expected: all green; zero clippy warnings; fmt clean.

- [ ] **Step 2: Confirm deleted surfaces are gone**

Run: `grep -rn "value_outputs\|copy_output_samples\|run_value\|compile_runner\|builtin::Registry\|register_core_dsp_builtins\|pub mod graph" rill-lang rill-adrift --include='*.rs'`
Expected: no matches.

- [ ] **Step 3: Confirm remaining blockers are documented, not regressions**

`cargo test -p rill-lang --features graph` no longer builds the feature (removed). `--all-targets` still blocked by stale `benches/lang_bench.rs` (pre-existing, untouched). Verify `cargo test -p rill-lang --features dsp --all-targets` failure is ONLY the bench, not lib/tests.

- [ ] **Step 4: Resolve PARK items and finish the branch**

Address any `PARK`/`TODO` markers introduced during implementation, run the full gate one final time, then decide branch completion (merge/push+PR/keep) with the user. Note: the array-hole (M8) work is a separate follow-up plan (`2026-10-18-rill-lang-array-hole-design.md`).

---

## Self-Review Notes

- **Spec coverage:** single-run unification → Tasks 1-2, 4 (run_blocks, program=fragment/no-unwind, force-entry/borrowed-return/ValueCallLazy special case); Decision 6 (main = IO () value, no String args) → Task 3; Decision 7 (no value-output channels, `prog.main` single field, buffer-inspection via `copy_buffer`) → Tasks 2, 5, 10, 15; unified compile (ForeignRegistry-only) → Tasks 5, 14; graph deletion → Task 13; test migration → Tasks 6-12, 16; rill-adrift → Tasks 3, 12, 14; KwMain removal → pre-plan commit `6fe3310` (lexer/parser no longer reserve `main`; the entry name is a compile-time parameter).
- **Hindley–Milner erasure (user, 2026-10-18):** types exist only during compilation — after `lower`, `RillProgram` carries only `Ir` + type-derived flags. The `IO ()` rule is a COMPILE-TIME gate on the entry (`ir.main_io_unit`, Task 14), never a runtime latch. Infer is BLIND to `main`: it types all top-level defs and returns their scheme map (`TypedProgram::def_schemes`), with no `main`-existence requirement, no retry-against-`IO ()` (a real `main`'s monad is fixed by concrete `do` actions; `main = return ()` is meaningless), and no `process_ty`. The entry name is the compile parameter: `"main"` for `compile`, an internal non-`main` marker for `test_eval`.
- **Buffer observability correction (user, 2026-10-18):** buffer data lives in `RillProgram::buffers: Vec<RegionHandle<T>>` + `block_len`, not the arena — `copy_buffer(r, dst)` is the designed accessor (Task 5, used Tasks 10-12, Review Focus 3).
- **Type consistency:** `prog.main` field name consistent (Tasks 2, 5, 15); `eval_run`/`result()`/`copy_buffer` identical everywhere; `test_eval(defs, expr)`/`test_eval_with_mode(defs, expr, …)` signatures match Task 5 and all migration tasks (the `eval`/`eval_mode` test helpers split the classic `main = expr` source into defs+expr internally); `compile` final signature matches Tasks 14, the doc examples, and rill-adrift.
- **Green-every-task:** renaming `compile_runner` → `compile`, deleting the legacy entries, and deleting `Registry` (Task 14) is atomic with migrating every consumer — all value/buffer consumers were migrated in Tasks 7-12 while the legacy entries still existed; the runner files + downstream legacy registers + `full_registry` migrate and delete in the same commit. Task 13 removes ONLY the graph path (its sole Registry user is `graph_partition.rs`, deleted there). Rust's no-overloading constraint is what makes this ordering mandatory.
- **Ordering ruling (inline):** `Registry` deletion moved OUT of Task 13 into Task 14 — Task 13 would otherwise break `deep_recursion_parity.rs`/`st_after_unification.rs` (created in Tasks 1/4 on the runner API) before their Task 14 migration. Cost if wrong: a larger single commit in Task 14, no behavioral cost.
- **Proportion:** migration tasks carry a rule + file list, not per-line transcripts; interpreter refactors reference exact line anchors; the two new test files' code is included only where the assertions pin the spec's exact values.