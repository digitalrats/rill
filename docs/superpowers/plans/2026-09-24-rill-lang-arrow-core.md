# rill-lang Arrow Core + CAF Free Variables Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Formalize rill-lang as a category of block arrows `(Block<Scalar>..) → (Block<Scalar>..)`, add Haskell-style free variables (CAF sharing for closed top-level definitions) with binding-level laziness, and migrate runtime storage from `Vec` to `rill_core::buffer` fixed buffers with a multichannel runtime.

**Architecture:** Two-level model — a Haskell-like meta-level (definitions, β-reduction, free-variable capture) over an object-level category of signal arrows (Faust combinators as arrow laws). Closed top-level definitions (0 input channels) become shared CAFs lifted once into the schedule. Runtime block storage becomes `FixedBuffer<T, BUF>` (pre-allocated, mutated in place). `RillProgram`/`ProgramEngine` are generified over `const BUF: usize`.

**Tech Stack:** Rust, rill-lang (lexer/parser/HM-infer/β-reduce/lower/schedule), rill-core (`buffer::FixedBuffer`, `buffer::DelayLine`, `traits::MultichannelAlgorithm`), cargo. Branch: `feature/rill-lang-vars`.

**Spec:** `docs/superpowers/specs/2026-09-24-rill-lang-arrow-core-design.md`

**Conventions:** `AGENTS.md` — zero warnings, English docs, `max_width=100`, `tab_spaces=4`, no `Vec` allocation in the RT path, conventional commits. TDD: failing test first, then implementation, then `cargo test -p rill-lang`, then commit.

---

## File map

| File | Role |
|---|---|
| `rill-lang/src/types/ty.rs` | `Scalar`, `Type` → `ArrowTy`, new `Block` |
| `rill-lang/src/types/arrow.rs` | (new) arrow-category docs + law tests |
| `rill-lang/src/types/infer.rs` | infer over arrow terms; CAF set on `TypedProgram` |
| `rill-lang/src/types/unify.rs` | scalar unification (touches `Type` references) |
| `rill-lang/src/ast.rs` | `Expr`/`Def`/`Program` (restructured in Phase 2) |
| `rill-lang/src/reduce.rs` | CAF-aware β-reduction |
| `rill-lang/src/lower.rs` | arrow interpreter; CAF lifting; const-fold edge case |
| `rill-lang/src/program.rs` | `RillProgram<T, const BUF>`; `FixedBuffer` store; `DelayLine` |
| `rill-lang/src/program_engine.rs` | `ProgramEngine<T, const BUF>` |
| `rill-lang/src/backend/interp.rs` | fixed-buffer executor (no `mem::take` moves) |
| `rill-lang/src/runtime.rs` | multichannel `launch`, pre-allocated channel buffers |
| `rill-lang/src/lib.rs` | `compile_graph::<T, BUF_SIZE>`; CAF plumbing |
| `rill-lang/src/parser.rs`, `render.rs`, `graph/*`, `schedule.rs`, `register.rs`, `builtin*.rs` | compile with renamed types |
| `rill-lang/tests/*` | behavior tests |
| `docs/src/guides/rill-lang.md` | language reference |

---

## Phase 1 — Type model: `Block`/`ArrowTy`

Rename `Type` → `ArrowTy`, add `Block { elem: Scalar }`, keep arity = channel count. Mechanical; compiler-verified.

### Task 1.1: Introduce `Block` and rename `Type` → `ArrowTy`

**Files:**
- Modify: `rill-lang/src/types/ty.rs`
- Test: compile via `cargo check -p rill-lang`

- [ ] **Step 1: Edit `types/ty.rs`** — add `Block`, rename `Type` to `ArrowTy` (type + impl + all uses within the file), add a deprecated alias for `Type` to keep the crate compiling during the rename:

```rust
/// A signal channel: one block of samples. `elem` is the per-sample scalar type.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// Scalar type of the samples in this channel's block.
    pub elem: Scalar,
}

impl Block {
    pub fn new(elem: Scalar) -> Self { Self { elem } }
}

/// A block transform: n input channels → m output channels.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrowTy {
    pub ins: Vec<Block>,
    pub outs: Vec<Block>,
}

/// Back-compat alias during the rename (removed at the end of Phase 1).
pub type Type = ArrowTy;
```

Update `ArrowTy` methods: `uniform(n_in, n_out, s)` builds `Block::new(s)` per wire; `arity_in()/arity_out()` return `len`. Update `Scheme.ty: ArrowTy`, `Subst::apply`, and all references in `ty.rs`.

- [ ] **Step 2: Verify compile**

Run: `cargo check -p rill-lang`
Expected: PASS (alias keeps old call sites compiling).

- [ ] **Step 3: Update `infer.rs`/`unify.rs`/`lower.rs` references** — replace `Type` uses with `ArrowTy` (they still compile via the alias; this makes the intent explicit), removing the alias at the end.

- [ ] **Step 4: Remove the `Type` alias**

```rust
// delete: pub type Type = ArrowTy;
```

- [ ] **Step 5: Verify full compile**

Run: `cargo check -p rill-lang`
Expected: PASS with zero `Type` references remaining.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/types/
git commit -m 'refactor(rill-lang): rename Type -> ArrowTy, introduce Block channel type'
```

### Task 1.2: Arrow-law tests

**Files:**
- Create: `rill-lang/src/types/arrow.rs`
- Test: `rill-lang/src/types/arrow.rs`

- [ ] **Step 1: Write failing law tests** (type-level identities; the combinators' typing functions already exist in `infer.rs` as `seq`/`par`/`split`/`merge`):

```rust
//! Arrow-category laws for the block-diagram combinators (see spec §2.3).
//!
//! The laws hold over arrow *types* (channel-count equations). Full AST-level
//! interchange is asserted structurally in `tests/graph_parse.rs`.

use super::ty::{ArrowTy, Block, Scalar};
use super::infer::{par, seq};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_unit_left() {
        let a = ArrowTy::uniform(1, 1, Scalar::Float);
        // `_ : A` ≡ `A` : same (1,1)
        assert_eq!(a.arity_in(), 1);
        assert_eq!(a.arity_out(), 1);
    }

    #[test]
    fn parallel_adds_arities() {
        let a = ArrowTy::uniform(1, 2, Scalar::Float);
        let b = ArrowTy::uniform(3, 1, Scalar::Float);
        let p = par(&a, &b);
        assert_eq!((p.arity_in(), p.arity_out()), (4, 3));
    }

    #[test]
    fn block_wire_is_not_scalar() {
        // A channel is a Block, not a Scalar: arity counts channels.
        let t = ArrowTy::uniform(2, 2, Scalar::Float);
        assert_eq!(t.ins.len(), 2);
        assert!(t.ins.iter().all(|b| b.elem == Scalar::Float));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-lang --lib types::arrow`
Expected: module doesn't exist → FAIL.

- [ ] **Step 3: Add `arrow.rs` module and wire it into `types/mod.rs`** (`pub mod arrow;`).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p rill-lang --lib`
Expected: PASS (new + existing).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/
git commit -m 'test(rill-lang): arrow-category law tests for Block/ArrowTy'
```

---

## Phase 2 — Arrow core restructure

Restructure `ast::Expr` into an explicit arrow core `ArrowExpr` (no duplicate AST — constructors are renamed/regrouped by category). The parser, inference, reduction, lowering, and graph reconstruction keep their pipeline; this phase is a rename+reorganization with compiler-verified equivalences.

### Task 2.1: Regroup `Expr` constructors into arrow-category groups

**Files:**
- Modify: `rill-lang/src/ast.rs`

- [ ] **Step 1: In `ast.rs`, add category doc comments and rename `Expr::Bin`/`BinOp` for arrow clarity** — introduce `Expr::Seq`, `Expr::Par`, `Expr::Split`, `Expr::Merge`, `Expr::Loop`, `Expr::Delay` as first-class variants (replacing the generic `Bin` with `BinOp`), keeping the arithmetic `Bin` variants (`Add/Sub/Mul/Div/Rem`) under `Expr::Arith`. Update `Expr::span()`.

- [ ] **Step 2: Update all match sites** (`parser.rs`, `infer.rs`, `reduce.rs`, `lower.rs`, `render.rs`, `graph/reconstruct.rs`) to the new variants.

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p rill-lang`
Expected: PASS (behavior unchanged).

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/
git commit -m 'refactor(rill-lang): restructure Expr into explicit arrow combinators'
```

### Task 2.2: `ArrowExpr` alias module + category doc

**Files:**
- Create: `rill-lang/src/arrow.rs`
- Modify: `rill-lang/src/lib.rs`

- [ ] **Step 1: Create `arrow.rs`** — module doc re-exporting the arrow combinators with the law table (spec §2.3) and the `ArrowTy`-indexed arity rules.

- [ ] **Step 2: Register the module and re-export from `lib.rs`** (`pub mod arrow;`).

- [ ] **Step 3: Compile + commit**

```bash
cargo check -p rill-lang && git add rill-lang/src/arrow.rs rill-lang/src/lib.rs && git commit -m 'docs(rill-lang): arrow module with category laws'
```

---

## Phase 3 — `rill_core::buffer` block storage

Generify `RillProgram`/`ProgramEngine` over `const BUF: usize` and swap `Vec<Vec<T>>` storage for `Vec<FixedBuffer<T, BUF>>`; replace the hand-rolled `DelayRing` with `DelayLine<T, MAX_DELAY>`.

### Task 3.1: Generify `RillProgram<T, const BUF: usize>`

**Files:**
- Modify: `rill-lang/src/program.rs`, `program_engine.rs`, `lib.rs`, `backend/interp.rs`, `schedule.rs`

- [ ] **Step 1: Add the const parameter** to `RillProgram`, `ProgramEngine`, `RillProgram::new`/`new_with`/`new_with_resources`, and all constructors. Choose `BUF = 256` in `compile`/`compile_with` (runtime-safe default); `compile_graph` takes it as an explicit const argument.

- [ ] **Step 2: Convert the register store.** Replace `block_regs: Vec<Vec<T>>` with `block_regs: Vec<FixedBuffer<T, BUF>>`, allocated once at construction (`num_regs` entries). `block_state`/`block_state_next` likewise. Replace `ensure_block_len` (grown-on-demand) with a fixed-size preallocation — `FixedBuffer` is always `BUF` long; the executor operates on `reg[..n]` for the current tick length `n`.

- [ ] **Step 3: Update `backend/interp.rs`.** Remove the `mem::take(&mut Vec)` buffer-move dance: index `prog.block_regs` directly. Where a step writes `dst` from `src` (same `Vec<FixedBuffer>`), use a two-phase read/write or `split_at_mut` to satisfy the borrow checker (read `src` into a small stack scratch `[T; BUF]` first, then write `dst`), preserving RT-safety (no allocation, stack scratch).

- [ ] **Step 4: Replace `DelayRing` with `DelayLine<T, MAX_DELAY>`** where `MAX_DELAY` = max `@ n` across sites (assert at compile; fall back to `TapeLoop`-style heap for oversized delays if needed). Update `read_block`/`write_block` call sites in `interp.rs` and `program.rs::reset`.

- [ ] **Step 5: Run full test suite**

Run: `cargo test -p rill-lang`
Expected: PASS (behavior identical; hybrid/reference equivalence tests in `tests/` must hold).

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/
git commit -m 'perf(rill-lang): FixedBuffer block storage, const BUF generic, DelayLine delays'
```

---

## Phase 4 — Multichannel runtime

### Task 4.1: Drive all channels in `Runtime::launch`

**Files:**
- Modify: `rill-lang/src/runtime.rs`
- Test: `rill-lang/tests/multichannel_runtime.rs` (new)

- [ ] **Step 1: Write a failing multichannel test** — a 2→2 program (`main = _ , _` style) processed through a fake capture/playback with 2 channels each:

```rust
// tests/multichannel_runtime.rs
use rill_core::io::{IoCapture, IoPlayback};
use rill_lang::{compile_graph, Runtime};
use rill_lang::program_runner::ProgramRunner;
use std::sync::{atomic::AtomicBool, Arc};

struct FakeCap { ch: usize }
impl IoCapture for FakeCap {
    fn num_input_channels(&self) -> usize { self.ch }
    fn read_input(&self, channel: usize, dst: &mut [f32]) -> bool {
        dst.fill(if channel == 0 { 1.0 } else { 2.0 });
        true
    }
}

struct FakePb { ch: usize, out: Vec<Vec<f32>> }
impl IoPlayback for FakePb {
    fn num_output_channels(&self) -> usize { self.ch }
    fn write_output(&mut self, channel: usize, src: &[f32]) -> bool {
        self.out[channel].extend_from_slice(src);
        true
    }
}
```

(Compile a 2→2 program, drive one tick via `Runtime::launch_stream`, assert both output channels received data.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-lang --test multichannel_runtime`
Expected: FAIL — only channel 0 is driven.

- [ ] **Step 3: Rewrite `Runtime::launch`.** Read backend channel counts at launch; verify against `program.num_inputs()/num_outputs()`; pre-allocate `[FixedBuffer<f32, BUF>; MAX_CHANNELS]` (const `MAX_CHANNELS = 8`) before `driver.run()`; per tick, `read_input(c, …)` for each input channel, call `program.apply`, `write_output(c, …)` for each output channel. Keep the duplex path channel-aware.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/runtime.rs rill-lang/tests/multichannel_runtime.rs
git commit -m 'feat(rill-lang): multichannel Runtime::launch drives all backend channels'
```

---

## Phase 5 — CAF free variables + binding-level laziness (core feature)

### Task 5.1: Compute the CAF set in inference

**Files:**
- Modify: `rill-lang/src/types/infer.rs`

- [ ] **Step 1: Write a failing test** (in `infer.rs`):

```rust
#[test]
fn closed_top_level_local_is_caf() {
    // osc = sine 440 0.5 0  (0 input channels, 0 λ-params) -> CAF
    let src = "osc = sine 440 0.5 0; main = _ * 0.5";
    // TestSigs must register `sine` (0 signal ins, 1 out)
    let typed = ty_with(src).unwrap();
    assert!(typed.cafs.contains("osc"));
}

#[test]
fn open_block_is_not_caf() {
    // gain = _ * 0.5  (1 input channel) -> macro, not CAF
    let src = "gain = _ * 0.5; main = _ * 0.5";
    let typed = ty_with(src).unwrap();
    assert!(!typed.cafs.contains("gain"));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --lib types::infer`
Expected: FAIL — `TypedProgram` has no `cafs` field.

- [ ] **Step 3: Add `cafs: HashSet<String>` to `TypedProgram`.** In `infer_def_group`, after the final pass, collect top-level `Def::Local` defs whose `Scheme` has `lam_count == 0 && ty.ins.is_empty()`. Expose the set.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-lang --lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/infer.rs
git commit -m 'feat(rill-lang): infer CAF set for closed top-level definitions'
```

### Task 5.2: Reduce leaves CAF references

**Files:**
- Modify: `rill-lang/src/reduce.rs`

- [ ] **Step 1: Write a failing test**:

```rust
#[test]
fn caf_ref_is_not_inlined() {
    // osc = sine 440 0.5 0; main = osc , osc
    // After reduce, main must still contain a Ref to `osc` (not two inlined sines).
    let src = "osc = sine 440 0.5 0; main = osc , osc";
    let tokens = tokenize(src).unwrap();
    let program = parser::parse(&tokens, src.as_bytes()).unwrap();
    let typed = infer_program(&program).unwrap();
    let cafs = typed.cafs.clone();
    let reduced = reduce_with_cafs(&program, &cafs);
    let main = reduced.main_def().unwrap();
    // assert the body references `osc` as a Var/Ref rather than inlining
    assert!(contains_name(main.body(), "osc"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-lang --lib reduce`
Expected: FAIL — today `osc` is inlined.

- [ ] **Step 3: Add `reduce_with_cafs(program, cafs)`** (and keep `reduce(program)` calling it with an empty set for back-compat). In `reduce_expr`, when `Expr::Ref(name)` (or the `Var` form) targets a name in `cafs`, leave it un-inlined in both passes. CAF bodies still get β-reduced.

- [ ] **Step 4: Update `lib.rs`** compile pipeline to pass `typed.cafs` into reduce.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rill-lang --lib`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/reduce.rs rill-lang/src/lib.rs
git commit -m 'feat(rill-lang): reduce keeps CAF references un-inlined'
```

### Task 5.3: Lift CAFs once in lowering (shared instance)

**Files:**
- Modify: `rill-lang/src/lower.rs`
- Test: `rill-lang/src/lower.rs`

- [ ] **Step 1: Write failing tests**:

```rust
#[test]
fn closed_caf_lowers_to_single_instance() {
    // osc = sine 440 0.5 0; main = osc , osc  -> ONE sine builtin
    let ir = ir_with_cafs("osc = sine 440 0.5 0; main = osc , osc");
    let sines = ir.builtins.iter().filter(|b| b.name == "sine").count();
    assert_eq!(sines, 1);
}

#[test]
fn open_block_stays_macro() {
    // integ = + ~ _; main = integ , integ  -> 2 state slots
    let ir = ir_of("integ = + ~ _; main = integ , integ");
    assert_eq!(ir.state.block_state_slots, 2);
}

#[test]
fn caf_const_param_still_folds() {
    // cutoff = 1000.0; main = _ : lowpass cutoff 0.7 -> param 1000.0
    let ir = ir_with("cutoff = 1000.0; main = _ : lowpass cutoff 0.7");
    let lp = ir.builtins.iter().find(|b| b.name == "lowpass").unwrap();
    assert!((lp.params[0] - 1000.0).abs() < 1e-9);
}

#[test]
fn recursive_caf_is_error_not_overflow() {
    // a = a  -> compile error
    assert!(compile_with("a = a; main = _").is_err());
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --lib lower`
Expected: FAIL — first test sees 2 sines; recursion overflows.

- [ ] **Step 3: Implement CAF lifting in `lower.rs`.** Add `cafs: &HashSet<String>` to `Lowerer`/`lower_with`. Add a `caf_cache: HashMap<String, Vec<usize>>`. In `lower_ref` for a `Def::Local` in `cafs`: if cached, return cached registers; else mark "lifting in progress" (recursion guard — return `CompileError::Type` on re-entry), lower the body with no input args, cache, and return. For builtin const-param positions (Float/Int/Record), when the arg is a `Var`/`Ref` to a closed CAF, `const_f64` its body (fold) instead of erroring.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rill-lang --lib`
Expected: PASS (all four).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lower.rs
git commit -m 'feat(rill-lang): lift closed CAFs once, share instances; recursion guard'
```

### Task 5.4: Binding-level laziness (DCE by reference)

**Files:**
- Modify: `rill-lang/src/lower.rs`, `rill-lang/src/reduce.rs`

- [ ] **Step 1: Write a failing test**:

```rust
#[test]
fn unreferenced_caf_is_not_lowered() {
    // dead = sine 440 0.5 0; main = _ * 0.5  -> no sine builtin
    let ir = ir_with_cafs("dead = sine 440 0.5 0; main = _ * 0.5");
    assert!(!ir.builtins.iter().any(|b| b.name == "sine"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-lang --lib lower`
Expected: FAIL — the unused CAF is still lifted (or the body is inline-referenced).

- [ ] **Step 3: Make CAF lifting lazy** — only lower a CAF body when its `Var` is actually referenced during lowering (natural consequence of the cache in Task 5.3; ensure the top-level def scan does not eagerly lower CAFs).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rill-lang --lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lower.rs
git commit -m 'feat(rill-lang): lazy CAF lowering — unreferenced definitions are dead code'
```

---

## Phase 6 — Tests, docs, verification

### Task 6.1: Feature integration tests

**Files:**
- Create: `rill-lang/tests/caf_free_variables.rs`

- [ ] **Step 1: Write integration tests** (behavioral, using `compile`/`compile_with`):

```rust
// caf_free_variables.rs
#[test]
fn shared_oscillator_single_phase() {
    // osc shared: both channels see the same oscillator phase progression.
    let mut prog = compile_with::<f32>(
        "osc = sine 440.0 1.0 0.0; main = osc , osc",
        &reg, 44100.0,
    ).unwrap();
    // feed 2 input channels (unused by osc), assert both outputs are identical
    // block-by-block over several ticks.
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p rill-lang --test caf_free_variables`
Expected: PASS.

- [ ] **Step 3: Verify no regressions**

Run: `cargo test -p rill-lang`
Expected: PASS (all existing tests).

- [ ] **Step 4: Commit**

```bash
git add rill-lang/tests/caf_free_variables.rs
git commit -m 'test(rill-lang): CAF free-variable integration tests'
```

### Task 6.2: Update the language guide

**Files:**
- Modify: `docs/src/guides/rill-lang.md`

- [ ] **Step 1: Document** the block-arrow model (program = `(Block<Scalar>..) → (Block<Scalar>..)` per-tick transform; Scalar/Block/ArrowTy levels), CAF semantics (§7 of the spec), binding-level laziness, per-tick execution, and the behavior change (§8: closed stateful locals referenced ≥2× become one shared instance).

- [ ] **Step 2: Verify docs build**

Run: `mdbook build docs/`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add docs/src/guides/rill-lang.md
git commit -m 'docs: rill-lang block-arrow model, CAF semantics, behavior change'
```

### Task 6.3: Full verification

- [ ] **Step 1: Workspace check**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 2: Clippy + fmt**

Run: `cargo clippy --workspace` and `cargo fmt`
Expected: zero warnings; formatting applied.

- [ ] **Step 3: Update checkpoint + session memory** per mind protocol.
