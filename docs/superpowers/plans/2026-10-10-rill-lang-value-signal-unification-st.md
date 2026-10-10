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

## Task B2: `ST s a` + `runST` — full rank-2 polymorphism — DONE

> **Status: COMPLETE** via the rank-N sub-plan (`docs/superpowers/plans/2026-10-10-rill-lang-rank2-thunk.md`, Tasks R1-R3).
> `runST` is now syntax sugar over a forall-typed built-in function: the parser
> desugars `runST (fn s -> body)` → `Apply runST [lam]`; `runST` has signature
> `(forall s. ST s a) -> a` in `TypeEnv::builtin_sigs`; rank-N application in
> `infer_apply_impl` rejects escaping automatically. Commits: R1 `049eec7`+`d84239d`,
> R2 `5530fd9`, R3 `b487cc0`. Tests: `st_monad.rs`, `rank2_poly.rs` (inference-level;
> the runtime `runST (fn s -> 1.0)` assertion is deferred to Task B3, where the
> track unification lands).

---
## Task B3: Full track unification — block track eliminated, everything is a closure

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`Rate` enum collapsed; `ValueTy::Buffer`; `Channel`; `Scalar`/`elem` fate)
- Modify: `rill-lang/src/types/infer.rs` (`Rate::Signal` arms → `Rate::Value` + `Buffer`; FFI typing)
- Modify: `rill-lang/src/ir.rs` (new `ValueCallForeign` instruction; remove `Instr` block variants)
- Modify: `rill-lang/src/lower.rs` (signal expressions lower to the value track with `Buffer` values; FFI calls → `ValueCallForeign` fragment)
- Modify: `rill-lang/src/backend/interp.rs` (remove `run_block_mimo`/`exec_block_op`/`exec_foreign_block`; add `ValueCallForeign` arm to `exec_value_instr`; merge `run_value_track` into `run_call`)
- Modify: `rill-lang/src/program.rs` (remove `block_regs`), `rill-lang/src/schedule.rs` (remove), `rill-lang/src/arena.rs` (`Value::Buffer`)
- Test: `rill-lang/tests/rate_unification.rs`

**Goal:** Signals are typed as `Rate::Value` with `ValueTy::Buffer`, AND the block track is eliminated entirely. Everything is a closure: program = root fragment (base=0), function = `Value::Callable` → `run_call`, FFI algorithm = a fragment whose single instruction is `ValueCallForeign { builtin }` → `Algorithm::process` over `Value::Buffer`, math = `Buffer` methods, state = `value_state`. `Instr` block variants, `block_regs`, `run_block_mimo`, `build_schedule`, `exec_block_op`, `exec_foreign_block`, and `schedule.rs` are DELETED.

> **Design:** in classical Haskell everything is a closure. `run_value_track` and
> `run_call` already share `exec_value_instr` (base param, post-T2); the only
> difference is base (0 vs fragment offset) and env-binding/args-source. B3
> merges them and represents FFI as a fragment with `ValueCallForeign`, keeping
> ONE call path.

- [ ] **Step 1: Audit the exact `Rate::Signal` sites and block-track surface**

Run:
```bash
grep -n "Rate::Signal" rill-lang/src/types/infer.rs rill-lang/src/types/ty.rs
grep -n "block_regs\|run_block_mimo\|build_schedule\|exec_block_op\|exec_foreign_block" rill-lang/src/backend/interp.rs rill-lang/src/program.rs rill-lang/src/schedule.rs rill-lang/src/ir.rs
```
Document each site's role. Expected: 8 `Rate::Signal` in infer, 4 in ty; block-track in interp/program/schedule/ir.

- [ ] **Step 2: Write the failing test**

Create `rill-lang/tests/rate_unification.rs`:

```rust
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

- [ ] **Step 3: Collapse `Rate` + add `ValueTy::Buffer`**

In `ty.rs`: add `ValueTy::Buffer` (or `App("Buffer", vec![])`); collapse `Rate` so no channel is `Signal` (either remove the enum or make all `Value`); `Channel::signal` → a `Channel::value(Buffer)` path; decide the `Scalar`/`elem` fate (keep if tied to Buffer's param, delete if dead).

- [ ] **Step 4: Rewrite the `infer.rs` arms + FFI typing**

Each `Rate::Signal` arm → `Rate::Value` with `vty = Buffer`. FFI typing (`FixedBuffer a` in foreign sigs) maps to `Rate::Value` with `vty = Buffer`. `check_all_numeric`/arity treat a buffer channel as a value channel.

- [ ] **Step 5: Add `ValueCallForeign` instruction + `Value::Buffer`**

In `ir.rs`: add `ValueInstr::ValueCallForeign { dst, srcs, instance }` (or reuse the args shape). In `arena.rs`: add `Value::Buffer(usize)` (a slot index into a program-level buffer table) or `Value::Buffer(RegionHandle)` — a first-class buffer value.

- [ ] **Step 6: Lower signal expressions to the value track**

In `lower.rs`: signal expressions (`main = _ : onepole 200.0 0.7`) lower through the value track with `Buffer` values. FFI calls lower to a `ValueCallForeign` instruction (a fragment whose single instruction dispatches `Algorithm::process` over `Buffer` values). `has_value_out`/`main_args` route `Buffer` channels through the value path (inputs → value-track input reads → `Buffer`).

- [ ] **Step 7: Merge the runners; delete the block track**

In `interp.rs`:
- `exec_value_instr` gains a `ValueCallForeign` arm (dispatches `Algorithm::process` over `Value::Buffer`).
- `run_value_track` merges into `run_call` (program = root fragment, base=0, empty env). 
- Delete `run_block_mimo`, `exec_block_op`, `exec_foreign_block`.
In `program.rs`: remove `block_regs`. Delete `schedule.rs`. Remove `Instr` block variants from `ir.rs` (and `regalloc.rs` if it only handled block IR).

- [ ] **Step 8: Run the tests**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`
Expected: PASS (all existing signal + value tests; the new rate_unification test). Fix regressions — the seam is where "value in types, buffer in execution" meets.

- [ ] **Step 9: Verify examples compile**

Run:
```bash
cargo build -p rill-adrift --features "io,lofi,lang" --example lang_chiptune
cargo build -p rill-adrift --features "io,lang,sampler" --example record_mic
```
Expected: both compile (running requires audio hardware).

- [ ] **Step 10: Clippy + fmt + commit**

Run: `cargo clippy --all-features -p rill-lang`, `cargo fmt`.
Commit:

```bash
cargo fmt
git add rill-lang/src/types/ty.rs rill-lang/src/types/infer.rs rill-lang/src/ir.rs rill-lang/src/arena.rs rill-lang/src/lower.rs rill-lang/src/backend/interp.rs rill-lang/src/program.rs rill-lang/tests/rate_unification.rs
git commit -m 'refactor(rill-lang): full track unification — block track eliminated, everything is a closure
Rate::Signal removed; signals are Buffer-typed value channels. Block Instr/
block_regs/run_block_mimo/build_schedule deleted. FFI = fragment with a single
ValueCallForeign instruction over Value::Buffer. run_value_track merges into
run_call (program = root fragment).'
```

> **Note:** this is a large task. If it proves too big for one pass, split it:
> B3a (type unification: Rate collapse + Buffer vty, lower to value track,
> block track still runs the Buffer ops), B3b (runtime unification: delete
> block track, ValueCallForeign fragment, merge runners). Each lands and tests
> green before the next.

---

## Task B4: Buffer math as `Buffer` methods; Faust ops as Arrow methods; `:` overload

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
- `Seq` → `seq` method (Buffer instance; the `:` overload, Part 2 of this task)
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


### Part 2: `:` single overload (cons for `List`, seq for `Buffer`) (cons for `List`, seq for `Buffer`)


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
3. Else (signal/buffer context) → keep the Faust seq behavior (currently `reject_value_channels`; keep for now, re-target to `Buffer` in the Buffer-methods part of this task).
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
-- Buffer instance added in the Buffer-methods part of this task
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

In `infer.rs`, the `foreign fn` typing path (see `infer_ref`/the `ffi_sig_from_typeexpr` path) maps `FixedBuffer a` signal channels to `Rate::Value` channels with `vty = Buffer`. The call is an arrow `Buffer -> ... -> Buffer`; combine with the `:` seq from Task B4 (Part 2).

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

- [ ] §2.1 Approach A (`Kleisli ST Buffer Buffer`) → Tasks B2 (ST), B4 (arrows), B7 (FFI arrows).
- [ ] §2.2 `:` single overload (List→cons, Buffer→seq) → Task B4 (Part 2).
- [ ] §2.3 math is in-place mutation, scalar loops → Tasks B1, B4.
- [ ] §2.4 ST with phantom `s`, runST, check-escaping → Task B2 (DONE via sub-plan).
- [ ] §2.5 Faust operators as Arrow/Buffer methods → Task B4.
- [ ] §2.5b FFI integration (signature/factory/`BlockBuiltin` unchanged) → Task B7.
- [ ] §2.6 IO deferred; `Rate::Signal` removed; linear IR survives as execution layer → Task B3.
- [ ] §3 RegionHandle as a `Buffer` value → Task B6.
- [ ] §5 backward compat (existing programs, RT safety, SIMD preserved) → Tasks B3, B4, B7 verification.
- [ ] §6 testing (math, ST, `:`, Faust-as-Arrow, Buffer-as-value, FFI arrows) → each task's test file.