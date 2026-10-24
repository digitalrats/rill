# rill-lang: Core I/O Model — Streams, Scalar-Arg `main`, `runStream` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move rill-lang from "ProgramRunner as signal processor" to "ProgramRunner as executor of a Turing-complete, domain-agnostic program": the program form is `(arg₁…argₙ) -> IO` with scalar args; I/O is operations over first-class `InputStream`/`OutputStream` values behind a `runStream` per-block contract; the old buffer-λ `main` and the global `readInput`/`writeOutput` are removed and existing programs migrated.

**Architecture:** `InputStream a` / `OutputStream a` become first-class values expressed with the language's full HOF/HKT machinery — higher-kinded type constructors (`* -> *`), a first-class `runStream` handler (HOF). They live in a new **`STD_PRELUDE`** (the common standard-library layer), never in the strictly-category-theory `CATEGORY_PRELUDE`. `runStream` is the program-facing per-block contract: `main` binds scalar args and runs warm setup once; the ProgramRunner invokes the handler per tick with the active stream pair. `read ins` / `write outs` / `channel k (read ins)` desugar to the existing per-tick channel-load/channel-drain IR (`ValueReadInputs` / `ValueWriteOutputList` / `ValueReadInput`), which is retained as the internal mechanism.

**Tech Stack:** Rust; the existing rill-lang compiler pipeline (infer → reduce → lower → interp), `rill-core` buffers, `rill-core-actor` mailbox.

**Spec:** `docs/superpowers/specs/2026-10-17-rill-lang-io-state-world-streams-design.md`

## Global Constraints

- **No backward compatibility (directive).** The buffer-λ-parameter `main`, the global `readInput`/`writeOutput` primitives, and their indexed variants are removed from the user surface; existing programs and tests migrate to the stream form. No sugar layer is kept.
- **ProgramRunner is an executor of a Turing-complete program**, not a signal processor; domain-agnostic. Its identity is executing the program, not transforming signals.
- **Do not touch `CATEGORY_PRELUDE`** (`rill-lang/src/types/ty.rs:256` — strictly category theory: Functor/Applicative/Monad/Monoid/Arrow, Kleisli, ST, IO, Pair). Stream types/ops go in a new `STD_PRELUDE`.
- **Use the full HOF/HKT machinery** for the stream contract: `InputStream a`/`OutputStream a` are higher-kinded constructors; `runStream`'s handler is a first-class function.
- **RT rules** (AGENTS.md): no heap allocation, locks, syscalls, `log`/`println`/`format!` on the hot (per-tick) path. `read ins`/`write outs`/`channel k` must not allocate on the per-tick path.
- **No new third-party dependencies** without explicit confirmation.
- `#![deny(unsafe_code)]`, `#![warn(missing_docs)]`, docs/comments in English, `max_width=100`.
- **Workspace baseline:** `cargo test --workspace` already fails because `rill-adrift` default enables `serialization` → `graph`, and `reconstruct.rs` carries 11 known pre-existing errors (deferred Plan 1 Task 2). This plan's verification runs against `-p rill-lang`; the `graph` feature is untouched. `serialization` must keep `graph`.

## Review Focus

1. **A program with no `runStream` and no streams** (`main = return ()`, or a pure scalar `do`-block): must compile and run with a no-op per-tick body, not error on an absent handler. → Task 2.
2. **`runStream` handler reading a 0-input-channel stream** (write-only program): `read ins` yields an empty list, no crash. → Task 2.
3. **`channel k` out of range** on a frame: bounded (empty/clamped/clean error), not a panic. → Task 2.
4. **A main scalar λ-arg is settable via `?name`/`SetParameter`**: control still flows through the scalar form after the main-form change. → Task 3.
5. **`write` with fewer channels than the backend expects**: a clean error, no partial under/overflow. → Task 4.

---

### Task 1: `STD_PRELUDE` + stream types + boundary operations (type level)

Introduce the standard-library layer and the stream contract at the type level. Everything here is additive — the old `readInput`/`writeOutput` still work and all existing tests stay green.

**Files:**
- Modify: `rill-lang/src/types/ty.rs` — add `pub(crate) const STD_PRELUDE: &str` (a sibling of `CATEGORY_PRELUDE` at line 256 and `SIGNAL_PRELUDE` at line 323), registered in the same code path that parses the other preludes into the `TypeEnv` (near `TypeEnv::with_builtins`). Contents:
  ```
  data InputStream  a = { unInputStream: a };
  data OutputStream a = { unOutputStream: a };
  ```
- Modify: `rill-lang/src/types/ty.rs` — in the `TypeEnv` builtin-signature setup (~lines 633-659, where `readInput`/`writeOutput` are inserted): **add** builtin sigs, keeping `readInput`/`writeOutput` until Task 4:
  - `read       : InputStream Buffer -> List Buffer` (empty list = closed/EOF)
  - `write      : OutputStream Buffer -> List Buffer -> IO ()`
  - `channel    : Int -> List Buffer -> Buffer` (k-th channel of a frame)
  - `runStream  : (InputStream Buffer -> OutputStream Buffer -> IO ()) -> IO ()`
- Test: new `rill-lang/tests/stream_contract.rs` (`#![cfg(feature = "dsp")]` if it needs DSP builtins, mirroring `program_json.rs`).

**Interfaces:**
- Produces: the higher-kinded constructors `InputStream`/`OutputStream`; builtin names `read`, `write`, `channel`, `runStream`; the `STD_PRELUDE` registration point later tasks extend.

- [ ] **Step 1: Write the failing type-level tests**

```rust
// tests/stream_contract.rs
let t = ty_of("main = runStream (fn ins outs -> do { write outs (read ins); return () })");
assert_eq!(t, ValueTy::Data("IO".into(), vec![ValueTy::Void]));

// write-only, 0-channel input reviewed (Review Focus 2)
let t = ty_of("main = runStream (fn ins outs -> do { write outs [sine 440.0 1.0 0.0]; return () })");
assert_eq!(t, ValueTy::Data("IO".into(), vec![ValueTy::Void]));

// scalar pipeline through channel + map
let t = ty_of("main = runStream (fn ins outs -> do { write outs (map (\\x -> x * 0.5) (read ins)); return () })");
assert_eq!(t, ValueTy::Data("IO".into(), vec![ValueTy::Void]));
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p rill-lang --test stream_contract`
Expected: FAIL — `InputStream`/`runStream`/`read`/`write` are unresolved names.

- [ ] **Step 3: Add `STD_PRELUDE` and the builtin sigs**

Add the `STD_PRELUDE` const with the two `data` declarations and register it alongside `CATEGORY_PRELUDE`/`SIGNAL_PRELUDE`. Add the four builtin sigs (`read`, `write`, `channel`, `runStream`) exactly as in the Interfaces block. Keep `readInput`/`writeOutput` untouched.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p rill-lang --test stream_contract`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/ty.rs rill-lang/tests/stream_contract.rs
git commit -m "feat(rill-lang): STD_PRELUDE stream types and boundary ops (type level)"
```

---

### Task 2: `runStream` lowering + interpreter + ProgramRunner drives the per-tick handler

Make stream programs run end-to-end. `read ins` / `write outs` / `channel k (read ins)` desugar to the existing per-tick IR, and `runStream`'s handler becomes the per-tick body the runner drives. Additive: the old forms still run.

**Files:**
- Modify: `rill-lang/src/lower.rs` (~lines 1130-1160, where `writeOutput`/`readInput` are lowered): lower the new builtins —
  - `read ins` → `ValueReadInputs` (all channels as `List Buffer`);
  - `write outs list` → `ValueWriteOutputList`;
  - `channel k (read ins)` → `ValueReadInput { index: k }` (direct channel load);
  - `runStream (fn p1 p2 -> body)` → bind `p1` = active input stream, `p2` = active output stream, and compile `body` as the per-tick track (the same role the old `main = do { … }` body played). A `runStream` application whose argument is not such a two-param λ is a compile error.
- Modify: `rill-lang/src/ir.rs` — confirm the instructions exist with the shapes in the Interfaces block (do not add new variants unless required).
- Modify: `rill-lang/src/backend/interp.rs` (~lines 1590-1650): the per-tick path already executes `ValueReadInputs`/`ValueReadInput`/`ValueWriteOutputList`; make the runner drive the `runStream` handler body per tick with the active stream pair (no new allocation on this path).
- Modify: `rill-lang/src/program_runner.rs` — `ProgramRunner::apply` (and its engine call) now runs the program's `runStream` handler per tick with the active stream pair; a program with no `runStream` has a no-op per-tick body (Review Focus 1). `apply(&mut self, inputs, outputs, tick)` signature is unchanged for the signal-domain caller.
- Test: new `rill-lang/tests/run_stream.rs`.

**Interfaces:**
- Consumes: Task 1's `InputStream`/`OutputStream`, `read`/`write`/`channel`/`runStream`.
- Produces: the per-tick handler mechanism; `ProgramRunner` runs stream-form programs. IR surface: `ValueReadInputs { dst }`, `ValueReadInput { dst, index }`, `ValueWriteOutputList { src }` (unchanged shapes — see `ir.rs:539`, `ir.rs:641`, `ir.rs:658`).

- [ ] **Step 1: Write the failing end-to-end tests**

```rust
// tests/run_stream.rs — mirror the existing program_runner.rs:92 test as a stream program
let engine = compile_graph::<f32, 256>(
    r"main = runStream (fn ins outs -> do { write outs (map (\x -> x * 0.5) (read ins)); return () })",
    &Registry::new(), 44100.0,
).unwrap();
// drive one tick with input [0.5; 4]; assert every output == 0.25

// Review Focus 1: no runStream, no streams — compiles and ticks with a no-op body
let engine = compile_graph::<f32, 256>("main = return ()", &Registry::new(), 44100.0).unwrap();
// drive one tick; no panic, no output

// Review Focus 2: write-only program on a 0-input backend — read ins is empty, no crash
// (drive with zero input channels)

// Review Focus 3: `channel 99 (read ins)` on a 1-channel frame — bounded, no panic
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p rill-lang --test run_stream`
Expected: FAIL — stream programs do not lower/run.

- [ ] **Step 3: Implement the lowering and interpreter wiring**

Lower `read`/`write`/`channel`/`runStream` as described; wire `ProgramRunner::apply` to drive the handler per tick. Keep the per-tick path allocation-free.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p rill-lang --test run_stream`
Expected: PASS (including all four new tests, old tests still green).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lower.rs rill-lang/src/ir.rs rill-lang/src/backend/interp.rs rill-lang/src/program_runner.rs rill-lang/tests/run_stream.rs
git commit -m "feat(rill-lang): runStream per-tick handler drives the hot path"
```

---

### Task 3: Scalar-arg `main` + migrate buffer-λ-main programs

Change the program form so `main`'s λ-parameters are scalars (control/launch args), not buffer inputs. Old buffer-λ-`main` programs are migrated to the stream form **in this same task** so the suite stays green.

**Files:**
- Modify: `rill-lang/src/types/infer.rs` — remove the `def.name() == "main"` → `ValueTy::Buffer` special-case at lines 1437-1447 and 1457-1467 (and mirror at 1673, 1707): `main`'s λ-params become the same scalar default as any other definition (`ValueTy::Float`).
- Modify: `rill-lang/src/lib.rs` — `compile_program` entry points: scalar launch args are bound before run (add, e.g., `ProgramRunner::set_args(&[f32])` that binds main's scalar cells via the existing `main_cells`/`set_param` machinery, `program.rs`). Defaults stay 0.0.
- Migrate: programs using the buffer-λ form (`main i1 i2 = …`) to the stream form:
  - `rill-lang/tests/graph_compile.rs` (lines 7, 19, 31, 51),
  - `rill-lang/tests/zz_probe_probe.rs` (lines 17, 76, 90, 104),
  - `rill-lang/tests/main_cells.rs` (lines 15, 37),
  - any `main i… = …` in `rill-lang/tests/ffi.rs`.
  - Transformation rule: a 1-in/1-out program
    `main i1 = do { writeOutput [f i1]; return () }` becomes
    `main = runStream (fn ins outs -> do { write outs [f (channel 0 (read ins))]; return () })`;
    a k-in/1-out program maps each `i_j` to `channel j (read ins)`. `?name`/`?gain` control params stay exactly as-is.
- Test: `rill-lang/tests/main_cells.rs` (already migrated) — assert a scalar main arg round-trips through `set_param`/`SetParameter` (Review Focus 4).

**Interfaces:**
- Consumes: Tasks 1-2.
- Produces: `main : (arg₁…argₙ) -> IO ()` with scalar λ-params, settable via launch `set_args` and `?name`/`SetParameter`.

- [ ] **Step 1: Write the failing scalar-main tests**

```rust
// Review Focus 4: a scalar main λ-arg stays controllable via ?name/SetParameter
// main g = runStream (fn ins outs -> do { write outs (map (\x -> x * g) (read ins)); return () })
//   1) default g = 1.0 -> input 0.5 -> output 0.5
//   2) set_param("g", 2.0) -> output 1.0
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p rill-lang --test main_cells`
Expected: FAIL — main λ-param is still typed `Buffer`, or `g` does not flow.

- [ ] **Step 3: Implement scalar `main` inference and migrate the buffer-λ programs**

Remove the main-Buffer special-case in `infer.rs`; add `set_args`. Apply the transformation rule to the listed test files so the compiler suite they feed stays green.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p rill-lang`
Expected: PASS — new scalar-main tests pass and the migrated files compile; no remaining buffer-λ `main i… =` programs.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/src/lib.rs rill-lang/src/program_runner.rs rill-lang/src/program.rs rill-lang/tests/graph_compile.rs rill-lang/tests/zz_probe_probe.rs rill-lang/tests/main_cells.rs rill-lang/tests/ffi.rs
git commit -m "feat(rill-lang): scalar-arg main (args)->IO; migrate buffer-lambda programs"
```

---

### Task 4: Remove `readInput`/`writeOutput` + migrate the remaining old-form programs

Remove the old user-facing `readInput`/`writeOutput` surface and migrate the remaining programs (list-form, no λ-params) to streams.

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (~lines 633-659) — remove the `readInput`/`writeOutput` builtin sigs from `TypeEnv` name resolution for user programs.
- Modify: `rill-lang/src/ir.rs` — retire `ValueWriteOutput` (indexed write, `ir.rs:649`) if now unused; keep `ValueReadInputs`/`ValueReadInput`/`ValueWriteOutputList` (the `read`/`write`/`channel` desugar targets).
- Modify: `rill-lang/src/lower.rs` — drop the old `readInput`/`writeOutput` name handling (~lines 1134-1153).
- Modify: `rill-lang/src/backend/interp.rs` — drop `ValueWriteOutput` if retired.
- Modify: `rill-lang/src/lib.rs` — `compile_to_ir_channels`'s channel-count params are retained only for the runner's backend wiring; note in the doc that the stream form takes channel count at runtime.
- Migrate: remaining `readInput`/`writeOutput`-only programs (no buffer-λ) to the stream form:
  - `rill-lang/tests/ffi.rs` (~30 sites: `main = do { writeOutput (map f (readInput)); return () }` → `main = runStream (fn ins outs -> do { write outs (map f (read ins)); return () })`),
  - `rill-lang/tests/multichannel_runtime.rs` (line 129 and header doc),
  - `rill-lang/tests/haskell2010_syntax.rs` (line 22),
  - `rill-lang/tests/program_json.rs` (line 60),
  - `rill-lang/src/program_runner.rs` (test at line 92),
  - `rill-lang/src/serde_def.rs` (line 51) and `rill-lang/src/parser.rs` (test programs using `writeOutput`/`readInput`).
- Test: `rill-lang/tests/multichannel_runtime.rs` (migrated) — the 2→2 identity `main = runStream (fn ins outs -> do { write outs (read ins); return () })` still round-trips; plus a channel-count-mismatch test (Review Focus 5 — `write` with wrong output count gives a clean error, not a panic).

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces: a `-p rill-lang` suite with zero `readInput`/`writeOutput` references in any program string.

- [ ] **Step 1: Grep to confirm the migration surface, then write the failing removal test**

```bash
rg -rn 'readInput|writeOutput' rill-lang/src rill-lang/tests --include '*.rs' | grep -v '^.*://'   # doc anchors only after migration
```
Expected to shrink to doc comments; add a test asserting `readInput` is no longer a resolvable name (`ty_of("x = readInput")` → error).

- [ ] **Step 2: Run the removal test to verify it fails (name still resolves)**

Run: `cargo test -p rill-lang --test stream_contract`
Expected: FAIL — `readInput` still resolves.

- [ ] **Step 3: Remove the builtins and migrate the programs**

Delete the `readInput`/`writeOutput` sigs and lowering; retire `ValueWriteOutput`; apply the transformation rule to the listed files. Ensure `cargo test -p rill-lang` is the author for migration completeness.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p rill-lang`
Expected: PASS; grep shows no `readInput`/`writeOutput` in any program string (only doc anchors).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/ty.rs rill-lang/src/ir.rs rill-lang/src/lower.rs rill-lang/src/backend/interp.rs rill-lang/src/lib.rs rill-lang/tests/ffi.rs rill-lang/tests/multichannel_runtime.rs rill-lang/tests/haskell2010_syntax.rs rill-lang/tests/program_json.rs rill-lang/src/program_runner.rs rill-lang/src/serde_def.rs rill-lang/src/parser.rs
git commit -m "refactor(rill-lang): remove readInput/writeOutput; migrate to stream model"
```

---

### Task 5: rill-adrift migration + workspace verification

Migrate the umbrella crate's example programs to the stream form and verify the workspace stays within its known baseline.

**Files:**
- Migrate: `rill-adrift` example programs that embed rill-lang source using the old `readInput`/`writeOutput`/buffer-λ form (`rill-adrift/examples/*.rs`, e.g. `lang_chiptune.rs`, `record_mic.rs`, `play_wav.rs`, and any `tests/`): rewrite the program strings to `main = runStream (fn ins outs -> …)`.
- Modify: none structural — verification only, unless a migration reveals a compile entry point used by `rill-adrift` that must change signature.

**Interfaces:**
- Produces: a `rill-adrift` tree with no old-form program strings, building within the known baseline.

- [ ] **Step 1: Grep the umbrella crate for old-form programs**

```bash
rg -rn 'readInput|writeOutput|main [a-z0-9_]+ *=' rill-adrift --include '*.rs'
```
Expected: the full list of sites to migrate (all flagged above are re-checked here).

- [ ] **Step 2: Migrate the `rill-adrift` programs**

Apply the same transformation rule as Tasks 3-4. Any `compile_graph`/`compile_with_ffi_channels` call passes the stream-form source unchanged.

- [ ] **Step 3: Verify the workspace**

Run: `cargo test -p rill-lang`, `cargo clippy -p rill-lang`, `cargo fmt --check`
Expected: green, zero warnings.
Run: `cargo check -p rill-adrift` (default features)
Expected: the only failures remain the known `graph`/`reconstruct.rs` baseline (11 pre-existing errors), unchanged from before this plan.

- [ ] **Step 4: Commit**

```bash
git add rill-adrift/examples rill-adrift/tests
git commit -m "refactor(rill-adrift): migrate example programs to the stream model"
```

---

## Self-Review note

- **Spec coverage:** §4.1 (IO remains the record-container monad; State/World threads in §4.5/§4.7 are the backends & concurrency plans) — the core program-form (§4.2 stream contract), `runStream` contract (§4.4), scalar-arg `main` (§2 goal 1), and the domain-agnostic ProgramRunner (north-star) are covered by Tasks 1-4. §4.2 console/file, §4.3 passive/active by-threading, §4.4 hot-loop RT type, and §4.7 threads/actors are explicitly deferred to follow-on plans.
- **Type consistency:** the `read`/`write`/`channel`/`runStream` signatures defined in Task 1 are the only ones used in Tasks 2-5; the IR shapes (`ValueReadInputs`/`ValueReadInput`/`ValueWriteOutputList`) match `ir.rs:539/641/658`.
- **Review Focus:** tests for all five lines are pinned to Tasks 2, 3, and 4 as marked inline.
