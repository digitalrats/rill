# rill-lang: Value/Signal Unification via ST — Implementation Plan (Stage B)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify rill-lang's signal and value tracks into one type system where a signal buffer is a first-class mutable value (`Buffer`), modelled as an ST-style effect monad with a phantom type `s`, and Faust operators / `:` become typeclass methods.

**Architecture:** Introduce a `Buffer` typeclass (math = in-place mutation, implemented with scalar loops that LLVM auto-vectorizes), an `ST s a` effect monad with `runST (fn s -> body)` regions (one-point rank-2, check-escaping), and re-type signal processing as `Kleisli ST Buffer Buffer` arrows. `:` becomes a single overload (cons for `List`, seq for `Buffer`) dispatched through the existing `class_of_method` → `resolve_method` machinery. `Rate::Signal` as a distinct type-system rate is removed; the linear IR (`Instr`/`block_regs`/`run_block_mimo`) survives as the execution layer for `Buffer` operations. FFI (`foreign fn`, factories, `BlockBuiltin`/`Algorithm::process`) is unchanged. See `docs/superpowers/specs/2026-10-10-rill-lang-value-signal-unification-st-design.md`.

**Tech Stack:** Rust, crate `rill-lang` (+ `rill-core::memory::RegionHandle`), branch `feature/rill-lang-arrays`. No new external dependencies.

---

## Conventions

- Verify after every task: `cargo test -p rill-lang`; before finishing `cargo clippy --all-features --workspace` + `cargo fmt`.
- Pre-commit hook runs `cargo fmt` — run `cargo fmt` before every commit.
- Commit style: conventional commits, single-quoted `-m`.
- The signal test suite lives in `rill-lang/tests/` (ffi.rs, categories.rs, arrow.rs, graph_*). The examples are in `rill-adrift/examples/` (`lang_chiptune.rs`, `record_mic.rs`) — they are build targets, not test targets; verify they still compile with `cargo build -p rill-adrift --features "io,lofi,lang" --example lang_chiptune` where possible.

---

## Task B1: `Buffer` typeclass + in-place math instances

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`SIGNAL_PRELUDE` — extend `typeclass Buffer`)
- Modify: `rill-lang/src/types/infer.rs` (typeclass method resolution for the new methods; `value_builtin_ty`/math typing)
- Create: `rill-lang/src/buffer_math.rs` (scalar-loop implementations of `add`/`sub`/… over `[T; N]`)
- Modify: `rill-lang/src/lib.rs` (declare `pub mod buffer_math;`)
- Test: `rill-lang/tests/buffer_math.rs`

**Goal:** Define `Buffer` as a typeclass with in-place math methods, and implement them with scalar loops (LLVM auto-vectorizes). No runtime changes yet — the typeclass and instances exist, math is testable in isolation.

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/buffer_math.rs`:

```rust
use rill_core::math::Transcendental;
use rill_lang::buffer_math::*;

#[test]
fn scalar_add_is_inplace() {
    let a = [1.0f32, 2.0, 3.0, 4.0];
    let b = [10.0f32, 20.0, 30.0, 40.0];
    let mut out = a.clone();
    add_into(&mut out, &b);   // out += b, in place
    assert_eq!(out, [11.0, 22.0, 33.0, 44.0]);
    // a is untouched (out is a separate buffer)
    assert_eq!(a, [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn scalar_mul_assign() {
    let a = [1.0f32, 2.0, 3.0];
    let mut out = a.clone();
    mul_into(&mut out, 2.0f32);   // out *= 2
    assert_eq!(out, [2.0, 4.0, 6.0]);
}

#[test]
fn unary_sin_inplace() {
    let a = [0.0f32, core::math::f64::PI / 2.0];
    let mut out = a.clone();
    sin_into(&mut out);
    assert!((out[1] - 1.0).abs() < 1e-6, "sin(pi/2) == 1");
}
```

(Use `rill_core::math::Transcendental` for `sin`; check `rill_core::math` for the exact `f64::PI` path during implementation.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test buffer_math`
Expected: FAIL — `buffer_math` module not found.

- [ ] **Step 3: Implement scalar-loop math**

Create `rill-lang/src/buffer_math.rs`:

```rust
//! Buffer math implemented with simple scalar loops. LLVM auto-vectorizes
//! these over contiguous `[T; N]` (AGENTS.md: manual SIMD does not pay off).

use rill_core::math::Transcendental;

/// `out += rhs` element-wise, in place.
pub fn add_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o = *o + r;
    }
}

pub fn sub_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o = *o - r;
    }
}

pub fn mul_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o = *o * r;
    }
}

pub fn div_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o = *o / r;
    }
}

pub fn rem_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o = *o % r;
    }
}

pub fn neg_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = T::ZERO - *o;
    }
}

pub fn sin_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.sin();
    }
}

pub fn cos_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.cos();
    }
}

pub fn tan_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.tan();
    }
}

pub fn sqrt_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.sqrt();
    }
}

pub fn exp_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.exp();
    }
}

pub fn ln_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.ln();
    }
}

pub fn tanh_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.tanh();
    }
}

pub fn abs_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.abs();
    }
}

pub fn min_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o = o.min(r);
    }
}

pub fn max_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o = o.max(r);
    }
}

pub fn fill_into<T: Transcendental>(out: &mut [T], v: T) {
    for o in out.iter_mut() {
        *o = v;
    }
}
```

Check the exact `Transcendental` method names against `rill_core/src/math/mod.rs` (sin/cos/tan/sqrt/exp/ln/tanh/abs/min/max) and adjust the implementations to the real trait.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang --test buffer_math`
Expected: PASS.

- [ ] **Step 5: Extend the `Buffer` typeclass prelude**

In `rill-lang/src/types/ty.rs`, replace the marker `typeclass Buffer b where { }` with:

```rill
typeclass Buffer b where {
    add:  b -> b -> ST ();
    sub:  b -> b -> ST ();
    mul:  b -> b -> ST ();
    div:  b -> b -> ST ();
    rem:  b -> b -> ST ();
    neg:  b -> ST ();
    sin:  b -> ST ();
    cos:  b -> ST ();
    tan:  b -> ST ();
    sqrt: b -> ST ();
    exp:  b -> ST ();
    ln:   b -> ST ();
    tanh: b -> ST ();
    abs:  b -> ST ();
    min:  b -> b -> ST ();
    max:  b -> b -> ST ();
    fill: Scalar -> b -> ST ();
}
```

Note: `ST` is introduced in Task B2; the prelude does not typecheck until then. To keep this task independently green, first land the prelude **without** method signatures (keep `typeclass Buffer b where { }`), and add the method signatures in Task B2 when `ST` exists. Document this ordering in a code comment.

- [ ] **Step 6: Run the full suite to confirm no regression**

Run: `cargo test -p rill-lang`
Expected: PASS (marker `Buffer` still empty; math module compiles).

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add rill-lang/src/buffer_math.rs rill-lang/src/lib.rs rill-lang/tests/buffer_math.rs
git commit -m 'feat(rill-lang): Buffer math scalar-loop implementations (in-place)'
```

---

## Task B2: `ST s a` + `runST` — full rank-2 polymorphism

**Files:**
- Modify: `rill-lang/src/ast.rs` (`Expr::RunST` variant)
- Modify: `rill-lang/src/lexer.rs` (keyword `runST`), `rill-lang/src/parser.rs` (`runST (fn s -> body)`)
- Modify: `rill-lang/src/reduce.rs` (substitute through `RunST`)
- Modify: `rill-lang/src/types/ty.rs` (prelude: `data ST s a = { unST: a }`)
- Modify: `rill-lang/src/types/infer.rs` (**extend `free_vars`/`instantiate` for value-tyvars; quantify `runST`'s phantom param into `Scheme.vars`**)
- Modify: `rill-lang/src/lower.rs` (lower `RunST`; erase the phantom `s`)
- Test: `rill-lang/tests/st_monad.rs`, `rill-lang/tests/rank2_poly.rs`

**Goal:** Full rank-2 polymorphism so `runST : (forall s. ST s a) -> a`. The phantom `s` is a *quantified* type variable in the argument's `Scheme.vars`; because it is quantified it cannot appear free in the result type `a` — check-escaping is automatic, with no post-hoc scan.

> **Design history:** an earlier one-point `runST` check-escaping scan was implemented (commit `5b2c84d`) and code review found two bypasses (phantom chained to a lower-id var via cross-kind unification; phantom not shadowing an outer value binder named `s`). The one-point approach is fragile. This task reworks it to full rank-2. **Revert `5b2c84d`'s inference approach** (keep the syntax/prelude/lowering; replace the ad-hoc `type_var_bindings` + `vty_mentions_tyvar` escape check with real generalization).

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/st_monad.rs`:

```rust
use rill_core::traits::MultichannelAlgorithm;
use rill_lang::builtin::Registry;

fn run(src: &str) -> rill_lang::program_engine::ProgramEngine<f32, 256> {
    let reg = Registry::<f32>::new();
    rill_lang::compile_graph::<f32, 256>(src, &reg, 44100.0).unwrap()
}

#[test]
fn runst_region_erases_phantom() {
    // `runST (fn s -> e)` executes `e` and erases the phantom type `s`.
    let mut p = run("{-# MODE complete #-}\nmain = runST (fn s -> 1.0);");
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut p, &[], &mut [&mut out]);
    assert_eq!(out[0], 1.0);
}

#[test]
fn runst_region_rejects_escaping_phantom() {
    // The phantom s is quantified, so a result mentioning s must NOT escape.
    let src = "{-# MODE complete #-}\nmain = runST (fn s -> (s));";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let r = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Complete);
    assert!(r.is_err(), "ST result must not escape the runST region");
}
```

Create `rill-lang/tests/rank2_poly.rs`:

```rust
// Regression: the two one-point-approach bypasses must also be rejected.
#[test]
fn phantom_chain_through_higher_id_var_rejected() {
    // The list unifies t and s; s is quantified so it must not unify with an
    // outer free variable. Compile must fail.
    let src = "{-# MODE complete #-}\nmain = (fn t -> runST (fn s -> [t, s])) 1.0;";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let r = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Complete);
    assert!(r.is_err(), "phantom s must not unify with an outer variable");
}

#[test]
fn phantom_shadows_outer_value_binder() {
    // An outer value named `s` must not disable the escape check.
    let src = "{-# MODE complete #-}\ns = 2.0;\nmain = runST (fn s -> (s));";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let r = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Complete);
    assert!(r.is_err(), "phantom s must shadow an outer value named s");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rill-lang --test st_monad --test rank2_poly`
Expected: FAIL — `runST` unknown, or the escape tests pass incorrectly (the point: rank-2 semantics must hold).

- [ ] **Step 3: Add `ST` data type to the prelude**

In `rill-lang/src/types/ty.rs` (CATEGORY_PRELUDE), add:

```rill
data ST s a = { unST: a };
```

(Register it the same way `Kleisli m a b` is registered — `data_types` + `data_arities` in `ty.rs`.)

- [ ] **Step 4: Parse `runST (fn s -> body)`**

Add keyword `runST` to `lexer.rs`, and in `parser.rs` add a parse arm that reads `runST` followed by a parenthesized lambda, producing a new `Expr::RunST { body: Expr::Lambda, span }` (add the variant to `ast.rs` `Expr` enum). Wire `reduce.rs` (substitute through the body) and `render.rs` (round-trip).

- [ ] **Step 5: Extend generalization to value-type variables**

In `infer.rs`:
1. **`free_vars`** (currently only collects `Scalar::Var` from channel `elem`): also collect free `ValueTy::Var(v)` / `TyConVar(v)` / `TyConApp(v, ..)` heads from each channel's `vty`. Add a helper `value_free_vars(&ValueTy, &Subst) -> Vec<TypeVarId>` (mirror `vty_mentions_tyvar`'s traversal but collect free vars, resolved through `subst.value_map`).
2. **`instantiate`**: when substituting `Scheme.vars`, also map value-tyvars: for `Rate::Value` channels, substitute `Var(v)`/`TyConVar(v)`/`TyConApp(v, args)` occurrences of quantified vars with fresh vars (mirror the `rw` closure but over `ValueTy`). Reuse the existing fresh-var allocation.

Verify: a polymorphic value function like `id2 = fn x -> x;` now has its `Var` in `Scheme.vars` and instantiates fresh at each use.

- [ ] **Step 6: Quantify `runST`'s phantom param (rank-2)**

In `infer.rs`, handle `Expr::RunST`:
1. The argument is a lambda `fn s -> body`. Allocate a fresh `TypeVarId` `s`.
2. Infer the body with `s` bound **in scope** (as a `ValueTy::TyConVar` visible to `Ref("s")` AND shadowing any outer value binder — bind in `ctx.locals` like the `Expr::Lambda` arm, save/restore the map).
3. Generalize: `s` goes into the argument's `Scheme.vars` (quantified). The `runST` application then requires the argument to be polymorphic in `s`; the result type `a` is the body's result with `s` NOT free (if `s` appears free in the result, the scheme's generalization cannot capture it and unification fails — the escape is rejected automatically).
4. `runST` is typed as `(forall s. ST s a) -> a`: the application takes the polymorphic lambda, instantiates the phantom with a fresh var for internal use, and returns the non-phantom result.

- [ ] **Step 7: Remove the one-point escape scan**

Delete the ad-hoc `type_var_bindings` fallback in `infer_ref_inner` and the `vty_mentions_tyvar`-based escape check added in `5b2c84d` — rank-2 generalization replaces them. Keep the `runST` syntax, prelude `ST`, parsing, reduce, render, and the no-op lowering.

- [ ] **Step 8: Lower `runST` (erase phantom)**

In `lower.rs`, handle `Expr::RunST`: lower the lambda body normally; the phantom `s` has no runtime representation, so `runST` is a no-op wrapper (a closure over the body).

- [ ] **Step 9: Run the tests**

Run: `cargo test -p rill-lang --test st_monad --test rank2_poly`
Expected: PASS — both runST tests AND both bypass regressions are rejected.

- [ ] **Step 10: Full suite + commit**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`, `cargo clippy -p rill-lang`, `cargo fmt`.
Commit:

```bash
cargo fmt
git add rill-lang/src/ast.rs rill-lang/src/lexer.rs rill-lang/src/parser.rs rill-lang/src/reduce.rs rill-lang/src/types/infer.rs rill-lang/src/types/ty.rs rill-lang/src/lower.rs rill-lang/src/render.rs rill-lang/tests/st_monad.rs rill-lang/tests/rank2_poly.rs
git commit -m 'feat(rill-lang): full rank-2 polymorphism — runST (forall s. ST s a) -> a
Value-type variables generalize into Scheme.vars; instantiate substitutes them;
runST quantifies its phantom param so escaping the region is rejected
automatically (replaces the fragile one-point check-escaping scan).'
```

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/st_monad.rs`:

```rust
use rill_core::traits::MultichannelAlgorithm;

fn run(src: &str) -> rill_lang::program_engine::ProgramEngine<f32, 256> {
    let reg = rill_lang::ffi::ForeignRegistry::<f32>::default();
    rill_lang::compile_graph::<f32, 256>(src, &reg, 44100.0).unwrap()
}

#[test]
fn runst_region_erases_phantom() {
    // `runST (fn s -> e)` executes `e` and erases the phantom type `s`;
    // the value flowing out is `e`'s result. s is only a type-level token.
    let mut p = run("{-# MODE complete #-}\nmain = runST (fn s -> 1.0);");
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut p, &[], &mut [&mut out]);
    assert_eq!(out[0], 1.0);
}

#[test]
fn runst_region_rejects_escaping_phantom() {
    // A result whose type mentions `s` (e.g. an `ST s Float`) must NOT escape
    // the region: check-escaping rejects it at compile time.
    let src = "{-# MODE complete #-}\nmain = runST (fn s -> (s));";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let r = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Complete);
    assert!(r.is_err(), "ST result must not escape the runST region");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test st_monad`
Expected: FAIL — `runST` unknown.

- [ ] **Step 3: Add `ST` data type to the prelude**

In `rill-lang/src/types/ty.rs` (CATEGORY_PRELUDE), add:

```rill
data ST s a = { unST: a };
```

(Register it the same way `Kleisli m a b` is registered — `data_types` + `data_arities` in `ty.rs`.)

- [ ] **Step 4: Parse `runST (fn s -> body)`**

Add keyword `runST` to `lexer.rs`, and in `parser.rs` add a parse arm that reads `runST` followed by a parenthesized lambda, producing a new `Expr::RunST { body: Expr::Lambda, span }` (add the variant to `ast.rs` `Expr` enum).

- [ ] **Step 5: Infer `runST` with phantom binding + check-escaping**

In `infer.rs`, handle `Expr::RunST`:
1. Allocate a fresh `TypeVarId` `s`; bind it as a `TyConVar` in a new scope.
2. Infer the lambda body with `s` visible.
3. **Check-escaping:** after inference, scan the result type for any free occurrence of `s`. If found → `CompileError::Type { msg: "ST region result escapes the phantom type s", span }`.
4. Return the body's result type with `s` erased (its `TyConVar` binding dropped).

- [ ] **Step 6: Lower `runST` (erase phantom)**

In `lower.rs`, handle `Expr::RunST`: lower the lambda body normally; the phantom `s` has no runtime representation, so `runST` is a no-op wrapper (a closure over the body). For value-track programs this is a plain closure; for buffer programs (Task B3+) it bounds the ST region.

- [ ] **Step 7: Wire reduce + render**

In `reduce.rs`, substitute through `Expr::RunST { body, .. }` (recurse into `body`). In `render.rs`, round-trip `runST`.

- [ ] **Step 8: Run the tests**

Run: `cargo test -p rill-lang`
Expected: PASS (new ST test + no regression).

- [ ] **Step 9: Commit**

```bash
cargo fmt
git add rill-lang/src/ast.rs rill-lang/src/lexer.rs rill-lang/src/parser.rs rill-lang/src/reduce.rs rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/src/types/ty.rs rill-lang/tests/st_monad.rs
git commit -m 'feat(rill-lang): ST s a effect monad + runST regions with phantom check-escaping'
```

---

## Task B3: `:` single overload (cons for `List`, seq for `Buffer`)

**Files:**
- Modify: `rill-lang/src/parser.rs` (keep `:` → `Expr::Seq`, no change here yet)
- Modify: `rill-lang/src/desugar.rs` (rewrite `Expr::Seq` in value position to a dispatched call)
- Modify: `rill-lang/src/types/infer.rs` (`infer_seq` — dispatch by operand types instead of `reject_value_channels`)
- Modify: `rill-lang/src/types/ty.rs` (prelude: `:` methods — cons instance for `List`, seq instance for `Buffer`)
- Modify: `rill-lang/src/lower.rs` (lower the dispatched call)
- Test: `rill-lang/tests/colon_overload.rs`

**Goal:** `x : xs` on a `List` is cons; `a : b` on buffers is seq. One spelling, type-directed dispatch.

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/colon_overload.rs`:

```rust
use rill_core::traits::MultichannelAlgorithm;

fn eval_main(src: &str) -> f32 {
    let reg = rill_lang::ffi::ForeignRegistry::<f32>::default();
    let mut p = rill_lang::compile_graph::<f32, 256>(src, &reg, 44100.0).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut p, &[], &mut [&mut out]);
    out[0]
}

#[test]
fn colon_on_list_is_cons() {
    // x : [y, z] == [x, y, z]; head of the result is x.
    // (complete mode: cons is LazyCons; head/tail work on Cons)
    assert_eq!(eval_main("{-# MODE complete #-}\nmain = match (head (1.0 : [2.0, 3.0])) of { Nothing => 0.0; Just x => x };"), 1.0);
}

#[test]
fn colon_on_signal_is_seq() {
    // a : b on signals is Faust sequencing — regression that it still works.
    // (uses the wire `_` as input, `onepole` FFI algorithm as the second stage)
    let _ = rill_lang::compile_graph::<f32, 256>("main = _ : onepole 200.0 0.7", &rill_lang::ffi::ForeignRegistry::<f32>::default(), 44100.0).unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test colon_overload`
Expected: FAIL — `:` on a value (`1.0 : [2.0, 3.0]`) is rejected by `infer_seq` (`reject_value_channels`).

- [ ] **Step 3: Replace `infer_seq`'s rejection with type-directed dispatch**

In `infer.rs` `infer_seq`:
1. Infer both operands.
2. If the RIGHT operand's value type is `List a` (or the pair unifies to a list-cons shape) → treat as cons: `x : xs` types as `List a` with element `x` unified to `a`.
3. Else (signal/buffer context) → keep the Faust seq behavior (currently `reject_value_channels`; keep for now, re-target to `Buffer` in Task B5).
4. If neither applies → ambiguous-`:` error.

Dispatch goes through the typeclass machinery: define a class `Colon a` (or reuse `Seq`) with instances for `List` and `Buffer`, and resolve via `class_of_method` → `resolve_method` (see the `Arrow (Kleisli m)` pattern).

- [ ] **Step 4: Add `:` methods to the prelude**

```rill
typeclass Colon a where {
    seq: a -> a -> a;    -- one method, two instances
}
instance Colon (List a) where {
    seq x xs = cons x xs;   -- cons (element : list)
}
-- Buffer instance added in Task B5
```

- [ ] **Step 5: Lower the dispatched call**

In `lower.rs`, lower the desugared `seq` call through the existing method-dispatch path (like `Arrow (Kleisli m)`).

- [ ] **Step 6: Run the tests**

Run: `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add rill-lang/src/desugar.rs rill-lang/src/types/infer.rs rill-lang/src/types/ty.rs rill-lang/src/lower.rs rill-lang/tests/colon_overload.rs
git commit -m 'feat(rill-lang): single ":" overload — cons for List, dispatched via typeclass'
```

---

## Task B4: Remove `Rate::Signal` as a distinct type-system rate

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`Rate` enum; `Channel::signal`; `Block` alias; `Scalar`)
- Modify: `rill-lang/src/types/infer.rs` (6 `Rate::Signal` arms)
- Modify: `rill-lang/src/lower.rs` (`has_value_out`, arity, `Block::new`)
- Modify: `rill-lang/src/ir.rs` (`Instr` keeps buffer ops; no type-level change)
- Test: `rill-lang/tests/rate_unification.rs`

**Goal:** Signals are typed as `Rate::Value` with `ValueTy` = `Buffer`; `Rate::Signal` is gone from the type system. The runtime (`Instr`/`block_regs`) is unchanged — it is the execution layer for `Buffer` values.

**Audit note:** only ~12 `Rate::Signal` uses exist in `infer.rs` (`:128,300,627,1552,3593,4223`). This task is mechanical: replace each `Rate::Signal` arm with a `Rate::Value` + `Buffer` typing, and collapse the `Channel` rate field.

- [ ] **Step 1: Audit the exact `Rate::Signal` sites**

Run: `grep -n "Rate::Signal" rill-lang/src/types/infer.rs rill-lang/src/types/ty.rs rill-lang/src/lower.rs`
Document each site's role in a comment in the plan/issue; expect `infer.rs:128,300,627,1552,3593,4223` and `ty.rs` (enum + `Channel::signal`).

- [ ] **Step 2: Write the failing test**

Create `rill-lang/tests/rate_unification.rs`:

```rust
// A signal program now types its channels as Rate::Value with Buffer vty.
#[test]
fn signal_channel_is_buffer_value() {
    let src = "main = _ : onepole 200.0 0.7";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let tp = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Strict).unwrap();
    // Every output channel is Rate::Value with vty == Buffer (not Rate::Signal).
    for c in &tp.process_ty.outs {
        assert_eq!(c.rate, rill_lang::types::ty::Rate::Value);
    }
}
```

- [ ] **Step 3: Collapse `Rate`**

In `ty.rs`, either (a) keep `Rate` enum but make all channels `Value`, or (b) remove `rate` and keep only `vty`. Prefer (b) if the churn is contained; otherwise (a). Update `Channel::signal` → `Channel::value(Buffer)`; delete the `Scalar`/`elem` machinery if it becomes dead.

- [ ] **Step 4: Rewrite the 6 `infer.rs` arms**

Each `Rate::Signal` arm becomes a `Rate::Value` arm with `ValueTy::App("Buffer", [scalar])` (or a dedicated `ValueTy::Buffer` if cleaner). Update `check_all_numeric`/arity logic to treat a buffer channel as a value channel whose `vty` is `Buffer`.

- [ ] **Step 5: Update `lower.rs` arity/`has_value_out`**

`has_value_out` (lower.rs:5581) and `main_args` must treat `Buffer`-typed value channels as signal channels for the linear IR (inputs → `LoadInput` → `block_regs`). This is the seam where "value in types, buffer in execution" meets.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p rill-lang`
Expected: PASS (all existing signal tests + the new one).

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add rill-lang/src/types/ty.rs rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/tests/rate_unification.rs
git commit -m 'refactor(rill-lang): unify Rate — signals are Buffer-typed value channels'
```

---

## Task B5: Buffer math → `Buffer` methods; Faust ops → Arrow/Buffer methods

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (prelude: `instance Buffer (FixedBuffer a)` with math bodies)
- Modify: `rill-lang/src/types/infer.rs` (type the math methods)
- Modify: `rill-lang/src/lower.rs` (lower math method calls to `buffer_math` calls; `Un`/`Bin`/`Const` become method dispatches)
- Modify: `rill-lang/src/backend/interp.rs` (`exec_block_op` `Un`/`Bin`/`Const` → call the instance's scalar-loop functions; keep SIMD helpers as an option)
- Modify: `rill-lang/src/desugar.rs` (`Seq`/`Par`/`Split`/`Merge`/`Loop`/`Delay` in signal position → Arrow/Buffer methods)
- Test: `rill-lang/tests/buffer_methods.rs`

**Goal:** `Un`/`Bin`/`Const` execute through `Buffer` methods (scalar loops); Faust combinators map to `Arrow`/`Buffer` methods. FFI `CallBlock` unchanged.

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/buffer_methods.rs`:

```rust
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn binop_lowers_through_buffer_method() {
    // `x + 1.0` on a signal lowers via Buffer.add (in-place on the buffer).
    let src = "main = _ + 1.0";
    let mut p = rill_lang::compile_graph::<f32, 256>(src, &rill_lang::ffi::ForeignRegistry::<f32>::default(), 44100.0).unwrap();
    let inputs: [&[f32]; 1] = [&[2.0f32, 4.0]];
    let mut out = [0.0f32; 2];
    let _ = MultichannelAlgorithm::process(&mut p, inputs, &mut [&mut out]);
    assert_eq!(out, [3.0, 5.0]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test buffer_methods`
Expected: FAIL (or passes if `Bin` still works — then make the test assert the dispatch path by checking the lowered IR, or rely on the Buffer instance bodies being exercised).

- [ ] **Step 3: Add `instance Buffer (FixedBuffer a)` math bodies**

In the prelude:

```rill
instance Buffer (FixedBuffer a) where {
    add x y = buffer_add x y;    -- builtin names wired to buffer_math::add_into
    sub x y = buffer_sub x y;
    -- ... one per method ...
    fill v x = buffer_fill v x;
}
```

Register `buffer_add`/`buffer_sub`/… as builtin names in `value_builtin`/`ffi`-style dispatch (add them to the same machinery that maps `+` → `BinArith::Add`).

- [ ] **Step 4: Route `Un`/`Bin`/`Const` lowering through the methods**

In `lower.rs`, when a math op's operand is a `Buffer`-typed value, lower it as a `Buffer` method call (which lowers to a `CallBlock`-like instance or a new buffer-op instruction that invokes `buffer_math::add_into` etc.). Keep the existing `Bin`/`Un` path for the strict scalar case if any remains.

- [ ] **Step 5: Map Faust ops to methods in desugar**

In `desugar.rs`, in signal position, rewrite:
- `Seq` → `seq` method (Buffer instance; the `:` overload from Task B3)
- `Par` → `both`/`par`, `Split` → `fan`/`split`, `Merge` → `merge`, `Loop` → feedback method, `Delay` → delay method

Reuse the `Arrow (Kleisli m)` instance methods (`compose`/`both`/`fan`/`first`/`second`) where they apply; add `Buffer`-specific instances where not.

- [ ] **Step 6: Execute via `buffer_math`**

In `interp.rs`, the math method dispatch calls the `buffer_math::*_into` scalar loops on the buffer's `[T; N]` slice. SIMD helpers (`rill_core::math::vector`) remain available as an option but are not required.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p rill-lang`
Expected: PASS (math via methods; existing signal tests keep working; `lang_chiptune`/`record_mic` still compile).

- [ ] **Step 8: Commit**

```bash
cargo fmt
git add rill-lang/src/types/ty.rs rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/src/backend/interp.rs rill-lang/src/desugar.rs rill-lang/tests/buffer_methods.rs
git commit -m 'feat(rill-lang): Buffer math as typeclass methods; Faust ops as Arrow/Buffer methods'
```

---

## Task B6: RegionHandle as a `Buffer` value

**Files:**
- Modify: `rill-lang/src/arena.rs` (`Value::Buffer` variant holding a handle/ref)
- Modify: `rill-lang/src/types/ty.rs` (prelude: `instance Buffer (RegionHandle a)`)
- Modify: `rill-lang/src/lower.rs` (alloc RegionHandle buffers for big buffers)
- Modify: `rill-lang/src/backend/interp.rs` (dispatch math/FFI over `Value::Buffer`)
- Modify: `rill-lang/src/program.rs` (`BufferAllocator` wiring for `Buffer` values)
- Test: `rill-lang/tests/buffer_region.rs`

**Goal:** `RegionHandle` (RC big-object handle, SP-4) becomes a `Value::Buffer` so large buffers are first-class values. `FixedBuffer` (inline) and `RegionHandle` (slab) are two `Buffer` instances.

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/buffer_region.rs`:

```rust
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn region_buffer_is_first_class_value() {
    // A large buffer value can be stored in a List and passed around.
    // `buffer_alloc n` allocates a RegionHandle-backed buffer of `n` samples
    // (a new Buffer builtin, added alongside `instance Buffer (RegionHandle a)`
    // in this task).
    let src = "{-# MODE complete #-}\nmain = match (head [buffer_alloc 1024.0, buffer_alloc 2048.0]) of { Nothing => 0.0; Just x => 1.0 };";
    let mut p = rill_lang::compile_graph::<f32, 256>(src, &rill_lang::ffi::ForeignRegistry::<f32>::default(), 44100.0).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut p, &[], &mut [&mut out]);
    assert_eq!(out[0], 1.0);
}

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test buffer_region`
Expected: FAIL — `Value::Buffer`/`buffer_alloc` unknown.

- [ ] **Step 3: Add `Value::Buffer` to the arena**

In `arena.rs`, add `Value::Buffer(usize)` (a slot index into a program-level buffer table backed by `RegionHandle`s) or `Value::Buffer(RegionHandle)` directly. Update `visit_refs`/`clone_pooled`/`detach_payload`/`value_size` for the new variant (mirror how `Thunk`/`Cons` were added in Stage A).

- [ ] **Step 4: Register `instance Buffer (RegionHandle a)`**

In the prelude, add the `RegionHandle` instance with the same math bodies (they operate on the handle's slice). Wire the `BufferAllocator` from `program.rs` so `Buffer`-value allocation carves from the slab.

- [ ] **Step 5: Dispatch math/FFI over `Value::Buffer`**

In `interp.rs`, the `Buffer` method dispatch and `exec_foreign_block` accept `Value::Buffer` handles, reading/writing through `RegionHandle::with_data`/`with_data_mut`. The `FixedBuffer`-instance path stays for inline buffers.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add rill-lang/src/arena.rs rill-lang/src/types/ty.rs rill-lang/src/lower.rs rill-lang/src/backend/interp.rs rill-lang/src/program.rs rill-lang/tests/buffer_region.rs
git commit -m 'feat(rill-lang): RegionHandle-backed Buffer values (big objects)'
```

---

## Task B7: FFI as Buffer arrows; integration + verification

**Files:**
- Modify: `rill-lang/src/lower.rs` (foreign calls typed as `Buffer` arrows)
- Modify: `rill-lang/src/types/infer.rs` (`foreign fn` typing returns `Buffer`-typed channels)
- Modify: `rill-lang/src/backend/interp.rs` (`exec_foreign_block` accepts `Value::Buffer` I/O)
- Test: `rill-lang/tests/ffi_arrows.rs`
- Verify: examples compile (`rill-adrift/examples/lang_chiptune.rs`, `record_mic.rs`)

**Goal:** Foreign algorithms (`sine`, `biquad`, `ay38910`, `lofi`, `onepole`, …) type as `Buffer` arrows; `Algorithm::process` applies to `Buffer` values. End-to-end integration.

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/ffi_arrows.rs`:

```rust
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn foreign_algo_is_buffer_arrow() {
    // onepole: FixedBuffer f32 -> ... -> FixedBuffer f32 — typed as a Buffer arrow.
    let src = "main = _ : onepole 200.0 0.7";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let tp = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Strict).unwrap();
    for c in &tp.process_ty.outs {
        assert_eq!(c.vty, rill_lang::types::ty::ValueTy::App("Buffer".into(), vec![]));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test ffi_arrows`
Expected: FAIL — foreign output still `FixedBuffer`-as-signal, not `Buffer`-typed.

- [ ] **Step 3: Type foreign calls as `Buffer` arrows**

In `infer.rs`, the `foreign fn` typing path (see `infer_ref`/the `ffi_sig_from_typeexpr` path) maps `FixedBuffer a` signal channels to `Rate::Value` channels with `vty = Buffer`. The call is an arrow `Buffer -> ... -> Buffer`; combine with the `:` seq from Task B3.

- [ ] **Step 4: Lower foreign calls as arrows**

In `lower.rs`, the foreign call lowers as a `Buffer` arrow application: inputs are `Buffer` values, the `Algorithm::process` mutates the output `Buffer`. `Instr::CallBlock`/`BuiltinInst`/`exec_foreign_block` keep working, now over `Value::Buffer` I/O (Task B6).

- [ ] **Step 5: Verify examples compile and run**

Run:
```bash
cargo build -p rill-adrift --features "io,lofi,lang" --example lang_chiptune
cargo build -p rill-adrift --features "io,lang,sampler" --example record_mic
```
Expected: both compile (running requires audio hardware; compilation is the gate).

- [ ] **Step 6: Full verification**

Run: `cargo test --workspace`, `cargo clippy --all-features --workspace`, `cargo fmt`
Expected: all green, 0 warnings.

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add rill-lang/src/lower.rs rill-lang/src/types/infer.rs rill-lang/src/backend/interp.rs rill-lang/tests/ffi_arrows.rs
git commit -m 'feat(rill-lang): FFI algorithms as Buffer arrows; end-to-end unification'
```

---

## Self-review checklist

- [ ] §2.1 Approach A (`Kleisli ST Buffer Buffer`) → Tasks B2 (ST), B5 (arrows), B7 (FFI arrows).
- [ ] §2.2 `:` single overload (List→cons, Buffer→seq) → Task B3.
- [ ] §2.3 math is in-place mutation, scalar loops → Tasks B1, B5.
- [ ] §2.4 ST with phantom `s`, runST, check-escaping → Task B2.
- [ ] §2.5 Faust operators as Arrow/Buffer methods → Task B5.
- [ ] §2.5b FFI integration (signature/factory/`BlockBuiltin` unchanged) → Task B7.
- [ ] §2.6 IO deferred; `Rate::Signal` removed; linear IR survives as execution layer → Task B4.
- [ ] §3 RegionHandle as a `Buffer` value → Task B6.
- [ ] §5 backward compat (existing programs, RT safety, SIMD preserved) → Tasks B4, B5, B7 verification.
- [ ] §6 testing (math, ST, `:`, Faust-as-Arrow, Buffer-as-value, FFI arrows) → each task's test file.