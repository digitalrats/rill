# rill-lang Arrays Branch Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the remaining PARK and task-deferred review items from the `feature/rill-lang-arrays` branch, scoped as PARK M3/M7/M9/M10/M11 plus the deferred T4 M5/M6/M8, T5 M6/M7/M8, T7 M6, T8 M8. M12 (`lang_bench`/`graph`) is explicitly out of scope.

**Architecture:** Eleven independent, mostly small changes across the parser (`forall a. C a => T`), the constrained-signature machinery (two targeted "annotate the parameter's type" diagnostics; applied-head classification; `conv`/splice dedup; perf clones; `where`-shadowing precedence), the interpreter (a public `test_eval_io` execution surface with the executed result captured into the caller's world; pooled-extractor dedup; a `freeze_starray` pooled-buffer leak), the prelude span mapping, and two doc-cross-reference passes. Everything is behavior-preserving except where a test pins the new behavior; the value track is preserved as internal mechanics.

**Tech Stack:** Rust, `rill-lang` crate (Cargo workspace), `rill-core` traits, the existing libtest integration-test harness in `rill-lang/tests/`.

**Spec:** `rill/.superpowers/sdd/2026-10-18-rill-lang-arrays/progress.md` (PARK adjudication, lines 200-206) and `task-4-report.md` (~253-260), `task-5-report.md` (~392-401), `task-7-report.md` (~323-327), `task-8-report.md` (~401-408), `task-9-report.md` (~355-358). The plan argues from the ledger; executors read both.

## Global Constraints

- **Gates (must stay green, run after every task):** `cargo test -p rill-lang --features dsp`; `cargo test -p rill-adrift`; `cargo clippy -p rill-lang --lib --tests --features dsp` (zero warnings); `cargo fmt -- --check` (clean).
- Clippy `--all-targets` is blocked by a PRE-EXISTING stale `benches/lang_bench.rs` (M12, out of scope). Do not chase it; the `--lib --tests` gate is the correct one.
- Work **in place** on branch `feature/rill-lang-arrays`. No merge, no PR, branch stays.
- Commit messages use **single quotes** (`git commit -m '...'`) — never double quotes (backticks would be command-substituted by the shell).
- Docs, code comments, and commit messages in **English**.
- Do **not** introduce or reintroduce the identifiers `main_fragment` / `eval_run` / `run_tick`.
- Terminology: "automaton"/"automatons" (never "Automata"); "signal" (never "audio" outside `rill-io`/`rill-lofi`); `IoBackend` reads as "I/O backend".
- **M9/M10 depth is diagnostics only** ("annotate the parameter's type"); do NOT fix the underlying specialization/accessor limitations.
- **RealWorld principle (from the design conversation):** a fragment's result is observed through the world after execution. The value track computes up to the first pure value — for an effectful fragment that is the IO action, a declarative description of the computation (`{ unIO: World -> Pair World a }`) — and executing it against the world is what produces the result. Any result surfaced bypassing the world (a frozen, unexecuted value) is an anomaly; machinery/tests that exist only to support such bypasses is dead code to be removed once confirmed unexercised. The value track itself is internal mechanics and stays.
- No new external crates.

## Review Focus

1. **M7 parsing must not regress plain `forall a. T` or rank-N** — the change touches the forall-inner parse. Pinned by existing `forall_syntax.rs` tests staying green + the new class-context test in Task 1.
2. **M9/M10 diagnostics must fire only on the failure shape** — annotated and already-working constrained calls keep their current errors. Pinned by Task 2 tests running the constrained suites unchanged.
3. **`test_eval_io` must keep the caller's world intact for post-run analysis and must not disturb `ProgramRunner::run`** — the `IO ()` gate and payload-discard stay on the executable path. Pinned by Task 3 tests (world state after) + existing `io_runner`/`io_stdio`/`io_streams_memory` tests staying green.
4. **T4 M5/M6 head-args changes must not alter the pinned dispatch** — `head_arg_dispatch`, `constrained_defs`, `constrained_signatures`, `type_newtype` stay green. Pinned in Task 6.
5. **T5 M8 `where`-shadowing must not change routing for ordinary constrained calls** — only a genuinely shadowing where-local routes to the local. Pinned in Task 9.
6. **Refactors (T5 M6, T5 M7, T7 M6, T8 M8) are behavior-preserving** — the higher-kinded splice semantics (App/Data/Newtype/TyConApp), the M4 `debug_assert`, and the pooled-buffer return-on-failure pattern are all pinned by existing tests.
7. **M11(b) span remap must not change errors that already point at user code** — only prelude-body spans are remapped. Pinned in Task 4.

---

### Task 1: `forall a. C a => T` parsing (PARK M7)

**Files:**
- Modify: `rill-lang/src/parser.rs:743-756` (`parse_forall_type` — the inner parse at line 753-754)
- Test: `rill-lang/tests/forall_syntax.rs` (add a class-context case)

**Interfaces:**
- Consumes: `parse_sig_type` (parser.rs:760-768), which already parses a Haskell 2010 class context (`C v => T`, `(C v, C w) => T`) via `parse_sig_constraints`.
- Produces: `TypeExpr::TForall(names, TConstrained(constraints, body))` — a shape inference already consumes: `constrained_def_sig` (infer.rs:1320), `is_constrained_sig` (infer.rs:1312), and `sig_func_parts`/`sig_constraints` all look through the `forall` binder, so this is a parse-only gap.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn forall_with_class_context_parses_and_discharges() {
    // `forall i. Ix i => Pair i i -> i -> Int` — the quantified spelling of a
    // class context (Haskell 2010). Mirrors the non-quantified constrained
    // signatures; `Ix` and its `Pair Int Int` instance ship in the prelude.
    // Previously a parse error on the `=>` after the binder.
    let src = r#"
        off :: forall i. Ix i => Pair i i -> i -> Int = \b i -> index b i;
        main = off (0, 9) 9;
    "#;
    // `index (0,9) 9` is the offset of `9` in the range `(0,9)` per the prelude
    // `Ix` `Pair Int Int` instance; verify the exact expected value against the
    // instance body (likely `9`) before finalizing the assertion.
    assert_eq!(run(src), 9.0);
}
```

Use the `run` helper pattern from `forall_syntax.rs`/`head_arg_dispatch.rs` (compile + eval the `main` fragment, read a numeric result).

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p rill-lang --features dsp --test forall_syntax`
Expected: FAIL — parse error on `=>` after the forall binder (or an unexpected-token error).

- [ ] **Step 3: Implement in `parse_forall_type` (parser.rs:753-754)**

Change the inner parse from `self.parse_type_expr()?` to `self.parse_sig_type()?` so the forall body may carry a class context (`Ix i => T`). Plain `forall a. T` still parses unchanged (`parse_sig_type` falls through to `parse_type_expr` when no constraint is present). Update the `parse_forall_type` doc comment (lines 741-742) to mention the optional class context.

- [ ] **Step 4: Run the test to verify it passes, then the gates**

Run: `cargo test -p rill-lang --features dsp` and `cargo clippy -p rill-lang --lib --tests --features dsp`
Expected: the new test PASSes; the full suite and clippy are green (no existing `forall_syntax`/`constrained_signatures` regression).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/parser.rs rill-lang/tests/forall_syntax.rs
git commit -m 'feat(lang): parse a class context under an explicit forall binder'
```

---

### Task 2: Targeted "annotate the parameter's type" diagnostics (PARK M9 + M10)

**Files:**
- Modify: `rill-lang/src/lower.rs:4434-4436` (`lower_constrained_apply_inner` — the M9 diagnostic)
- Modify: `rill-lang/src/types/infer.rs:2161-2171` (`infer_def_body` λ-parameter `Float` default — the M10 diagnostic)
- Test: M10 in `rill-lang/tests/sig_typed_params.rs`; M9 in `rill-lang/tests/constrained_signatures.rs`

**Interfaces:**
- Consumes: the existing `lower_constrained_apply_inner` argument-lowering loop; the existing `infer_def_body` parameter-typing path.
- Produces: a targeted `CompileError::Type` whose message includes the exact phrase **`annotate the parameter's type`** and names the offending parameter, in both failure shapes. Diagnostics only — no change to how annotated/working definitions typecheck.

**M9 steps (bare named function into a function-shaped constrained parameter):**

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn bare_named_fn_into_function_shaped_param_gets_annotate_diagnostic() {
    // A bare named function passed to a FUNCTION-shaped constrained parameter
    // lowers to the empty `Thunk([],[])` shape and fails. The diagnostic must
    // name the parameter and suggest annotating its type (a λ-literal is the
    // working workaround).
    let err = eval_ffi(defs_with_function_shaped_constrained_def, bare_fn_call, ffi)
        .expect_err("the bare named fn into a function-shaped param must fail");
    assert!(err.to_string().contains("annotate the parameter's type"), "{err}");
}
```

Craft the defs/call on the `ixmap`-style shape the ledger pins (PARK M9: "bare named fn into function-shaped constrained param fails (`Thunk([],[])`); λ works around"). Mirror the existing constrained-def call patterns in `constrained_signatures.rs`.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p rill-lang --features dsp --test constrained_signatures`
Expected: FAIL — the current error does not contain the hint.

- [ ] **Step 3: Implement the M9 diagnostic (lower.rs:4434-4436)**

In `lower_constrained_apply_inner`, when lowering an argument against a `Thunk`-shaped `expected_arg` and the argument is a bare named function reference that produces the empty `Thunk([],[])` failure shape, return a targeted `CompileError::Type` that names the parameter and includes `annotate the parameter's type`. Do not attempt to fix the specialization.

- [ ] **Step 4: Run the test to verify it passes, then the gates**

Run: `cargo test -p rill-lang --features dsp` and clippy
Expected: the M9 test PASSes; the full suite (all constrained_*) and clippy are green.

**M10 steps (signature-less accessor):**

- [ ] **Step 5: Write the failing test**

```rust
#[test]
fn signature_less_accessor_param_gets_annotate_diagnostic() {
    // `at a i = a ! i` with no signature defaults `a` to the scalar Float and
    // fails confusingly. The diagnostic must name `a` and suggest annotating
    // its type.
    let err = eval_ffi("", r#"at a i = a ! i; main = at (0,9) 9;"#, ffi)
        .expect_err("the signature-less accessor must fail");
    assert!(err.to_string().contains("annotate the parameter's type"), "{err}");
}
```

- [ ] **Step 6: Run the test to verify it fails**

Run: `cargo test -p rill-lang --features dsp --test sig_typed_params`
Expected: FAIL — the current error (a Float-has-no-index / unification failure) lacks the hint.

- [ ] **Step 7: Implement the M10 diagnostic (infer.rs:2161-2171, 2251-2253)**

In `infer_def_body`, when an unannotated λ-parameter took the scalar `Float` default (line 2171 `unwrap_or(ValueTy::Float)` or the 2251-2253 `Var`→`Float` fallback) and the body then fails at a site that type-requires a structured value on that parameter (array indexing `a ! i`, a field projection, `length a`), augment the error with a targeted message naming the parameter and including `annotate the parameter's type`. Diagnostics only.

- [ ] **Step 8: Run the test to verify it passes, then the gates**

Run: `cargo test -p rill-lang --features dsp` and clippy
Expected: the M10 test PASSes; full suite and clippy green.

- [ ] **Step 9: Commit (both diagnostics, one commit)**

```bash
git add rill-lang/src/lower.rs rill-lang/src/types/infer.rs rill-lang/tests/constrained_signatures.rs rill-lang/tests/sig_typed_params.rs
git commit -m 'feat(lang): targeted annotate-the-parameter-type diagnostics for bare-fn and signature-less accessor failures'
```

---

### Task 3: Public `test_eval_io` execution surface + do-lazy contract tests (PARK M11a)

**Files:**
- Modify: `rill-lang/src/lib.rs:180-187` (add `test_eval_io` immediately after `test_eval`)
- Modify: `rill-lang/src/backend/interp.rs:274-353` (`run_main_io` — extract the apply-action core so the executed payload can be captured; `ProgramRunner` behavior unchanged)
- Create: `rill-lang/tests/test_eval_io.rs`
- Modify: `rill-lang/tests/st_array.rs` (pin the do-lazy contract explicitly; drop now-dead forcing workarounds)

**Interfaces:**
- Consumes: `WorldState::seat` (program.rs:410), `exec_entry` (interp.rs:86, value track — computes the first pure value, for an effectful fragment the IO description, into `world.main`), the extracted IO-apply core from `run_main_io`.
- Produces: `pub fn test_eval_io<T: Transcendental, const BUF: usize>(ir: &Ir, world: &mut WorldState<T, BUF>) -> Result<(), CompileError>` — seats the caller's independent world, executes the fragment's IO computation against it (value track yields the IO description → `unIO` applied to a World token → every effect runs into the world → the final pair is forced), and captures the **executed result** into `world.main` (`world.result()`), leaving the world (stdout/stderr/streams/params/arena) intact for post-run analysis. No `IO ()` unit gate (unlike `ProgramRunner::run`): any `IO a` fragment is executed and `a` is captured.

- [ ] **Step 1: Write the failing tests (`rill-lang/tests/test_eval_io.rs`)**

```rust
// Execution, not WHNF capture: effects run into the caller's independent
// world and the world survives for post-run analysis.
#[test]
fn io_effects_run_into_the_independent_world() {
    let ir = compile_eval::<f32, 256>("", r#"do { write (Just "hi") stdout; return () }"#, Mode::Complete).unwrap();
    let mut world = WorldState::<f32, 256>::new(WorldConfig {
        ffi: Some(ForeignRegistry::new()),
        ..Default::default()
    });
    test_eval_io(&ir, &mut world).unwrap();
    assert_eq!(world.stdout, ["hi"]); // the effect ran; the world survived
}

// The executed result is captured into the world, not discarded.
#[test]
fn executed_payload_is_captured_into_the_world() {
    let ir = compile_eval::<f32, 256>("", r#"do { x <- read stdin; return x }"#, Mode::Complete).unwrap();
    let mut world = WorldState::<f32, 256>::new(WorldConfig {
        ffi: Some(ForeignRegistry::new()),
        ..Default::default()
    });
    world.inject_stdin(&[String::from("hi")]);
    test_eval_io(&ir, &mut world).unwrap();
    let r = world.result().expect("the executed result is captured");
    assert!(matches!(world.arena().get(r).unwrap(), Value::String(s) if s == "hi"));
}

// do-lazy contract: a do-bound value inside a returned structure field stays
// a lazy thunk until forced; forcing yields the value.
#[test]
fn do_bound_field_is_lazy_until_forced() {
    let ir = compile_eval::<f32, 256>("", r#"do { x <- read stdin; return (Pair { first: x, second: x }) }"#, Mode::Complete).unwrap();
    let mut world = WorldState::<f32, 256>::new(WorldConfig {
        ffi: Some(ForeignRegistry::new()),
        ..Default::default()
    });
    world.inject_stdin(&[String::from("hi")]);
    test_eval_io(&ir, &mut world).unwrap();
    let r = world.result().expect("the executed result is captured");
    let Value::Record(fields) = world.arena().get(r).unwrap() else { panic!("expected a Pair") };
    // Contract: each field is a lazy thunk (Value::Callable) until forced;
    // forcing yields Value::String("hi"). Force both fields and assert.
}
```

Verify the exact prelude spellings (`write`/`read`/`stdout`/`stdin`/`Pair`) against `io_runner.rs` and `ty.rs` STD_PRELUDE before finalizing the fragments. Use `Mode::Complete` throughout.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p rill-lang --features dsp --test test_eval_io`
Expected: FAIL — `test_eval_io` is not defined; and the do-lazy fragment's fields are not forced by any surface.

- [ ] **Step 3: Implement the execution surface**

Extract the "apply the action to a World token, force the final pair, return the forced payload" core out of `run_main_io` (interp.rs:285-333) into a `pub(crate)` helper (e.g. `fn exec_io_to_payload(ir, world, action_ref, drops) -> Option<ArenaRef>`). `run_main_io` keeps its `IO ()` unit gate and drops the payload (ProgramRunner behavior unchanged). Add `pub fn test_eval_io` in lib.rs after `test_eval`: `world.seat(ir)?;` then run `exec_entry` (builds the IO description into `world.main`), apply the action via the helper, and store the forced payload into `world.main` so `world.result()` returns it. Document per the RealWorld principle: the result is observed through the surviving world; the value track computes the IO description (first pure value), execution against the world produces the result.

- [ ] **Step 4: Run the tests to verify they pass, then the gates**

Run: `cargo test -p rill-lang --features dsp` and clippy
Expected: the three `test_eval_io` tests PASS; full suite (io_runner/io_stdio/io_streams_memory/compile_gate included) and clippy green — ProgramRunner untouched.

- [ ] **Step 5: Pin the do-lazy contract in the affected ST tests and drop dead forcing**

Rewrite `rill-lang/tests/st_array.rs:96-98` (`starray_read_write_modify_inside_a_region`) and the forcing-op patterns the T8 test-shape note names (`b.first`, `length es`, `arrayFromList`+`arrayIndexFlat`, direct-effect regions) so the do-lazy contract is explicit: a comment plus assertions that a `do`-bound value in a structure field is a lazy thunk until forced and the value after forcing. Remove forcing workarounds that are now redundant because the contract is pinned. Value-track mechanics themselves are untouched.

- [ ] **Step 6: Dead-code inventory and removal**

Grep the result-surfacing paths that bypass the world (frozen value-track captures in effect tests, the forcing-op workarounds). After Step 5, confirm each is unexercised by any live path and remove it; where a path is still live (the value track itself, `exec_runst`/`run_io_action` internal results), leave it and note why. Re-run the full suite + clippy.

- [ ] **Step 7: Commit**

```bash
git add rill-lang/src/lib.rs rill-lang/src/backend/interp.rs rill-lang/tests/test_eval_io.rs rill-lang/tests/st_array.rs
git commit -m 'feat(lang): add test_eval_io execution surface and pin the do-lazy contract'
```

---

### Task 4: Prelude error spans (PARK M11b)

**Files:**
- Modify: `rill-lang/src/types/ty.rs:837-846` (`register_prelude` — mark prelude-registered definitions' spans as prelude spans)
- Modify: the span-rendering/reporting path that surfaces `CompileError::Type { span, .. }` — remap prelude spans
- Test: new case in `rill-lang/tests/constrained_signatures.rs` or `rill-lang/tests/sig_typed_params.rs`

**Interfaces:**
- Consumes: `Span` (error.rs), the spans carried by `register_prelude`-registered defs/instances.
- Produces: for an error whose reported span currently falls inside the `STD_PRELUDE` compiled-in text, a user-meaningful span (the user's call site, or a synthetic "standard prelude" marker); errors already pointing at user code are unchanged.

- [ ] **Step 1: Write the failing test**

Identify the concrete repro from the T8/T9 notes ("prelude error spans pointing at the prelude body" — the `index` dual-name routing, or the `runSTArray` body-shape error, both reach prelude-registered signatures). Assert the error message's span is meaningful (the user's call site or a `standard prelude` marker), not a raw byte offset into the prelude body:

```rust
#[test]
fn error_span_into_the_prelude_body_is_remapped() {
    let err = eval_ffi(defs_triggering_a_prelude_span_error, expr, ffi)
        .expect_err("the prelude-span error must still fire");
    let text = err.to_string();
    // The reported span must not be a raw offset into the compiled-in prelude.
    assert!(!looks_like_a_prelude_body_offset(&text), "{text}");
    assert!(text.contains("standard prelude") || text.contains("annotate"), "{text}");
}
```

Implement `looks_like_a_prelude_body_offset` as: the message carries a span whose offset is below `STD_PRELUDE.len()` while the user source is longer — i.e. the span cannot be a user source position. Adjust to the exact repro you confirm.

- [ ] **Step 2: Run the test to verify it fails**

Run: the owning test target
Expected: FAIL — the current message points at the prelude body offset.

- [ ] **Step 3: Implement the remap**

Mark prelude-registered definitions' spans as prelude spans (a provenance flag on registration in `register_prelude`, or a recorded `STD_PRELUDE` byte range). In the span-reporting path, when a reported span is a prelude span, remap it to the user's call-site span when one is available, else to a synthetic `standard prelude` marker. Leave every span that already points at user code untouched.

- [ ] **Step 4: Run the test to verify it passes, then the gates**

Run: `cargo test -p rill-lang --features dsp` and clippy
Expected: the new test PASSes; all existing error-message tests (which assert exact spans/messages) stay green.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/ty.rs <error-path-file> <test-file>
git commit -m 'fix(lang): remap error spans that land inside the prelude body'
```

---

### Task 5: Dispatch-site doc cross-references (PARK M3 + T4 M8)

**Files:**
- Modify: `rill-lang/src/types/infer.rs:4321` (`method_shadowed_by_def`), `:4345` (`probe_first_arg_solves_class_var`)
- Modify: `rill-lang/src/lower.rs:4265-4287` (`method_shadowed_by_def`), `:4303` (`peek_value_ty`)

**Interfaces:** None — doc comments only, no behavior change.

- [ ] **Step 1: Add the cross-references**

PARK M3: cross-reference the shadowing-predicate sites and document the intentional asymmetry — inference's `method_shadowed_by_def` (infer.rs:4321) ↔ lowering's `method_shadowed_by_def` (lower.rs:4265) share the predicate but differ on fragment captures (lowering sees `fragment_captures`; inference never does — intentional and safe, per the existing lower.rs:4266-4267 note). Add the missing doc link from `probe_first_arg_solves_class_var` (infer.rs:4345) to `peek_value_ty` (lower.rs:4303), the probe-sandbox counterpart, and back.

T4 M8: audit the infer/lower dispatch pairs (`first_arg_solves_class_var` ↔ `lower_constrained_apply_site`'s routing predicate, `method_dispatch_key` ↔ `method_shadowed_by_def`) and add the missing cross-reference comments. Most is already in place; the residual is the links above.

- [ ] **Step 2: Verify**

Run: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --lib --tests --features dsp`, `cargo fmt -- --check`
Expected: all green (doc-only; no behavior change).

- [ ] **Step 3: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/src/lower.rs
git commit -m 'docs(lang): cross-reference the shadowing and dispatch sites across infer/lower'
```

---

### Task 6: Applied-head classification and head-args binding (T4 M5 + M6)

**Files:**
- Modify: `rill-lang/src/types/infer.rs:911-924` (`class_var_selector_index` — M5)
- Modify: `rill-lang/src/types/infer.rs:1280-1304` (`prepend_head_args_into_class_var` — M6)
- Test: `rill-lang/tests/head_arg_dispatch.rs` (M5), a newtype container case (M6)

**Interfaces:**
- Consumes: `class_var_param_indices`, the `is_bare_nullary_method_ref` predicate (M5); `ValueTy::{Data,App,Thunk}` arms (M6).
- Produces: a selector-index classification that distinguishes a head argument that is itself a partial application from a bare variable (M5); head-arg prepending under `Newtype` and `TyConApp` containers (M6).

- [ ] **Step 1: Write the M5 failing test (`head_arg_dispatch.rs`)**

The T4 M5 gap: "a head arg that is itself a partial application is not distinguished from a bare variable". Craft a dual-name / class-method call whose first argument is itself a partial application and assert the class-variable selector index picks the argument that actually solves the class variable (reproduced against the probe class `C` pattern already in `head_arg_dispatch.rs`). Assert the correct instance dispatches.

- [ ] **Step 2: Run the M5 test to verify it fails**

Run: `cargo test -p rill-lang --features dsp --test head_arg_dispatch`
Expected: FAIL — the current classification picks the wrong selector.

- [ ] **Step 3: Implement the M5 classification (infer.rs:911-924)**

Refine `class_var_selector_index` so an applied/partial-application head argument is not treated as a bare variable when scanning for the class-variable selector index. Keep the existing fallback behavior for genuinely bare nullary method references.

- [ ] **Step 4: Run the M5 test to verify it passes**

Run: `cargo test -p rill-lang --features dsp --test head_arg_dispatch`
Expected: PASS.

- [ ] **Step 5: Write the M6 failing test (newtype container case)**

A constrained signature whose class variable appears under a `Newtype` or `TyConApp` container must have its instance head args prepended (the def typechecks and dispatches). Use the existing `type_newtype.rs` machinery (a user `newtype` + a constrained def whose signature mentions the newtype-shaped class variable under a function/container position). Assert the def typechecks and the instance dispatches.

- [ ] **Step 6: Run the M6 test to verify it fails**

Run: the owning test target
Expected: FAIL — the head args are not prepended under the `Newtype`/`TyConApp` container (the `_ => {}` arm at infer.rs:1302 drops them).

- [ ] **Step 7: Implement the M6 arms (infer.rs:1280-1304)**

Add `ValueTy::Newtype` and `ValueTy::TyConApp` arms to `prepend_head_args_into_class_var`, mirroring the existing `Data`/`App` arms: prepend `head_values` ahead of the container's own args, recursing through `Thunk`. Leave the catch-all for genuinely opaque kinds.

- [ ] **Step 8: Run the M6 test to verify it passes, then the gates**

Run: `cargo test -p rill-lang --features dsp` and clippy
Expected: both new tests PASS; `head_arg_dispatch`, `type_newtype`, `constrained_*` and the rest of the suite stay green.

- [ ] **Step 9: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/tests/head_arg_dispatch.rs <newtype-test-file>
git commit -m 'fix(lang): classify applied heads as selectors and prepend head args under newtype containers'
```

---

### Task 7: `conv` / higher-kinded splice dedup (T5 M6)

**Files:**
- Modify: `rill-lang/src/types/infer.rs:1149-1236` (`conv`) and `:1515+` (`sig_ty_to_vty`)

**Interfaces:**
- Consumes: `is_declared_type_name`, `ClassVarSubst`, the `TApp`/`TName`/`TFunc`/`TForall`/`TConstrained` spine.
- Produces: one shared TypeExpr→ValueTy conversion helper; `conv` and `sig_ty_to_vty` both route through it. The higher-kinded splice semantics — the `TApp(head) if head == class_var` arm's `App`/`Data`/`Newtype`/`TyConApp` head-splicing (infer.rs:1170-1202) — must be preserved EXACTLY.

- [ ] **Step 1: Confirm the baseline**

Run: `cargo test -p rill-lang --features dsp` — green before the refactor.

- [ ] **Step 2: Implement the dedup**

Extract the shared TypeExpr→ValueTy conversion (the declaration-name handling and container-flavour logic `data_arities` → `Data` vs `App`) into one helper parameterized by the class-variable substitution, and route `conv` and `sig_ty_to_vty` through it. The higher-kinded splice arm stays intact (task-5 report: "`conv` now shares the declared-name predicate but still has its own class-variable handling" — this is the residual to fold). This is a behavior-preserving refactor; the constrained/higher-kinded tests are the safety net.

- [ ] **Step 3: Verify the gates**

Run: `cargo test -p rill-lang --features dsp` and clippy
Expected: all green — especially `constrained_defs`, `constrained_signatures`, `head_arg_dispatch`, and the HKT tests.

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/types/infer.rs
git commit -m 'refactor(lang): share the TypeExpr-to-ValueTy conversion between conv and sig_ty_to_vty'
```

---

### Task 8: `def_sig_tys` / `effective_def_params` clone removal (T5 M7)

**Files:**
- Modify: `rill-lang/src/types/infer.rs:1369` (`def_sig_tys` — `let env = ctx.env.clone();` per call)
- Modify: `rill-lang/src/types/infer.rs:2062` (`effective_def_params` — `def.body().clone()` per call)

**Interfaces:** None — compile-time performance, behavior-preserving.

- [ ] **Step 1: Confirm the baseline**

Run: `cargo test -p rill-lang --features dsp` — green.

- [ ] **Step 2: Implement**

`def_sig_tys` (infer.rs:1365-1371): replace the per-call `ctx.env.clone()` with a borrow of `ctx.env` (check the borrow lifetimes of the `def_sig_tys_conv` signature; restructure the parameter if needed). `effective_def_params` (infer.rs:2054-2063): avoid cloning `def.body()` per call — return the borrow or restructure the caller so the body is borrowed. Compile-time only.

- [ ] **Step 3: Verify the gates**

Run: `cargo test -p rill-lang --features dsp` and clippy
Expected: all green; no behavior change (the constrained and annotated-def suites pass unchanged).

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/types/infer.rs
git commit -m 'perf(lang): drop per-call clones in def_sig_tys and effective_def_params'
```

---

### Task 9: `where`-shadowing precedence in the constrained intercept (T5 M8)

**Files:**
- Modify: `rill-lang/src/lower.rs:4230-4238` (`lower_constrained_apply_site`)
- Test: `rill-lang/tests/constrained_defs.rs`

**Interfaces:**
- Consumes: `self.defs` (where-block locals land here — lower.rs:3608), `env.constrained_defs`, the existing `value_locals`/`fragment_captures` shadow check (lower.rs:4234-4238).
- Produces: the constrained `Apply` intercept declines when a `where`-block local shadows the constrained definition's name, so the call routes to the local — mirroring the bare-reference diagnostic's respect for `value_locals`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn where_local_shadows_a_constrained_def() {
    // A `where`-block local whose name collides with a class-constrained
    // definition must win: the call resolves to the local, not the constrained
    // def. Currently the intercept routes through `env.constrained_defs` first.
    let src = r#"
        cd :: Ix i => Pair i i -> Int = \b -> index b b;
        main = let r = cd (0, 9) in r where { cd = 42.0; }
    "#;
    assert_eq!(run(src), 42.0); // the where-local, not the constrained def
}
```

Adjust the fragment so the where-local is used in a position that type-checks (a numeric where-local bound to the result is the shape to aim for); the assertion is that the where-local's value wins.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p rill-lang --features dsp --test constrained_defs`
Expected: FAIL — the call routes to the constrained def (or fails), not the where-local.

- [ ] **Step 3: Implement (lower.rs:4230-4238)**

In `lower_constrained_apply_site`, add the where-local shadow check alongside the existing `value_locals`/`fragment_captures` check so a where-local (a `self.defs` entry introduced by a `where` block) shadows the constrained definition. Route ordinary constrained calls exactly as before.

- [ ] **Step 4: Run the test to verify it passes, then the gates**

Run: `cargo test -p rill-lang --features dsp` and clippy
Expected: the new test PASSes; all constrained_* and dual-name (`index`) tests stay green.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lower.rs rill-lang/tests/constrained_defs.rs
git commit -m 'fix(lang): let a where-block local shadow a class-constrained definition'
```

---

### Task 10: Pooled-extractor dedup (T7 M6)

**Files:**
- Modify: `rill-lang/src/backend/interp.rs:1984-2071` (`pooled_elems_of`, `pooled_elems_of_ref`, `pooled_array_elems_of`)

**Interfaces:**
- Consumes: `Arena::take_buf`, `latch_exhaustion`, `world.value_regs`, `world.arena.get`.
- Produces: one generic pooled-payload extractor; all three call sites route through it. Behavior-preserving — the M4 `debug_assert_eq!(out.len(), len)` guard (interp.rs:2069) is preserved.

- [ ] **Step 1: Confirm the baseline**

Run: `cargo test -p rill-lang --features dsp` — green.

- [ ] **Step 2: Implement**

Fold the three near-identical pooled-payload extractors into one generic helper (parameterized by the container arm — `List`/`Set`/`Array` — and by reg-vs-ref source). Preserve the exhaustion latching ("value buffer pool") and the array M4 debug_assert exactly. No behavior change (task-7 report: "Working as intended; the duplication ... is a code smell worth folding into one generic helper").

- [ ] **Step 3: Verify the gates**

Run: `cargo test -p rill-lang --features dsp` and clippy
Expected: all green — especially `array_storage`, `st_array`, and the collections tests that exercise the pooled paths.

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/backend/interp.rs
git commit -m 'refactor(lang): fold the pooled-payload extractors into one generic helper'
```

---

### Task 11: `freeze_starray` pooled-buffer leak on exhaustion (T8 M8)

**Files:**
- Modify: `rill-lang/src/backend/interp.rs:5480-5489` (`freeze_starray`)
- Test: extend the exhaustion test pattern in `rill-lang/tests/st_array.rs`

**Interfaces:**
- Consumes: `st_array_bounds`, `st_array_elems_of` (returns a pooled buffer), `alloc_owned`, `Arena::put_buf`.
- Produces: on `alloc_owned` failure (arena exhaustion) at interp.rs:5489, the already-`take_buf`'d `elems` buffer is returned to the pool via `world.arena.put_buf(elems)` — mirroring the existing bounds-failure guard at 5480-5483. Leak on a fatal path only; no correctness change when allocation succeeds.

- [ ] **Step 1: Write the failing test**

Extend the existing exhaustion test harness in `st_array.rs` (the `freeze_keeps_the_element_claims_balanced` / latched-fault pattern) to force `alloc_owned` failure inside `freeze_starray` and assert the pooled buffer count is balanced afterward (the element buffer was returned to the pool). Use the `fault`/`run` helpers already in the file; drive the arena to exhaustion so `alloc_owned` at interp.rs:5489 returns `None` after `st_array_elems_of` already took the buffer.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p rill-lang --features dsp --test st_array`
Expected: FAIL — the pool leaks one buffer on the exhaustion path.

- [ ] **Step 3: Implement (interp.rs:5489)**

When `alloc_owned(ir, world, Value::Array { bounds, elems })` fails (returns `None`), call `world.arena.put_buf(elems)` before returning `None` — exactly the pattern already used at 5480-5483 for the bounds-copy failure. The latched exhaustion error is unchanged.

- [ ] **Step 4: Run the test to verify it passes, then the gates**

Run: `cargo test -p rill-lang --features dsp` and clippy
Expected: the new test PASSes; all `st_array`/`array_storage` tests stay green.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/backend/interp.rs rill-lang/tests/st_array.rs
git commit -m 'fix(lang): return the pooled element buffer to the pool when freezing an STArray exhausts the arena'
```

---

## Final verification (after the last task)

- [ ] `cargo test -p rill-lang --features dsp` — green
- [ ] `cargo test -p rill-adrift` — green
- [ ] `cargo clippy -p rill-lang --lib --tests --features dsp` — zero warnings
- [ ] `cargo fmt -- --check` — clean
- [ ] `git status` — clean tree; branch `feature/rill-lang-arrays` in place (no merge, no PR)
