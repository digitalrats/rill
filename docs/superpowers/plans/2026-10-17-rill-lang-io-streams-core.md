# rill-lang: Core I/O Model — Program `(String…) -> IO`, Streams with Backends, `runStream` Master-Slave Executor

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the execution system: `ProgramRunner` stops being a signal processor (`&[&[f32]] → &mut [&mut [f32]]`) and becomes the **executor of a fully Turing-complete, domain-agnostic program** `main : (String…) -> IO ()`; I/O is operations over first-class stream values whose **backends are created together with the streams inside the program**; the per-block contract is `runStream` with a **master-slave** stream pair; the old buffer-λ `main`, the global `readInput`/`writeOutput`, and the signal-transform runner/engine are removed and all programs migrate (no transition period).

**Architecture:** The runner exposes **no external I/O interface** — inputs are never passed into it from outside. The program creates its own streams via `mkStream :: BackendSpec -> IO (Stream Buffer, Stream Buffer)`; each stream is created together with its backend, resolved from a runtime backend registry threaded through the program's `State`. `runStream handler` binds the handler's first parameter to the **master** stream (the active side whose backend drives the tick) and the remaining parameter(s) to **slaves** (passive, serviced within the master's tick). The `runStream` boundary is typed over a general `Stream a`; `read`/`write`/`channel` stay on the concrete `InputStream`/`OutputStream` views via `asInput`/`asOutput` projections. The handler body is lowered as the per-tick hot loop; `read`→`ValueReadInputs`, `write`→`ValueWriteOutputList`, `channel k (read …)`→`ValueReadInput` (retained internal per-tick IR, spec §4.4).

**Tech Stack:** Rust; the existing rill-lang pipeline (infer → reduce → lower → value-track interpreter), `rill-core` buffers.

**Spec:** `docs/superpowers/specs/2026-10-17-rill-lang-io-state-world-streams-design.md`

## Global Constraints

- **No backward compatibility (directive).** Buffer-λ-parameter `main`, the global `readInput`/`writeOutput` primitives and their indexed variants, and the signal-transform `ProgramRunner`/`ProgramEngine` framing are removed. All existing programs and tests migrate to the stream form. No sugar layer is kept. No transition period: the old execution system is not kept alongside the new one.
- **ProgramRunner is an executor of a Turing-complete program**, domain-agnostic. Its identity is executing the program, not transforming signals.
- **No external I/O into the runner.** The runner takes no input buffers, output buffers, or backend handles from outside in any form. Streams — with their backends, created together — are the program's I/O.
- **main λ-args are `String` (launch-time),** bound via `set_args(&[String])`. They are NOT control params: `?name`/`SetParameter` does not target main's λ-params. `?name`/`SetParameter` remains for dynamic builtin params (Float/Int) and later in-language actors.
- **Backends are created together with streams, inside the program:** `mkStream :: BackendSpec -> IO (Stream Buffer, Stream Buffer)`. A runtime backend registry lives in the program's `State`. In-memory test backends land in this plan; real backends (audio/file/console) land in the follow-on plan as additional registry instances.
- **`runStream` is master-slave:** the handler's first parameter is the **master** stream (the active side — its backend drives the tick); the remaining parameters are **slaves** (passive, serviced within the master's tick). The master may be an InputStream or an OutputStream.
- **`runStream` boundary is typed over a general `Stream a`;** `read`/`write`/`channel` operate on the concrete `InputStream a`/`OutputStream a` views obtained via `asInput`/`asOutput` projections. A projection that does not match the stream's actual direction is a clean runtime error, never a panic.
- **Per-tick internal IR is retained** (spec §4.4): `read`→`ValueReadInputs`, `write`→`ValueWriteOutputList`, `channel k (read …)`→`ValueReadInput`. The mono-kernel value-track interpreter is not replaced.
- **RT rules** (AGENTS.md): no heap allocation, locks, syscalls, `log`/`println`/`format!` on the per-tick hot path. Warm setup runs once before the hot loop.
- **Do not touch `CATEGORY_PRELUDE`.** Stream types/ops live in `STD_PRELUDE`.
- **No new third-party dependencies** without explicit confirmation.
- `#![deny(unsafe_code)]`, `#![warn(missing_docs)]`, docs/comments in English, `max_width=100`.
- **Workspace baseline:** verification runs against `cargo test -p rill-lang --features dsp`; the `graph` feature is untouched; `rill-adrift` stays at its known baseline.

## Review Focus

1. **A program with no `runStream`** (`main = return ()`, or a pure scalar `IO` block): compiles and runs with a no-op per-tick body, not an error on an absent handler. → Task 3.
2. **`runStream` handler whose master input has no data / write-only master:** `read` on the master input yields an empty list, no crash. → Task 3.
3. **`channel k` out of range** on a frame: bounded (empty/clamped/clean error), never a panic. → Task 3.
4. **A main String λ-arg is bound at launch via `set_args(&[String])`** and readable by the program. → Task 3.
5. **`write`/backend output-channel mismatch:** a clean error, no partial overflow. → Task 4.

---

### Task 1: `STD_PRELUDE` + stream types + boundary operations (type level)

**Status:** DONE (commits 857f65b, ccb3377) — but the type surface is amended by Task 2 (general `Stream a`, `asInput`/`asOutput` projections, `runStream` handler params retyped to `Stream Buffer`). Task 2's first steps perform the amendment.

---

### Task 2: Stream/backend runtime foundation — `Stream a`, `mkStream`, backend registry, test backends

Build the value/runtime foundation of the new stream model. Everything here is **additive**: the old execution system still runs and all existing tests stay green.

**Files:**
- Modify: `rill-lang/src/types/ty.rs` — in `STD_PRELUDE` (added in Task 1), amend the type surface:
  ```
  data Stream a;                              -- general stream value (runStream boundary)
  data InputStream a = { unInputStream: a };  -- concrete input view (existing)
  data OutputStream a = { unOutputStream: a } -- concrete output view (existing)
  ```
  Keep the existing `read`/`write`/`channel`/`runStream` builtin sigs but amend `runStream`'s handler parameter to the general stream:
  - `read    : InputStream Buffer -> List Buffer`
  - `write   : OutputStream Buffer -> List Buffer -> IO ()`
  - `channel : Int -> List Buffer -> Buffer`
  - `asInput : Stream Buffer -> InputStream Buffer`
  - `asOutput: Stream Buffer -> OutputStream Buffer`
  - `runStream : (Stream Buffer -> Stream Buffer -> IO ()) -> IO ()` (handler params are general `Stream`; first = master, second = slave)
- Modify: `rill-lang/src/runtime.rs` — the runtime `State`: add a **backend registry** (`BackendRegistry`) that resolves a `BackendSpec` to a concrete stream backend instance, plus the stream table that maps a `Stream` value (a handle) to its backend side.
- Create: `rill-lang/src/backend.rs` (or extend `runtime.rs`) — the stream-backend contract for this plan: an in-memory backend with an input side and an output side; a `testInput`/`testOutput`-style in-memory backend usable by tests (fill the input side, capture the output side).
- Modify: `rill-lang/src/types/ty.rs` builtin-sig setup — add the `mkStream` sig:
  - `mkStream : BackendSpec -> IO (Stream Buffer, Stream Buffer)` (creates the backend together with the stream pair; returns the input-side and output-side streams)
  - `BackendSpec` — a value identifying a backend (for this plan: a name/record resolved against the runtime registry).
- Test: amend `rill-lang/tests/stream_contract.rs` (Task 1 tests) for the amended `runStream` handler type; add type-level tests for `Stream`/`asInput`/`asOutput`/`mkStream`.

**Interfaces:**
- Consumes: Task 1's `InputStream`/`OutputStream`/`read`/`write`/`channel`/`runStream`.
- Produces: general `Stream a`, `asInput`/`asOutput`, `mkStream :: BackendSpec -> IO (Stream Buffer, Stream Buffer)`; the runtime backend registry + stream table; the in-memory test backend.

- [ ] **Step 1: Write the failing type-level tests**

```rust
// tests/stream_contract.rs (amended)
// runStream handler params are general Stream Buffer
let t = ty_of("main = runStream (fn master slave -> do { write (asOutput slave) (read (asInput master)); return () })");
assert_eq!(t, ValueTy::Data("IO".into(), vec![ValueTy::Void]));

// projections type-check: asInput :: Stream Buffer -> InputStream Buffer
let t = ty_of("x = read (asInput (undefined :: Stream Buffer))");  // or an inline example
// mkStream returns IO (Stream Buffer, Stream Buffer)
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p rill-lang --features dsp --test stream_contract`
Expected: FAIL — `Stream`/`asInput`/`asOutput` are unresolved.

- [ ] **Step 3: Implement the amended types and the runtime backend foundation**

Add `data Stream a` to `STD_PRELUDE`; add the `asInput`/`asOutput`/`mkStream` builtin sigs; amend `runStream`'s handler param type to `Stream Buffer`. Add the backend registry to the runtime `State` and the in-memory test backend.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p rill-lang --features dsp`
Expected: PASS — amended type tests pass, all existing tests still green.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/ty.rs rill-lang/src/runtime.rs rill-lang/src/backend.rs rill-lang/tests/stream_contract.rs
git commit -m "feat(rill-lang): general Stream type, asInput/asOutput, mkStream backend registry"
```

---

### Task 3: New execution system — ProgramRunner as executor of `(String…) -> IO`, `runStream` master-slave hot loop

**This is the core replacement.** Build the new executor and make it the only execution path; remove the old signal-transform execution system and migrate the whole `-p rill-lang` suite to the stream form in this same task (no transition period).

**Files:**
- Create/Modify: `rill-lang/src/program_runner.rs` — `ProgramRunner` is now the executor of `main : (String…) -> IO ()`:
  ```rust
  pub struct ProgramRunner<T: Transcendental, const BUF: usize> { /* program + runtime State + mailbox */ }
  impl ProgramRunner<f32, BUF> {
      pub fn set_args(&mut self, args: &[String]);                       // bind main's String λ-params
      pub fn run_warm(&mut self) -> Result<(), CompileError>;            // run main's IO once: mkStream creates streams/backends, runStream registers the hot loop
      pub fn process_tick(&mut self) -> Result<(), ProcessError>;        // per-tick hot loop — NO external buffers; called by the master backend's callback (or a test driver)
      pub fn handle(&self) -> ActorRef<CommandEnum>;                     // SetParameter for builtin dynamic params
      pub fn backends(&self) -> &BackendRegistry;                        // test access to created backends
  }
  ```
  `apply(&mut self, inputs, outputs, tick)` **does not exist** — no external I/O.
- Modify: `rill-lang/src/program.rs` — replace `RillProgram`'s role: remove the `MultichannelAlgorithm`/`Algorithm` **signal-transform** impls and the program-fixed `num_inputs`/`num_outputs`; the program owns the per-tick hot loop (the `runStream` handler body) + the warm-setup instruction list + the String-arg cells. The value-track interpreter (`backend/interp.rs`) is retained as the mono-kernel.
- Remove: `rill-lang/src/program_engine.rs` (`ProgramEngine` signal wrapper) — mailbox/param routing moves into `ProgramRunner` or a thin control helper.
- Modify: `rill-lang/src/lower.rs` — the new lowering entry for `(String…) -> IO`:
  - main λ-params → persistent String cells (bound by `set_args`), removing the Buffer-input path.
  - `mkStream spec` → create a backend + stream pair at warm time; the streams become the program's active master/slave.
  - `runStream handler` → extract the 2-param handler; bind the first param as the master stream and the second as the slave; lower the handler body as the per-tick hot loop. A `runStream` whose argument is not such a two-param function is a compile error.
  - `read (asInput master)` → `ValueReadInputs`; `write (asOutput slave) list` → `ValueWriteOutputList`; `channel k (read (asInput master))` → `ValueReadInput { index: k }`.
  - `main = return ()` (no `runStream`) → empty/no-op hot loop (Review Focus 1).
- Modify: `rill-lang/src/lib.rs` — the compile entry points produce the new `Program`/`ProgramRunner`; remove the channel-count plumbing (`compile_to_ir_channels`'s `input_channels`/`output_channels`) and the `ProgramEngine`-returning entry points.
- Modify: `rill-lang/src/types/infer.rs` — remove the `main` → `ValueTy::Buffer` special-case (lines 1437-1447/1457-1467, 1673, 1707): main's λ-params are typed `String`.
- Modify: `rill-lang/src/ir.rs` — keep `ValueReadInputs`/`ValueReadInput`/`ValueWriteOutputList`; retire `ValueWriteOutput` if unused after migration; the IR's program-level framing drops `num_inputs`/`num_outputs`-as-signal-arities (channel counts are runtime stream properties).
- Migrate (in this task, so the suite stays green): all `-p rill-lang` programs to the stream form —
  - `writeOutput (map f (readInput))` → `write (asOutput slave) (map f (read (asInput master)))` inside a `runStream` handler;
  - buffer-λ `main i1` → `channel 0 (read (asInput master))`;
  - signal mains `main g = g * 0.5` → the same inside a `runStream` handler;
  - `?name`/`?gain` builtin dynamic params stay exactly as-is.
  Files: `tests/{ffi,ffi_extended,ffi_partial_application,integration,multichannel_runtime,graph_compile,main_cells,smooth,atan2_lang_op,caf_free_variables,collections_list,haskell2010_syntax,program_json,io_types}.rs`, `src/{serde_def.rs,parser.rs,lib.rs}` tests, `benches/lang_bench.rs`.
- Test: new `rill-lang/tests/run_stream.rs` — end-to-end through streams created in the program (Review Focus 1-3, plus the master-slave directions).

**Interfaces:**
- Consumes: Task 2's `Stream`/`asInput`/`asOutput`/`mkStream`, the backend registry + test backend.
- Produces: `ProgramRunner` (executor; `set_args`/`run_warm`/`process_tick`/`handle`/`backends`); the per-tick hot loop; String-arg main; a `-p rill-lang` suite with zero old-form program strings.

- [ ] **Step 1: Write the failing end-to-end tests**

```rust
// tests/run_stream.rs — streams created in the program, no external I/O
// master = input, slave = output: read the master input, scale, write the slave output
let src = r"
    handler master slave = do
        write (asOutput slave) (map (\x -> x * 0.5) (read (asInput master)))
        return ()
    main = do
        (a, b) <- mkStream (backend "test")
        runStream handler
        return ()";
// compile to ProgramRunner with a registry seeded with the in-memory "test" backend
// run_warm() -> fill the test backend's input side with [0.5; 4]
// process_tick() -> assert the captured output == [0.25; 4]

// Review Focus 1: no runStream -> compiles, no-op per-tick body
let src = r"main = do { return () }";

// Review Focus 2: write-only master output / empty master input -> read is empty, no crash
// Review Focus 3: `channel 99 (read (asInput master))` on a 1-channel frame -> bounded, no panic
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p rill-lang --features dsp --test run_stream`
Expected: FAIL — stream programs do not lower/run; `mkStream`/`runStream` are not wired.

- [ ] **Step 3: Implement the new executor, the new lowering, and remove the old execution system**

Implement `ProgramRunner` (set_args/run_warm/process_tick), the new lowering (String-arg main, mkStream, runStream master-slave, read/write/channel with projections), and remove `ProgramEngine`/`RillProgram`-signal-transform framing. Migrate all `-p rill-lang` programs per the rule above. Keep the per-tick path allocation-free.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p rill-lang --features dsp`
Expected: PASS — the new end-to-end tests pass, the migrated suite is green, no old-form program strings remain.
Run: `cargo clippy -p rill-lang --features dsp` and `cargo fmt --check` — clean.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src rill-lang/tests rill-lang/benches
git commit -m "feat(rill-lang): replace execution system — ProgramRunner executes (String...)->IO with runStream master-slave streams"
```

---

### Task 4: Remove `readInput`/`writeOutput` remnants + channel-mismatch error

Clean up the last old-surface remnants and pin the output-channel mismatch behavior.

**Files:**
- Modify: `rill-lang/src/types/ty.rs` — remove the `readInput`/`writeOutput` builtin sigs from `TypeEnv` name resolution (user programs can no longer resolve them).
- Modify: `rill-lang/src/ir.rs` — retire `ValueWriteOutput` (indexed write) if unused; keep `ValueReadInputs`/`ValueReadInput`/`ValueWriteOutputList`.
- Modify: `rill-lang/src/lower.rs` and `rill-lang/src/backend/interp.rs` — drop any leftover `readInput`/`writeOutput`/`ValueWriteOutput` handling.
- Test: `rill-lang/tests/run_stream.rs` — add a Review Focus 5 test: `write` a chunk whose channel count does not match the output stream's backend channel count gives a clean error (no partial overflow/panic).

**Interfaces:**
- Consumes: Task 3's executor + migrated suite.
- Produces: a `-p rill-lang` tree with zero `readInput`/`writeOutput` references in any program string (doc anchors only).

- [ ] **Step 1: Grep to confirm the removal surface, then write the failing removal test**

```bash
rg -rn 'readInput|writeOutput' rill-lang/src rill-lang/tests --include '*.rs'
```
Expected to shrink to doc comments after Task 3. Add a test asserting `readInput` is no longer a resolvable name.

- [ ] **Step 2: Run the removal test to verify it fails (name still resolves)**

Run: `cargo test -p rill-lang --features dsp --test stream_contract`
Expected: FAIL — `readInput` still resolves.

- [ ] **Step 3: Remove the builtins, retire the IR variant, pin the channel-mismatch error**

Delete the sigs and remaining handling; ensure `write` with a wrong channel count surfaces a clean error.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p rill-lang --features dsp`
Expected: PASS; grep shows no `readInput`/`writeOutput` in any program string (only doc anchors).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src rill-lang/tests
git commit -m "refactor(rill-lang): remove readInput/writeOutput remnants; clean output-channel mismatch errors"
```

---

### Task 5: rill-adrift migration + workspace verification

Migrate the umbrella crate's example programs to the stream form and verify the workspace stays within its known baseline.

**Files:**
- Migrate: `rill-adrift/examples/*.rs` and `rill-adrift/tests/*.rs` that embed old-form rill-lang source (`readInput`/`writeOutput`/buffer-λ `main`), e.g. `lang_chiptune.rs`, `record_mic.rs`, `play_wav.rs`: rewrite the program strings to `main = do { …; runStream handler }` + `mkStream`. Any `compile_graph`/`compile_with_ffi_channels` call site that the new entry points change must be updated.
- Modify: none structural — verification only, unless a migration reveals a compile entry point used by `rill-adrift` that must change signature.

**Interfaces:**
- Produces: a `rill-adrift` tree with no old-form program strings, building within the known baseline.

- [ ] **Step 1: Grep the umbrella crate for old-form programs**

```bash
rg -rn 'readInput|writeOutput|main [a-z0-9_]+ *=' rill-adrift --include '*.rs'
```

- [ ] **Step 2: Migrate the `rill-adrift` programs**

Apply the same transformation rule as Tasks 3-4; update any changed compile entry points.

- [ ] **Step 3: Verify the workspace**

Run: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp`, `cargo fmt --check`
Expected: green, zero warnings.
Run: `cargo check -p rill-adrift` (default features)
Expected: the only failures remain the known `graph`/`reconstruct.rs` baseline, unchanged from before this plan.

- [ ] **Step 4: Commit**

```bash
git add rill-adrift
git commit -m "refactor(rill-adrift): migrate example programs to the stream model"
```

---

## Self-Review note

- **Spec coverage:** §4.1 (IO reader over State+World; State carries the backend registry) — Tasks 2-3; §4.2 (streams replace readInput/writeOutput; mkStream creates backends inside the program) — Tasks 2-3; §4.3/§4.4 (passive/active; the active backend drives only the hot loop; `runStream` is the per-block contract) — Task 3; scalar-arg `main` (as String, §2 goal 1 + §6) — Task 3; the domain-agnostic ProgramRunner (north-star) — Task 3. Real backends (audio/file/console), by-threading passive/active nuances, the RT-safe hot-loop type, and in-language threads/actors are explicitly deferred to follow-on plans (per the I/O-1 decomposition).
- **Type consistency:** `runStream`'s handler params are the general `Stream Buffer` (first = master); `read`/`write`/`channel` stay on `InputStream`/`OutputStream` via `asInput`/`asOutput`; `mkStream :: BackendSpec -> IO (Stream Buffer, Stream Buffer)`. The per-tick IR shapes (`ValueReadInputs`/`ValueWriteOutputList`/`ValueReadInput`) match `ir.rs:539/641/658`.
- **Review Focus:** tests for all five lines are pinned to Tasks 3 and 4 as marked inline.
