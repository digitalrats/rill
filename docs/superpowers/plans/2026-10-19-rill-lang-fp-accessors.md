# First-Class λ-Parameters: Signature-Less Accessors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make signature-less curried accessors such as `at a i = a ! i` compile and run without a signature — the λ-parameter types (`a : Array i e`, `i : Ix`) become first-class, recovered in inference and specialized per call site in lowering — and remove the `anchor_param_tys` side-channel this replaces.

**Architecture:** (1) Inference already recovers a curried def's λ-parameter types into its scheme (the def is typed as a VALUE — its λ-chain — and its scheme is `Thunk([Array i e, i], [e])`); the blocker is that the body's `a ! i` reaches the typeclass dispatch for `index`/`inRange` with an unsolved index var, which fails in LOWERING. (2) Lowering will specialize signature-less curried defs at the call site like it already does for class-constrained defs (`lower_constrained_apply_inner`): lower the arguments, bind the λ-parameters to the argument registers with their solved types, lower the body against that scope, check the result. (3) The recovered λ-parameter types move into the scheme for the remaining (non-curried) cases that today rely on `anchor_param_tys`, then that field is deleted.

**Tech Stack:** `rill-lang` (Rust), `types/infer.rs`, `types/ty.rs`, `lower.rs`, `render.rs`. No new dependencies. Gates: `cargo test -p rill-lang --features dsp`, `cargo test -p rill-adrift`, `cargo clippy -p rill-lang --lib --tests --features dsp` (0 warnings), `cargo fmt -- --check`. `--all-targets` stays blocked by the stale `lang_bench` (PARK M12, out of scope).

**Spec:** PARK M9/M10 from `rill/.superpowers/sdd/2026-10-18-rill-lang-arrays/progress.md:203-206`, and the deferred-specialization contract pinned by `rill-lang/tests/constrained_defs.rs`. The user-confirmed scope (brainstorm, 2026-10-19): unannotated accessors become working via deferred per-call-site specialization (the constrained-def mechanism); `anchor_param_tys` disappears; genuinely-unbound params stay `Float`; bang/force behavior unchanged; M10's "annotate the parameter's type" diagnostic becomes subsumed (working calls) or sharper (mismatching calls). PARK M9 (bare named fn into a function-shaped constrained param) stays as the shipped diagnostic — out of scope here.

## Global Constraints

- **Verification gates (all must be green at the end):** `cargo test -p rill-lang --features dsp`; `cargo test -p rill-adrift`; `cargo clippy -p rill-lang --lib --tests --features dsp` (0 warnings); `cargo fmt -- --check`. `--all-targets` blocked by stale `lang_bench` (PARK M12) — do not attempt.
- **No blanket `#[allow(...)]`.** New public items need doc comments. Existing site-level `#[allow(clippy::too_many_arguments)]` on `lower_fragment` stays.
- **Commit messages single-quoted** (`git commit -m '...'`). English text for code, comments, docs, and commit messages. Branch `feature/rill-lang-arrays-fp` stays without merge/PR until the user closes it.
- **λ-parameter storage is unchanged:** λ-params are signal scalars / fragment-local registers (v1 model). This plan changes which TYPES lowering assigns those registers (from the scheme instead of the `anchor_param_tys` side-channel), not the register ABI.
- **`index` is dual-named** (the `Ix.offset` method and the array accessor) — every routing predicate (`first_arg_solves_class_var`, `method_shadowed_by_def`, the dual-name decline in `lower_constrained_apply_site`) must keep choosing the accessor for an `Array` first argument. The `constrained_defs.rs` dual-name tests must stay green unchanged.
- **Do not regress the bang contract:** `at !a i :: Array Int Float -> Int -> Float = a ! i` must still emit exactly one `ValueForce` (`bang_patterns.rs:247-254`).
- **Do not regress constrained defs:** `constrained_defs.rs` (deferral, per-call-site specialization, non-instanced type error, partial application, bare-ref error) stays green unchanged.
- **genuinely-unbound λ-params stay `Float`:** `add a b = a + b` keeps `Float`-typed params and eager (non-deferred) typing.
- **Worktree:** start from a clean worktree/feature branch off `develop`. (The `rill/` repo uses git-flow; `feature/rill-lang-arrays-fp`.)

## Review Focus

1. **A signature-less curried def used as a bare VALUE** (no application — `g = at`, `map at arr`): today the λ-chain types it and it may work in some positions; the target behavior must match the constrained-def contract ("cannot use class-constrained definition without applying it") — the call-site-specialization path has no call site, so a bare ref must error with a clear message, not silently bind `Float`-typed registers. Pin with a test in Task 2.
2. **A signature-less curried def with MIXED params** — one class-demanding (`a`) and one scalar (`k`), e.g. `atk a k = a ! k`: the scalar param must stay `Float`/scalar while the array param specializes per call. Task 3's scheme must not flatten every param to the recovered class type. Pin in Task 3.
3. **Call-site arg whose type is a *nested* `Array`/`STArray` index** (2-D `Pair` index, or `checkedOffset`-style `Int` offset): the specialized body must discharge `Ix (Pair Int Int)` the same way the prelude `index` does for the annotated case (`array_immutable.rs:266-274`). Pin a 2-D read in Task 2.
4. **Recursive or mutually-recursive signature-less accessor** (`at a i = if i == 1 then a ! i else at a (i - 1)`): the specialization guard (`constrained_lifting`) must catch recursion with the constrained-def message, and `infer_def_group`'s Phase 2/second pass must not double-infer a deferral-flagged def into two different schemes. Cover in Task 5.
5. **Render idempotence when the same def is typed** (`render` typed-program round-trip): the plan never writes a synthesized signature into the AST (`Def::Eval.sig` stays `None`), so `render` output must be unchanged for signature-less defs. Pin a round-trip test in Task 6.

---

### Task 1: Pin the target behavior with failing tests

**Files:**
- Create: `rill-lang/tests/accessor_first_class.rs`
- Use helpers from `rill-lang/tests/common/mod.rs` (`eval`, `eval_mode`, `run_value`-style local helpers if the file needs its own — check `sig_typed_params.rs` for the local helper pattern)

**Interfaces:**
- Consumes: `eval(defs, expr)` → `Result<WorldState<f32,256>, CompileError>` (Strict); `eval_mode::<256>(defs, expr, ffi, Mode::Complete)` for lazy mode. `compile_eval`/`test_eval` under the hood.
- Produces: the RED suite the rest of the plan turns green — `accessor_first_class.rs`.

- [ ] **Step 1: Write the failing tests**

```rust
// accessor_first_class.rs — mod common; use common::*;
#[test]
fn signature_less_array_accessor_reads_a_cell_in_strict() {
    // park10: today this dies in lowering with
    // "no instance of `Ix` for type `Pair` and no default for `inRange`".
    let (defs, expr) = (
        "at a i = a ! i;",
        "at (listArray (Pair { first: 0, second: 2 }) [1.0, 2.0, 3.0]) 1",
    );
    assert_eq!(run_num(...), 2.0); // via eval + out_value, matching sig_typed_params helpers
}

#[test]
fn signature_less_array_accessor_reads_a_cell_in_complete() {
    // same, but eval_mode(.., Mode::Complete) == 2.0
}

#[test]
fn signature_less_st_accessor_reads_a_cell_in_strict() {
    // `readAt st i = readArray st i` over ST — see st_array.rs for the ST surface.
    // If the ST surface needs a runST wrapper, mirror tests/st_array.rs's harness.
}

#[test]
fn signature_less_accessor_works_with_a_20_index_pair() {
    // at a i = a ! i; on a 2-D bounds (Pair {first:0, second:2}, ...) with a Pair index —
    // mirrors array_immutable.rs:266-274 2-D spell.
}

#[test]
fn genuinely_unbound_params_stay_float() {
    // add a b = a + b; main = add 2.0 3.0  == 5.0 — PASSES today, regression pin
}

#[test]
fn signature_less_list_accessor_keeps_working() {
    // nth xs k = head (tail xs); main = nth [10.0, 20.0] 0 == 10.0 — PASSES today, pin
}

#[test]
fn mismatching_call_still_names_the_annotation() {
    // at (0, 9) 9 → CompileError whose msg contains "annotate the parameter's type"
    //   and "`at`" (keeps the PARK M10 diagnostic for bad calls)
}
```

- [ ] **Step 2: Run and confirm the expected split**

Run: `cargo test -p rill-lang --features dsp --test accessor_first_class -- --nocapture`
Expected: the first three array/ST accessor tests FAIL (today: `no instance of `Ix` for type `Pair` and no default for `inRange``); the three regression pins PASS; the M10 test PASSES (diagnostic already shipped in `71225b5`).

- [ ] **Step 3: Commit the RED suite**

```bash
git add rill-lang/tests/accessor_first_class.rs
git commit -m 'test(lang): pin signature-less accessor target behavior (RED)'
```

---

### Task 2: Reuse the constrained specialization in lowering for signature-less curried defs

**Files:**
- Modify: `rill-lang/src/lower.rs` (`Expr::Apply` arm ~1725-1740, near `lower_constrained_apply_site` ~4222-4255, `lower_constrained_apply_inner` ~4402-4503)
- Test: `rill-lang/tests/accessor_first_class.rs`

**Interfaces:**
- Consumes: `fn_param_tys: HashMap<String, Vec<ValueTy>>` (built at `types/infer.rs:1980-1998` — flattened λ-param list per curried def, resolved through the final substitution; already populated for `at` with `[Array Var i e, Var i]`); `Lowerer::value_locals`, `fragment_captures`, `lower_value_expected`, `pending_param_tys`, `lower_with_where_defs`.
- Produces: `fn lower_signature_less_apply_site(&mut self, name: &str, args: &[Expr], expected: Option<&ValueTy>, span: Span) -> Option<Result<(usize, ValueTy), CompileError>>` — a mirror of `lower_constrained_apply_site` for signature-less curried defs; `Option` so the caller falls through to the existing path when not applicable.

Implementation notes (decisions locked, bodies by the implementer):
- **Trigger:** in `Expr::Apply`, before the existing lambda-inline/scheme paths, if `self.env.defs` (or the fragment's defs) has a signature-less `Def::Eval` with `params().is_empty()`, body a λ-nest (`Expr::Lambda`), whose scheme `fn_param_tys`/flattened type is a `Thunk` AND whose flattened λ-param list contains at least one non-`Float` type with a would-be class demand — fall back exactly like the constrained def: if a local value/fragment capture shadows the name, or `method_shadowed_by_def`/dual-name applies, return `None`. (Reuse the existing guard shape from `lower_constrained_apply_site` at `lower.rs:4229-4249`.)
- **Full-arity path** (args.len() == params len): lower each arg; build `scope: HashMap<String,(usize,ValueTy)>` binding λ-param names to arg regs and their SOLVED static types; lower the body with that scope (a `value_locals.push(scope)`; mirror `lower_constrained_apply_inner` body-lowering + result check). If the body result type disagrees with the scheme result, error (like `check_specialized_result` at `lower.rs:4496-4503`).
- **Partial application** (args.len() < params): lower the applied args, bind their λ-params, and set `pending_param_tys = Some(remaining scheme param tys)` so the trailing λ-literal of the returned closure types its params (mirror `lower.rs:4482-4485`).
- **Bare reference** (`g = at`, no application): return `None` today so the existing behavior stays; the hard error is Task 5.

- [ ] **Step 1: Add the call-site routing + full-arity specialization (strict path)** — implement `fn lower_signature_less_apply_site` (full-arity only) and wire it into the `Expr::Apply` arm next to `lower_constrained_apply_site`.
- [ ] **Step 2: Run Task 1 suite** — `cargo test -p rill-lang --features dsp --test accessor_first_class` — the strict array + 2-D accessor tests must now PASS. Keep the M10 test (Pair arg) passing.
  - **Working hypothesis to verify here:** the Task-1 RED failures surface in LOWERING (the assembled-program span), not in inference — the probe showed an unreferenced `at` compiles, i.e. the λ-body's class demand defers during inference and only the lowered body fails. If Step 2 shows the failure instead happens during inference of the call (a `unify`/discharge error at the `at` argument's span), STOP and report back: the fix boundary shifts to `infer.rs` (record pending λ-param class demands into the scheme and discharge at the call), and Tasks 2's split changes accordingly.
- [ ] **Step 3: Partial-application arm** — add the `args.len() < params` branch.
- [ ] **Step 4: Full rill-lang suite** — `cargo test -p rill-lang --features dsp` green; `cargo test -p rill-adrift` green. No regressions in `array_immutable`, `st_array`, `ix`, `bang_patterns`, `constrained_defs`, `sig_typed_params`.
- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lower.rs rill-lang/tests/accessor_first_class.rs
git commit -m 'feat(lang): specialize signature-less curried accessors per call site'
```

---

### Task 3: Carry recovered λ-parameter types in the scheme (non-curried + mixed)

**Files:**
- Modify: `rill-lang/src/types/infer.rs` (`def_scheme_vty` ~2475-2494; `infer_def_body` ~2249-2268 retry-recording; `effective_def_params` ~2064-2073)
- Modify: `rill-lang/src/types/infer.rs` `fn_param_tys` builder (~1980-1998) if it must also cover single-param signature-less defs
- Test: `rill-lang/tests/accessor_first_class.rs` + `rill-lang/tests/sig_typed_params.rs`

**Interfaces:**
- Consumes: `DefBodyTy { ty, sig_params }`; the retry path's `param_tys` computation (infer.rs:2250-2266); `free_vars_quantified`.
- Produces: `def_scheme_vty` uses the RETRY-RECOVERED `param_tys` (infer.rs:2250-2266) instead of the `(0..lam_count).map(|_| Float)` fallback, so a signature-less single-param def (`first p = p.x`, `swap`-style) gets a `Thunk([RecoveredType], [ret])` scheme. The genuinely-unbound stays `Float` (a `Var` that never unifies still maps to `Float`, matching infer.rs:2261-2263).

Implementation notes:
- The current `def_scheme_vty` uses `body_ty.sig_params` (signature-driven) for annotated defs and the Float fallback otherwise (`infer.rs:2492`). Extend `DefBodyTy` with a third field `lambda_param_tys: Vec<ValueTy>` (recovered λ-param types from the retry, or empty when the first attempt typed the body with signature types), and have `def_scheme_vty` prefer it (when present and length == lam_count) over the Float fallback. Signature-annotated defs keep `sig_params`.
- **Mixed params (Review Focus 2):** `atk a k = a ! k` — the retry recovery already resolves `a : Array i e` and leaves `k` a `Var` → Float, so the scheme becomes `Thunk([Array i e, Float], [e])`. Add the mixed accessor to `accessor_first_class.rs` in Step 1.
- This is what lets Task 4 delete `anchor_param_tys`: the scheme (and `fn_param_tys`, rebuilt from the scheme) now carries the data.

- [ ] **Step 1: Add a mixed-param test** (`atk a k = a ! k`, `k` unused-but-present scalar) to `accessor_first_class.rs`; confirm it RED today.
- [ ] **Step 2: Extend `DefBodyTy` + `def_scheme_vty`** to carry recovered λ-param types as above; keep every annotated-def behavior identical (`sig_params` untouched).
- [ ] **Step 3: Run** `cargo test -p rill-lang --features dsp` — full suite green, no regression in `sig_typed_params` (schemes for `f x :: a -> a` etc.) or `bang_patterns`.
- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/tests/accessor_first_class.rs
git commit -m 'feat(lang): carry recovered lambda-parameter types in schemes'
```

---

### Task 4: Remove the `anchor_param_tys` side-channel

**Files:**
- Modify: `rill-lang/src/types/infer.rs` — field (`Ctx`, ~103; `TypedProgram`, ~47), producers (`:2196-2201`, `:2249-2268`), the clone into `TypedProgram` (~2045), the `Ctx` constructor clones (~1806, ~1921, ~2045, unit-test ~6705)
- Modify: `rill-lang/src/lower.rs` — field (`Lowerer` ~717), consumers (C1 `:4948-4953`, C2 `:5567-5574`), and every `TypedProgram`/`Lowerer` clone site (`:7849, :8092, :8162, :8336, :8364, :8393, :8427`)
- Test: full suites

**Interfaces:**
- Consumes: Task 3's scheme-carried λ-param types; `fn_param_tys` rebuilt from schemes; `flatten_thunk` for curried defs.
- Produces: the `anchor_param_tys` key entirely gone from `TypedProgram`, `Ctx`, `Lowerer` (and `Ctx` no longer needs it before/after `infer_def_body`).

Implementation notes:
- **Replace C1/C2 consumers with scheme/flatten-derived types.** Both consumers currently fall back to `vec![ValueTy::Float; def_params.len()]` — keep that fallback for any def whose scheme doesn't yet carry types, so untouched defs (e.g. genuinely-unbound) don't change.
- C2 (Complete-mode `compile_anchor_closure`, `lower.rs:5567-5574`) and C1 (non-Complete anchor closure `:4948-4953`) should read the same data Task 3 now puts in the scheme. For single-param sig-less defs, `fn_param_tys` is rebuilt from the scheme in `infer.rs:1980-1998`; extend that builder to include `params().len() == 1` signature-less defs (not just `Expr::Lambda`-bodied curried ones) if the anchor consumers need it.
- **Do not delete behavior:** the `anchor_param_tys` producer inside `infer_def_body`'s retry (infer.rs:2249-2268) is superseded by Task 3's `lambda_param_tys`; the sig-typed producer (`:2196-2201`) was already redundant with `sig_params`. Deleting them must not change any test outcome.

- [ ] **Step 1: Delete the producers** in `infer_def_body` (both) and the field from `Ctx`; run `cargo test -p rill-lang --features dsp` and confirm the ONLY failures are compile errors from the remaining consumers/fields (expect them — we delete consumers next).
- [ ] **Step 2: Delete the consumers** C1/C2 and the `Lowerer` field + all six clone sites; rebuild consumers to read the scheme (notes above).
- [ ] **Step 3: Delete the `TypedProgram` field + Ctx constructor clones**; `cargo build -p rill-lang --features dsp` clean.
- [ ] **Step 4: Full gates** — `cargo test -p rill-lang --features dsp`, `cargo test -p rill-adrift`, clippy 0 warnings, fmt clean.
- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/src/lower.rs
git commit -m 'refactor(lang): drop the anchor_param_tys side-channel'
```

---

### Task 5: Bare refs, recursion, and the deferred-def call contract

**Files:**
- Modify: `rill-lang/src/lower.rs` (`lower_signature_less_apply_site` bare-ref handling; recursion guard)
- Modify: `rill-lang/src/types/infer.rs` (Phase-2 double-inference guard; M10 message)
- Test: `rill-lang/tests/accessor_first_class.rs`, `rill-lang/tests/sig_typed_params.rs`

**Interfaces:**
- Consumes: `constrained_lifting` guard pattern (lower.rs:4380-4398); M10 hook `retry_inferred_defs` (infer.rs:5950-5964) and its Phase-2 seeding (infer.rs:2415-2423).
- Produces: for a signature-less curried def whose λ-body carries class demands, using it as a bare VALUE errors with a message mirroring the constrained-def one (lower.rs:4553-4569: "cannot use class-constrained definition ... without applying it"); recursion through the specialization guard errors "recursive ... definition".

Implementation notes:
- **Bare ref (Review Focus 1):** in `lower_signature_less_apply_site`, detect the no-call-site case in the `Def`/`Ref` arms and emit the constrained-style error. The deferred def is only meaningful applied.
- **Recursion (Review Focus 4):** reuse a `SignatureLessLifting: HashSet<String>` guard symmetric to `constrained_lifting` so `at a i = if i == 0 then a ! i else at a (i - 1)` errors `recursive signature-less curried definition 'at'` instead of looping or double-specializing.
- **M10 message:** with Tasks 2-4, a correct-arg call now works; a wrong-arg call still hits `apply_by_signature`'s unify. Keep the "annotate the parameter's type" phrase and `at` name; drop nothing. Re-run the M10 test in `sig_typed_params.rs` — it stays green.

- [ ] **Step 1: RED tests** — `bare_ref_to_a_signature_less_accessor_is_a_compile_error` (`g = at;` or `map at arr`), `recursive_signature_less_accessor_is_a_compile_error`.
- [ ] **Step 2: Implement** bare-ref error + recursion guard + verify no double-inference in `infer_def_group` (the deferral must be decided once, in Phase 2, so the second pass doesn't re-defer).
- [ ] **Step 3: Full gates** — rill-lang, rill-adrift, clippy, fmt.
- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/lower.rs rill-lang/src/types/infer.rs rill-lang/tests/accessor_first_class.rs
git commit -m 'feat(lang): guard bare refs and recursion of deferred accessors'
```

---

### Task 6: Render round-trip safety and the bang_patterns note

**Files:**
- Modify: `rill-lang/src/render.rs` (only if a test exposes a leak — the design writes no synthesized signature to the AST, so no change is expected)
- Modify: `rill-lang/tests/bang_patterns.rs` (~217-265 comment) — update the "signature is what makes this..." comment to note that signature-less accessors now specialize per call site
- Test: `rill-lang/tests/accessor_first_class.rs`

**Interfaces:**
- Consumes: `render`/`parse`/`render` idempotence helpers (render.rs:706-723) or the suite's own round-trip.
- Produces: a pinned test that a signature-less accessor def renders byte-identically to its source form (its `sig` stays `None`), and that annotating it later still round-trips.

- [ ] **Step 1: Add the round-trip test** (`render_accessor_def_is_idempotent`) — render `at a i = a ! i; main = ...` through the typed-program render path; assert stable output and that no synthesized `::` appears.
- [ ] **Step 2: Update the bang_patterns comment** to the new mechanism (call-site specialization) — a doc-only edit; the test body is unchanged and must stay green including the force-count assertion.
- [ ] **Step 3: Commit**

```bash
git add rill-lang/tests/accessor_first_class.rs rill-lang/tests/bang_patterns.rs
git commit -m 'docs(lang): note per-call-site specialization of accessors; pin render round-trip'
```

---

### Task 7: Full verification and branch close-out

- [ ] **Step 1: Full gates** — `cargo test -p rill-lang --features dsp`; `cargo test -p rill-adrift`; `cargo clippy -p rill-lang --lib --tests --features dsp` (0 warnings); `cargo fmt -- --check` — all green.
- [ ] **Step 2: Review Focus sweep** — run each Review Focus case manually (a probe test, deleted after): bare ref, mixed param, 2-D index, recursion, render round-trip — confirm each behaves per the list above.
- [ ] **Step 3: Checkpoint the ledger** — append the completion + rulings to `rill/.superpowers/sdd/2026-10-19-rill-lang-arrays-closure/progress.md` (Task 2 already ruled: sites 4291/5898 differed from the plan; this plan's rulings go here).
- [ ] **Step 4: Leave the branch unmerged** (`feature/rill-lang-arrays-fp`), report the commit list and the before/after behavior table to the user; do NOT merge or open a PR until told.

---

## Out of Scope (explicitly)

- **PARK M9** — bare named fn into a function-shaped constrained param stays a clear diagnostic (`71225b5`); making named fns first-class function values is a separate concern from λ-parameter types.
- **PARK M12** — stale `benches/lang_bench.rs` / `--all-targets`; separate future task.
- **`anchor_param_tys`-adjacent `shared_cell_result_tys`, `fn_param_tys`, `runst_payload_tys`** — only `anchor_param_tys` is removed; the others stay unless a task explicitly says otherwise.
- **Synthesized signatures in the AST / user-visible `::`** — the plan never adds a signature the user did not write.
