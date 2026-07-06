# rill-lang DSP/Model Built-ins (FFI Registry) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let rill-lang programs call stateful DSP built-ins from `rill-core-dsp` / `rill-core-model` via an extensible foreign-function registry: sample-level built-ins (`process_sample`, feedback-legal) and block-level built-ins (`Algorithm`, opaque block step). Bindings live in `rill-adrift`; rill-lang core stays `rill-core`-only.

**Architecture:** rill-lang core adds a `Registry<T>` + `SampleBuiltin<T>` trait and a `SignatureSource` view. `compile_with::<T>(src, &registry, sample_rate)` resolves built-in calls during inference/lowering, emits `Instr::CallSample`/`Instr::CallBlock` plus a `Ir.builtins` side table, and `RillProgram::new_with` builds the boxed instances. The scheduler routes `CallSample` into sample regions and `CallBlock` into a new `Step::ForeignBlock`, rejecting block built-ins inside feedback. Existing `compile()` is unchanged (empty registry).

**Tech Stack:** Rust 2021, `max_width=100`, `#![deny(unsafe_code)]`. rill-lang core: no new deps. `rill-adrift`: uses existing `rill-core-dsp`, and `rill-core-model`/`rill-analog-filters` behind the existing `analog` feature.

**Design:** `docs/superpowers/specs/2026-07-06-rill-lang-dsp-builtins-design.md`.
**Branch:** `feature/rill-lang`.

**Locked conventions:**
- Built-ins take **constant params as args**; **signals flow via combinators**. A built-in reference has arity `(signal_ins → signal_outs)`; `signal_outs == 1` in this increment.
- Params are constant expressions folded to `f64` (reuse `lower::const_fold` logic; a float-valued fold — extend it to accept float literals/arith).
- To preserve every existing test, add **`_with` variants** and keep the old entry points delegating to an empty registry: `infer_program(prog)` → `infer_program_with(prog, &NoSigs)`, `lower(tp)` → `lower_with(tp, &NoSigs)`, `RillProgram::new(ir)` → builds zero built-ins. `compile()` uses the empty path.
- `CallSample` is stateful (feedback-legal). `CallBlock` is a `Step::ForeignBlock` and is **illegal inside feedback** (compile error).

---

## File Structure

```
rill-lang/src/
  builtin.rs         # NEW: SampleBuiltin, BuiltinKind, BuiltinSig, Registry<T>, SignatureSource, NoSigs
  ir.rs              # MODIFY: Instr::CallSample/CallBlock, BuiltinInstance, Ir.builtins
  types/infer.rs     # MODIFY: infer_program_with(&dyn SignatureSource); builtin arity + param checks
  lower.rs           # MODIFY: lower_with(&dyn SignatureSource); emit CallSample/CallBlock + ir.builtins; float const fold
  schedule.rs        # MODIFY: is_stateful(CallSample), CallBlock→ForeignBlock step, arity helpers
  backend/interp.rs  # MODIFY: exec CallSample (sample region) + ForeignBlock; reference handles both
  program.rs         # MODIFY: sample_builtins/block_builtins; new_with(ir,&registry,sr); init/reset
  lib.rs             # MODIFY: pub mod builtin; compile_with(); re-exports
rill-adrift/src/
  lang_builtins.rs   # NEW: register_dsp_builtins, register_model_builtins (#[cfg analog]), full_registry
  lib.rs             # MODIFY: pub use of the above under `lang`
rill-lang/tests/builtins.rs   # NEW: (uses a local test registry) — but real bindings are in adrift
rill-adrift/tests/lang_builtins.rs # NEW: integration (dsp/model builtins)
```

---

## Task D1: Registry + `SampleBuiltin` trait

**Files:** Create `rill-lang/src/builtin.rs`; modify `rill-lang/src/lib.rs` (`pub mod builtin;`). Test in `builtin.rs`.

- [ ] **Step 1: Create `rill-lang/src/builtin.rs`**

```rust
//! Foreign-function registry: DSP/model built-ins callable from rill-lang.
//!
//! Two kinds: [`SampleBuiltin`] (per-sample, feedback-legal) and block built-ins
//! (`rill_core::Algorithm`, opaque whole-buffer). Concrete bindings live outside
//! this crate (e.g. `rill-adrift`); core stays `rill-core`-only.

use std::collections::HashMap;

use rill_core::math::Transcendental;
use rill_core::traits::Algorithm;

/// A stateful per-sample built-in: `signal_ins` inputs → 1 output.
pub trait SampleBuiltin<T: Transcendental>: Send {
    /// Process one sample. `inputs.len() == signal_ins`.
    fn process_sample(&mut self, inputs: &[T]) -> T;
    /// Re-initialise for a sample rate (default no-op).
    fn init(&mut self, _sample_rate: f32) {}
    /// Clear internal state.
    fn reset(&mut self);
}

/// Whether a built-in is per-sample or whole-buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinKind {
    /// Per-sample [`SampleBuiltin`].
    Sample,
    /// Whole-buffer `Algorithm` (1→1).
    Block,
}

/// Type-checker-facing signature of a built-in (independent of `T`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinSig {
    /// Registered name.
    pub name: &'static str,
    /// Number of signal inputs.
    pub signal_ins: usize,
    /// Number of signal outputs (1 in this increment).
    pub signal_outs: usize,
    /// Number of constant params.
    pub num_params: usize,
    /// Sample vs block.
    pub kind: BuiltinKind,
}

/// A boxed factory building an instance from folded params + a sample rate.
type SampleFactory<T> = Box<dyn Fn(&[f64], f32) -> Box<dyn SampleBuiltin<T>> + Send + Sync>;
type BlockFactory<T> = Box<dyn Fn(&[f64], f32) -> Box<dyn Algorithm<T>> + Send + Sync>;

enum Factory<T: Transcendental> {
    Sample(SampleFactory<T>),
    Block(BlockFactory<T>),
}

/// A registry entry.
pub struct Entry<T: Transcendental> {
    /// The signature.
    pub sig: BuiltinSig,
    factory: Factory<T>,
}

impl<T: Transcendental> Entry<T> {
    /// Build a sample instance (panics if this entry is a block built-in — callers
    /// gate on `sig.kind`).
    pub fn build_sample(&self, params: &[f64], sample_rate: f32) -> Option<Box<dyn SampleBuiltin<T>>> {
        match &self.factory {
            Factory::Sample(f) => Some(f(params, sample_rate)),
            Factory::Block(_) => None,
        }
    }
    /// Build a block instance.
    pub fn build_block(&self, params: &[f64], sample_rate: f32) -> Option<Box<dyn Algorithm<T>>> {
        match &self.factory {
            Factory::Block(f) => Some(f(params, sample_rate)),
            Factory::Sample(_) => None,
        }
    }
}

/// A collection of built-in definitions.
pub struct Registry<T: Transcendental> {
    entries: HashMap<String, Entry<T>>,
}

impl<T: Transcendental> Default for Registry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental> Registry<T> {
    /// An empty registry.
    pub fn new() -> Self {
        Self { entries: HashMap::new() }
    }

    /// Register a per-sample built-in.
    pub fn register_sample(
        &mut self,
        sig: BuiltinSig,
        factory: impl Fn(&[f64], f32) -> Box<dyn SampleBuiltin<T>> + Send + Sync + 'static,
    ) {
        debug_assert_eq!(sig.kind, BuiltinKind::Sample);
        self.entries.insert(sig.name.to_string(), Entry { sig, factory: Factory::Sample(Box::new(factory)) });
    }

    /// Register a whole-buffer (`Algorithm`) built-in.
    pub fn register_block(
        &mut self,
        sig: BuiltinSig,
        factory: impl Fn(&[f64], f32) -> Box<dyn Algorithm<T>> + Send + Sync + 'static,
    ) {
        debug_assert_eq!(sig.kind, BuiltinKind::Block);
        self.entries.insert(sig.name.to_string(), Entry { sig, factory: Factory::Block(Box::new(factory)) });
    }

    /// Look up an entry by name.
    pub fn get(&self, name: &str) -> Option<&Entry<T>> {
        self.entries.get(name)
    }
}

/// A `T`-independent signature lookup used by the type checker and lowering.
pub trait SignatureSource {
    /// The signature for `name`, if registered.
    fn builtin_sig(&self, name: &str) -> Option<&BuiltinSig>;
}

impl<T: Transcendental> SignatureSource for Registry<T> {
    fn builtin_sig(&self, name: &str) -> Option<&BuiltinSig> {
        self.entries.get(name).map(|e| &e.sig)
    }
}

/// A signature source with no built-ins (used by `compile()` / existing tests).
pub struct NoSigs;
impl SignatureSource for NoSigs {
    fn builtin_sig(&self, _name: &str) -> Option<&BuiltinSig> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Gain {
        k: f32,
    }
    impl SampleBuiltin<f32> for Gain {
        fn process_sample(&mut self, inputs: &[f32]) -> f32 {
            inputs[0] * self.k
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn register_and_lookup_sample() {
        let mut reg = Registry::<f32>::new();
        reg.register_sample(
            BuiltinSig { name: "gain", signal_ins: 1, signal_outs: 1, num_params: 1, kind: BuiltinKind::Sample },
            |params, _sr| Box::new(Gain { k: params[0] as f32 }),
        );
        let sig = reg.builtin_sig("gain").unwrap();
        assert_eq!((sig.signal_ins, sig.num_params), (1, 1));
        let mut inst = reg.get("gain").unwrap().build_sample(&[0.5], 44100.0).unwrap();
        assert_eq!(inst.process_sample(&[2.0]), 1.0);
        assert!(reg.builtin_sig("missing").is_none());
    }
}
```

- [ ] **Step 2:** Add `pub mod builtin;` to `rill-lang/src/lib.rs`.

- [ ] **Step 3:** Run `cargo test -p rill-lang builtin` → 1 test passes. `cargo clippy -p rill-lang --all-targets` → clean. `cargo fmt -p rill-lang`.

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/builtin.rs rill-lang/src/lib.rs
git commit -m 'feat(rill-lang): built-in FFI registry (SampleBuiltin, Registry, SignatureSource)'
```

---

## Task D2: IR additions

**Files:** Modify `rill-lang/src/ir.rs`.

- [ ] **Step 1:** In `rill-lang/src/ir.rs`, add the built-in instance descriptor and IR field, and two `Instr` variants.

Add near the top (after the `use`s / `BinArith`):

```rust
use crate::builtin::BuiltinKind;

/// A resolved built-in call site: its name, folded constant params, and kind.
/// Runtime instances are built from these by `RillProgram::new_with`.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinInstance {
    /// Registered built-in name.
    pub name: String,
    /// Folded constant params.
    pub params: Vec<f64>,
    /// Sample vs block.
    pub kind: BuiltinKind,
}
```

Add to `enum Instr` (note: `BuiltinKind` derives `Eq` but `BuiltinInstance`/`Ir`
already use `f64`, so keep `PartialEq` only on `Instr`):

```rust
    /// Call a per-sample built-in: `srcs` inputs → `dst`, instance index.
    CallSample { dst: Reg, srcs: Vec<Reg>, instance: usize },
    /// Call a whole-buffer built-in (1→1): `src` → `dst`, instance index.
    CallBlock { dst: Reg, src: Reg, instance: usize },
```

Add to `struct Ir`:

```rust
    /// Built-in call-site descriptors, indexed by the `instance` field of
    /// `CallSample`/`CallBlock`.
    pub builtins: Vec<BuiltinInstance>,
```

- [ ] **Step 2:** Fix every `Ir { … }` construction to include `builtins`. The only
  constructor is in `lower.rs::lower` — Task D4 updates it; for now, to keep the
  crate compiling after D2, temporarily add `builtins: Vec::new()` to that literal.
  Also any test that builds an `Ir` literal must add `builtins: Vec::new()`.

Run `cargo build -p rill-lang`. If it fails only on missing `builtins:` fields,
add `builtins: Vec::new()` at those sites. Run `cargo test -p rill-lang` → existing
tests still pass (new variants unused yet).

- [ ] **Step 3: Commit**

```bash
git add rill-lang/src/ir.rs rill-lang/src/lower.rs
git commit -m 'feat(rill-lang): IR CallSample/CallBlock + builtins side table'
```

---

## Task D3: Inference + lowering registry integration

**Files:** Modify `rill-lang/src/types/infer.rs`, `rill-lang/src/lower.rs`. Tests in both.

### D3a — inference

- [ ] **Step 1:** In `infer.rs`, thread a `&dyn SignatureSource` through inference.

Add `use crate::builtin::{BuiltinKind, SignatureSource};`. Add a field to `Ctx`:
`sigs: &'a dyn SignatureSource` (make `Ctx<'a>` lifetime-generic). Change the
public API:

```rust
/// Back-compat: infer with no built-ins.
pub fn infer_program(program: &Program) -> Result<TypedProgram, CompileError> {
    infer_program_with(program, &crate::builtin::NoSigs)
}

/// Infer with a signature source for built-in resolution.
pub fn infer_program_with(
    program: &Program,
    sigs: &dyn SignatureSource,
) -> Result<TypedProgram, CompileError> {
    // identical body to the old infer_program, but Ctx carries `sigs`.
    // ...
}
```

- [ ] **Step 2:** In `infer_ref` and `infer_apply`, consult the registry **before**
  the user-def / unknown-identifier paths.

In `infer_ref(ctx, name, span)` — after the builtin math/arith checks and before
the `locals`/`defs` lookup, add: if `ctx.sigs.builtin_sig(name)` is `Some(sig)` and
`sig.num_params == 0`, return `Type::uniform(sig.signal_ins, sig.signal_outs,
Scalar::Float)` (a 0-param built-in used bare).

In `infer_apply(ctx, name, args, span)` — at the very start, add:

```rust
if let Some(sig) = ctx.sigs.builtin_sig(name) {
    let sig = sig.clone();
    // Built-in call: args are CONSTANT params, not signals.
    if args.len() != sig.num_params {
        return Err(CompileError::Type {
            msg: format!("built-in `{name}` expects {} param(s), got {}", sig.num_params, args.len()),
            span,
        });
    }
    for a in args {
        // Each param must be a constant (arity 0→1). Type-check it (folds later).
        let at = infer_expr(ctx, a)?;
        if at.arity_in() != 0 || at.arity_out() != 1 {
            return Err(CompileError::Type {
                msg: format!("param to `{name}` must be a constant expression"),
                span: a.span(),
            });
        }
    }
    return Ok(Type::uniform(sig.signal_ins, sig.signal_outs, Scalar::Float));
}
```

(The existing user-def application logic stays after this block.)

- [ ] **Step 3:** Add tests in `infer.rs` (`mod tests`) using a small local
  `SignatureSource`:

```rust
struct TestSigs;
impl crate::builtin::SignatureSource for TestSigs {
    fn builtin_sig(&self, name: &str) -> Option<&crate::builtin::BuiltinSig> {
        use crate::builtin::{BuiltinKind, BuiltinSig};
        // leak a 'static sig for the test
        match name {
            "lowpass" => Some(Box::leak(Box::new(BuiltinSig {
                name: "lowpass", signal_ins: 1, signal_outs: 1, num_params: 2, kind: BuiltinKind::Block,
            }))),
            "onepole" => Some(Box::leak(Box::new(BuiltinSig {
                name: "onepole", signal_ins: 1, signal_outs: 1, num_params: 2, kind: BuiltinKind::Sample,
            }))),
            _ => None,
        }
    }
}

fn ty_with(src: &str) -> Result<TypedProgram, CompileError> {
    infer_program_with(&parse(&tokenize(src).unwrap()).unwrap(), &TestSigs)
}

#[test]
fn builtin_call_is_1_to_1() {
    let t = ty_with("process = _ : lowpass(1000.0, 0.7);").unwrap();
    assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
}
#[test]
fn builtin_wrong_param_count_errors() {
    assert!(ty_with("process = _ : lowpass(1000.0);").is_err());
}
#[test]
fn builtin_non_const_param_errors() {
    assert!(ty_with("process = _ : lowpass(_, 0.7);").is_err());
}
#[test]
fn sample_builtin_in_feedback_typechecks() {
    // legality is enforced only for BLOCK builtins, and at schedule time.
    assert!(ty_with("process = + ~ onepole(200.0, 0.5);").is_ok());
}
```

> Note: `Box::leak` in tests is acceptable (test-only). The real registry provides
> `&'static`-lifetime sigs via `&BuiltinSig` borrowed from the registry map.

- [ ] **Step 4:** Run `cargo test -p rill-lang infer` → all pass (old + new). Update
  any internal callers of `infer_program` if needed (schedule/lower test helpers
  still call `infer_program` which now delegates — no change needed).

### D3b — lowering

- [ ] **Step 5:** In `lower.rs`, thread `&dyn SignatureSource` and emit built-in
  instrs. Add `use crate::builtin::{BuiltinInstance, BuiltinKind, SignatureSource};`.
  Give `Lowerer` a `sigs: &'a dyn SignatureSource` and a `builtins: Vec<BuiltinInstance>`.

Public API:

```rust
pub fn lower(tp: &TypedProgram) -> Result<Ir, CompileError> {
    lower_with(tp, &crate::builtin::NoSigs)
}
pub fn lower_with(tp: &TypedProgram, sigs: &dyn SignatureSource) -> Result<Ir, CompileError> {
    // same as old lower, but Lowerer carries `sigs` + `builtins`,
    // and the final Ir includes `builtins: lw.builtins`.
}
```

- [ ] **Step 6:** In `Lowerer::lower_ref` (bare ref) and the `Expr::Apply` arm,
  handle built-ins. Add a float-capable constant folder `const_f64(e) -> Option<f64>`
  (like the existing `const_int` but for floats: `Float(v)`, `Int(v) as f64`,
  `Neg`, arithmetic). Then:

In the `Expr::Apply { name, args, span }` arm of `Lowerer::lower`, before the
existing "(a,b,...) : name" logic:

```rust
if let Some(sig) = self.sigs.builtin_sig(name) {
    let sig = sig.clone();
    // Fold params.
    let mut params = Vec::with_capacity(args.len());
    for a in args {
        let v = const_f64(a).ok_or_else(|| CompileError::Type {
            msg: format!("param to `{name}` must be a constant"),
            span: a.span(),
        })?;
        params.push(v);
    }
    let instance = self.builtins.len();
    self.builtins.push(BuiltinInstance { name: name.clone(), params, kind: sig.kind });
    let dst = self.fresh_reg();
    match sig.kind {
        BuiltinKind::Sample => {
            let srcs = inputs.to_vec(); // signal_ins incoming regs
            self.emit(Instr::CallSample { dst, srcs, instance });
        }
        BuiltinKind::Block => {
            // MVP: block builtins are 1→1.
            self.emit(Instr::CallBlock { dst, src: inputs[0], instance });
        }
    }
    return Ok(vec![dst]);
}
```

Also handle a **bare 0-param built-in** in `lower_ref` (mirror the above with
empty params, using `inputs` as srcs). Add `const_f64`:

```rust
fn const_f64(e: &Expr) -> Option<f64> {
    match e {
        Expr::Float(v, _) => Some(*v),
        Expr::Int(v, _) => Some(*v as f64),
        Expr::Neg(inner, _) => const_f64(inner).map(|v| -v),
        Expr::Bin { op, lhs, rhs, .. } => {
            let a = const_f64(lhs)?;
            let b = const_f64(rhs)?;
            Some(match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => a / b,
                _ => return None,
            })
        }
        _ => None,
    }
}
```

- [ ] **Step 7:** Add lowering tests (`mod tests` in `lower.rs`) with a local
  `SignatureSource` (reuse the `TestSigs` pattern), asserting `_ : onepole(200.0,0.5)`
  lowers to a `CallSample` with one src + one `BuiltinInstance{kind:Sample}`, and
  `_ : lowpass(1000.0,0.7)` to a `CallBlock` + `BuiltinInstance{kind:Block}`.

- [ ] **Step 8:** Run `cargo test -p rill-lang` → all pass. `cargo clippy -p rill-lang --all-targets` → clean. `cargo fmt`.

- [ ] **Step 9: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/src/lower.rs
git commit -m 'feat(rill-lang): resolve built-in calls in inference + lowering (const params)'
```

---

## Task D4: Scheduler + executor + `RillProgram` + `compile_with`

**Files:** Modify `rill-lang/src/schedule.rs`, `backend/interp.rs`, `program.rs`, `lib.rs`.

### D4a — scheduler

- [ ] **Step 1:** In `schedule.rs`:
  - Add `Step::ForeignBlock(usize)` to the `Step` enum (doc: "an opaque
    whole-buffer built-in").
  - In `instr_dst`: `CallSample { dst, .. } | CallBlock { dst, .. } => Some(dst)`.
  - In `instr_srcs`: `CallSample { srcs, .. } => srcs.clone()`, `CallBlock { src, .. } => vec![*src]`.
  - In `is_stateful`: add `Instr::CallSample { .. } => true` (per-sample state). Do
    **not** mark `CallBlock` stateful for `is_stateful` (it's handled as its own
    step) — instead, in classification, detect `CallBlock` singletons.
  - In classification: after computing each SCC, if the SCC is a single instr that
    is `CallBlock` → `Step::ForeignBlock(idx)`. Else keep the existing rule
    (recurrent/stateful → `Sample`, else `Block`). Since `CallBlock` is not marked
    `is_stateful`, a lone `CallBlock` is a singleton non-recurrent SCC → route it to
    `ForeignBlock`. (A `CallBlock` that lands in a multi-instr SCC = inside feedback;
    it will end up in a `Sample` region — Task D4c validates and rejects that.)

- [ ] **Step 2:** Add schedule tests: `_ : onepole(...)` → one Sample region; `_ :
  lowpass(...)` → one ForeignBlock step + block ops for LoadInput; `+ ~ lowpass(...)`
  → a Sample region that (incorrectly) contains a CallBlock (used by D4c's validation
  test). Use the `TestSigs` pattern.

### D4b — executor

- [ ] **Step 3:** In `backend/interp.rs`:
  - `run_block_hybrid`: add a match arm `Step::ForeignBlock(idx) => exec_foreign_block(prog, *idx, n)`.
  - Add `exec_foreign_block`:

```rust
fn exec_foreign_block<T: Transcendental>(prog: &mut RillProgram<T>, idx: usize, n: usize) {
    if let Instr::CallBlock { dst, src, instance } = prog.ir.instrs[idx].clone() {
        let mut out = std::mem::take(&mut prog.block_regs[dst]);
        let _ = prog.block_builtins[instance].process(Some(&prog.block_regs[src][..n]), &mut out[..n]);
        prog.block_regs[dst] = out;
    }
}
```

  - In `exec_sample_region`'s instr match, add:

```rust
Instr::CallSample { dst, srcs, instance } => {
    // Gather inputs into a fixed stack buffer (no allocation). MVP: ≤ 4 inputs.
    let mut buf = [T::ZERO; 4];
    let k = srcs.len().min(4);
    for (j, &s) in srcs.iter().take(4).enumerate() {
        buf[j] = prog.block_regs[s][i];
    }
    let y = prog.sample_builtins[instance].process_sample(&buf[..k]);
    prog.block_regs[dst][i] = y;
}
Instr::CallBlock { .. } => unreachable!("block builtin scheduled into a sample region"),
```

  - In `exec_block_op`'s `match`, add `Instr::CallSample { .. } | Instr::CallBlock { .. } => unreachable!(...)` (they never appear as a plain `Block` step).
  - In the **reference** `eval_sample_scalar`, add handling so the oracle supports
    both built-ins (converting f64↔T):

```rust
Instr::CallSample { dst, srcs, instance } => {
    let mut buf = [T::ZERO; 4];
    let k = srcs.len().min(4);
    for (j, &s) in srcs.iter().take(4).enumerate() {
        buf[j] = T::from_f64(prog.regs_scalar[s]);
    }
    prog.regs_scalar[dst] = prog.sample_builtins[instance].process_sample(&buf[..k]).to_f64();
}
Instr::CallBlock { dst, src, instance } => {
    let x = T::from_f64(prog.regs_scalar[src]);
    let mut o = [T::ZERO; 1];
    let _ = prog.block_builtins[instance].process(Some(&[x]), &mut o);
    prog.regs_scalar[dst] = o[0].to_f64();
}
```

### D4c — RillProgram + compile_with

- [ ] **Step 4:** In `program.rs`:
  - Add fields `sample_builtins: Vec<Box<dyn rill_core::traits::Algorithm<T>>>`? No —
    `sample_builtins: Vec<Box<dyn crate::builtin::SampleBuiltin<T>>>` and
    `block_builtins: Vec<Box<dyn rill_core::traits::Algorithm<T>>>`.
  - Keep `new(ir)` building **zero** built-ins (`ir.builtins` must be empty for the
    empty-registry path; if non-empty, that's a bug — but `new` is only used by
    `compile()` which never produces built-ins). Add:

```rust
pub(crate) fn new_with(
    ir: Ir,
    registry: &crate::builtin::Registry<T>,
    sample_rate: f32,
) -> Result<Self, crate::error::CompileError> {
    let mut sample_builtins = Vec::new();
    let mut block_builtins = Vec::new();
    // Assign each ir.builtins entry a runtime instance; CallSample/CallBlock
    // `instance` indexes ir.builtins, but instances split by kind — so we keep a
    // parallel map from ir.builtins index → (kind-local index). To keep the IR
    // `instance` field pointing at the right vec, build a redirect table.
    // SIMPLEST: store BOTH vecs and have exec use a per-ir.builtins index that we
    // remap here into the right vec. To avoid a remap, store instances in ONE
    // Vec of an enum. See note below.
    // ...
    Ok(/* ... */)
}
```

> **Instance indexing decision (do this):** to avoid a redirect table, make the IR
> `instance` index a **single** `builtins` runtime vec of an enum:
> ```rust
> pub(crate) enum BuiltinInst<T: Transcendental> {
>     Sample(Box<dyn crate::builtin::SampleBuiltin<T>>),
>     Block(Box<dyn rill_core::traits::Algorithm<T>>),
> }
> ```
> `RillProgram` holds `builtins: Vec<BuiltinInst<T>>` indexed directly by the IR
> `instance` field (which equals the `ir.builtins` index). The executor matches:
> `CallSample` expects `BuiltinInst::Sample`, `CallBlock` expects
> `BuiltinInst::Block`. Update the executor snippets in Step 3 accordingly
> (`match &mut prog.builtins[instance] { BuiltinInst::Sample(b) => …, _ => unreachable!() }`).

  - `new_with` builds `builtins: Vec<BuiltinInst<T>>` by iterating `ir.builtins`:
    for each, `registry.get(name)` → `build_sample`/`build_block` with params +
    sample_rate; call `init(sample_rate)`; wrap in `BuiltinInst`. Missing name →
    `CompileError::Unsupported(format!("unknown built-in '{name}'"))`.
  - `reset()`: also reset every `BuiltinInst` (`Sample(b)=>b.reset()`, `Block(b)=>b.reset()`).
  - Add `pub fn init(&mut self, sample_rate: f32)` forwarding to all `BuiltinInst`.

- [ ] **Step 5:** In `lib.rs`, add:

```rust
use crate::builtin::Registry;

/// Compile with a built-in registry and a sample rate.
pub fn compile_with<T: Transcendental>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<RillProgram<T>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens)?;
    let typed = types::infer::infer_program_with(&program, registry)?;
    let ir = lower::lower_with(&typed, registry)?;
    // Feedback-legality: no CallBlock may appear inside a Sample region.
    validate_block_builtins(&ir)?;
    RillProgram::<T>::new_with(ir, registry, sample_rate)
}

fn validate_block_builtins(ir: &crate::ir::Ir) -> Result<(), CompileError> {
    use crate::ir::Instr;
    use crate::schedule::{build_schedule, Step};
    let sched = build_schedule(ir);
    for step in &sched.steps {
        if let Step::Sample(instrs) = step {
            for &idx in instrs {
                if matches!(ir.instrs[idx], Instr::CallBlock { .. }) {
                    return Err(CompileError::Unsupported(
                        "block built-in cannot be used inside a feedback loop (`~`)".to_string(),
                    ));
                }
            }
        }
    }
    Ok(())
}
```

  Re-export `compile_with`, `builtin::{Registry, SampleBuiltin, BuiltinSig, BuiltinKind}`
  in the prelude.

- [ ] **Step 6:** Add executor/compile tests in `interp.rs` (`mod tests`) using a
  local registry with a real sample built-in (a leaky one-pole implemented inline
  as a `SampleBuiltin`) and a real block built-in (a simple gain `Algorithm`), and
  assert: sample built-in in feedback runs and matches a hand-rolled recurrence;
  block built-in runs; `compile_with("process = + ~ myblock();", …)` returns
  `Err` (block-in-feedback). Also assert hybrid == reference for a sample-built-in
  program.

- [ ] **Step 7:** Run `cargo test -p rill-lang` (all), `cargo clippy -p rill-lang --all-targets`
  (zero warnings; add `#[allow(clippy::needless_range_loop)]` where the sample loop
  needs it), `cargo fmt`.

- [ ] **Step 8: Commit**

```bash
git add rill-lang/src/schedule.rs rill-lang/src/backend/interp.rs rill-lang/src/program.rs rill-lang/src/lib.rs
git commit -m 'feat(rill-lang): schedule/exec built-ins (CallSample sample-region, CallBlock ForeignBlock) + compile_with'
```

---

## Task D5: `rill-adrift` bindings + integration tests

**Files:** Create `rill-adrift/src/lang_builtins.rs`; modify `rill-adrift/src/lib.rs`; create `rill-adrift/tests/lang_builtins.rs`.

- [ ] **Step 1:** Create `rill-adrift/src/lang_builtins.rs`. Read the real
  constructors first: `rill_core_dsp::filters::{Biquad, OnePole, MoogLadder, FilterParams, FilterType}`
  (`OnePole::new(FilterParams)` + `process_sample`; `MoogLadder::new(cutoff, resonance)`
  + `process_sample` + `set_cutoff`/`set_resonance`; `Biquad::new(FilterParams)` impl
  `Algorithm`); confirm `Biquad`/`OnePole` `init(sample_rate)`. Then:

```rust
//! rill-lang built-in bindings for rill-core-dsp / rill-core-model blocks.

use rill_core::math::Transcendental;
use rill_core::traits::Algorithm;
use rill_lang::builtin::{BuiltinKind, BuiltinSig, Registry, SampleBuiltin};

// --- sample built-ins ---

struct OnePoleBuiltin<T: Transcendental> {
    inner: rill_core_dsp::filters::OnePole<T>,
}
impl<T: Transcendental> SampleBuiltin<T> for OnePoleBuiltin<T> {
    fn process_sample(&mut self, inputs: &[T]) -> T {
        self.inner.process_sample(inputs[0])
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.inner, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
}

struct MoogBuiltin<T: Transcendental> {
    inner: rill_core_dsp::filters::MoogLadder<T>,
}
impl<T: Transcendental> SampleBuiltin<T> for MoogBuiltin<T> {
    fn process_sample(&mut self, inputs: &[T]) -> T {
        self.inner.process_sample(inputs[0])
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.inner, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
}

/// Register the always-available rill-core-dsp built-ins.
pub fn register_dsp_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    use rill_core_dsp::filters::{Biquad, FilterParams, FilterType, OnePole};

    reg.register_sample(
        BuiltinSig { name: "onepole", signal_ins: 1, signal_outs: 1, num_params: 2, kind: BuiltinKind::Sample },
        |p, sr| {
            let mut inner = OnePole::<T>::new(FilterParams {
                filter_type: FilterType::LowPass, cutoff: p[0] as f32, q: p[1] as f32, gain_db: 0.0,
            });
            Algorithm::init(&mut inner, sr);
            Box::new(OnePoleBuiltin { inner })
        },
    );
    reg.register_sample(
        BuiltinSig { name: "moog", signal_ins: 1, signal_outs: 1, num_params: 2, kind: BuiltinKind::Sample },
        |p, sr| {
            let mut inner = rill_core_dsp::filters::MoogLadder::<T>::new(p[0] as f32, p[1] as f32);
            Algorithm::init(&mut inner, sr);
            Box::new(MoogBuiltin { inner })
        },
    );
    reg.register_block(
        BuiltinSig { name: "lowpass", signal_ins: 1, signal_outs: 1, num_params: 2, kind: BuiltinKind::Block },
        |p, sr| {
            let mut b = Biquad::<T>::new(FilterParams {
                filter_type: FilterType::LowPass, cutoff: p[0] as f32, q: p[1] as f32, gain_db: 0.0,
            });
            Algorithm::init(&mut b, sr);
            Box::new(b)
        },
    );
    reg.register_block(
        BuiltinSig { name: "highpass", signal_ins: 1, signal_outs: 1, num_params: 2, kind: BuiltinKind::Block },
        |p, sr| {
            let mut b = Biquad::<T>::new(FilterParams {
                filter_type: FilterType::HighPass, cutoff: p[0] as f32, q: p[1] as f32, gain_db: 0.0,
            });
            Algorithm::init(&mut b, sr);
            Box::new(b)
        },
    );
}

/// Register rill-core-model / analog built-ins.
#[cfg(feature = "analog")]
pub fn register_model_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    use rill_analog_filters::WdfMoogLadderProcessor;
    reg.register_block(
        BuiltinSig { name: "analog_moog", signal_ins: 1, signal_outs: 1, num_params: 2, kind: BuiltinKind::Block },
        |p, sr| {
            let mut f = WdfMoogLadderProcessor::<T, 1>::new(sr);
            f.cutoff = p[0] as f32;
            f.resonance = p[1] as f32;
            Algorithm::init(&mut f, sr);
            Box::new(f)
        },
    );
}

/// A registry populated with all available built-ins.
pub fn full_registry<T: Transcendental>() -> Registry<T> {
    let mut reg = Registry::new();
    register_dsp_builtins(&mut reg);
    #[cfg(feature = "analog")]
    register_model_builtins(&mut reg);
    reg
}
```

> Verify each constructor/field against source (`WdfMoogLadderProcessor` `new`,
> public `cutoff`/`resonance` fields — see `rill-adrift/src/registration.rs`
> `register_analog`, which uses exactly these). `WdfMoogLadderProcessor<T, BUF_SIZE>`
> is generic over `BUF_SIZE`; `Algorithm::process` is buffer-length-agnostic, so
> `<T, 1>` works for arbitrary block lengths — CONFIRM by reading the type; if it
> hard-codes internal `BUF_SIZE` arrays, pick a suitable size like `512` and note it.

- [ ] **Step 2:** In `rill-adrift/src/lib.rs`, under the `lang` feature:

```rust
#[cfg(feature = "lang")]
pub mod lang_builtins;
```

  Ensure `rill-lang` exposes `pub mod builtin;` (Task D1) so `rill_lang::builtin::*`
  is reachable from adrift.

- [ ] **Step 3:** Create `rill-adrift/tests/lang_builtins.rs`:

```rust
#![cfg(feature = "lang")]

use rill_adrift::lang_builtins::full_registry;
use rill_core::traits::Algorithm;
use rill_lang::compile_with;

fn run(src: &str, input: &[f32], sr: f32) -> Vec<f32> {
    let reg = full_registry::<f32>();
    let mut prog = compile_with::<f32>(src, &reg, sr).unwrap();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(input), &mut out).unwrap();
    out
}

#[test]
fn onepole_sample_builtin_smooths() {
    // A one-pole lowpass should attenuate a fast alternating signal.
    let input: Vec<f32> = (0..64).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
    let out = run("process = _ : onepole(200.0, 0.7);", &input, 48_000.0);
    // Output energy should be well below input energy (1.0 per sample).
    let e: f32 = out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32;
    assert!(e < 0.9, "onepole did not attenuate (energy {e})");
}

#[test]
fn lowpass_block_matches_direct_biquad() {
    use rill_core_dsp::filters::{Biquad, FilterParams, FilterType};
    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.3).sin()).collect();

    let via_lang = run("process = _ : lowpass(1000.0, 0.7);", &input, 48_000.0);

    let mut b = Biquad::<f32>::new(FilterParams {
        filter_type: FilterType::LowPass, cutoff: 1000.0, q: 0.7, gain_db: 0.0,
    });
    Algorithm::init(&mut b, 48_000.0);
    let mut direct = vec![0.0f32; input.len()];
    b.process(Some(&input), &mut direct).unwrap();

    for (x, y) in via_lang.iter().zip(direct.iter()) {
        assert!((x - y).abs() < 1e-5, "lang {x} vs direct {y}");
    }
}

#[test]
fn sample_builtin_composes_in_feedback() {
    // A sample built-in is feedback-legal.
    let reg = full_registry::<f32>();
    assert!(compile_with::<f32>("process = + ~ onepole(500.0, 0.5);", &reg, 48_000.0).is_ok());
}

#[test]
fn block_builtin_in_feedback_is_rejected() {
    let reg = full_registry::<f32>();
    let err = compile_with::<f32>("process = + ~ lowpass(500.0, 0.7);", &reg, 48_000.0);
    assert!(err.is_err());
}
```

- [ ] **Step 4:** Run:
  `cargo test -p rill-adrift --features lang` → pass.
  `cargo test -p rill-adrift --features "lang analog"` → pass (adds `analog_moog`; add a quick test if desired).
  `cargo clippy -p rill-adrift --features "lang analog" --all-targets` → clean.
  `cargo fmt`.

- [ ] **Step 5: Commit**

```bash
git add rill-adrift/src/lang_builtins.rs rill-adrift/src/lib.rs rill-adrift/tests/lang_builtins.rs
git commit -m 'feat(rill-adrift): rill-lang DSP/model built-in bindings (onepole/moog/lowpass/highpass/analog_moog)'
```

---

## Task D6: Docs + changelog + full verification

**Files:** Modify `rill-lang/README.md`, `docs/src/guides/rill-lang-language.md`, `CHANGELOG.md`.

- [ ] **Step 1:** Add a "Built-in functions" section to the language guide: the
  calling convention (`_ : lowpass(1000.0, 0.7)`), that params are constants,
  sample vs block built-ins, the feedback rule (block built-ins can't be inside
  `~`), and that bindings come from `rill-adrift` (`full_registry`, `compile_with`).
  Add a short note to `README.md`.

- [ ] **Step 2:** `CHANGELOG.md` — add under the rill-lang section:

```markdown
- **DSP/model built-ins (FFI registry).** rill-lang programs can call stateful
  built-ins from `rill-core-dsp`/`rill-core-model` via `compile_with(src,
  &registry, sample_rate)`: per-sample built-ins (`onepole`, `moog` — feedback-
  legal) and whole-buffer built-ins (`lowpass`, `highpass`, `analog_moog`). Params
  are constants (`_ : lowpass(1000.0, 0.7)`); signals flow via combinators.
  Bindings live in `rill-adrift` (`lang_builtins::full_registry`, `analog_moog`
  behind the `analog` feature). rill-lang core stays `rill-core`-only.
```

- [ ] **Step 3:** Full verification:
  `cargo fmt --check`; `cargo clippy --workspace --all-targets`;
  `cargo test --workspace`; `cargo test -p rill-lang --features serde`;
  `cargo test -p rill-adrift --features "lang analog"`; `mdbook build docs/`
  (confirm the guide page renders, not a stub).

- [ ] **Step 4: Commit**

```bash
git add rill-lang/README.md docs/src/guides/rill-lang-language.md CHANGELOG.md
git commit -m 'docs(rill-lang): document DSP/model built-ins + changelog'
```

---

## Self-Review checklist (completed while writing)

- **Spec coverage:** registry + `SampleBuiltin` → D1; IR `CallSample`/`CallBlock` +
  `builtins` table → D2; inference/lowering resolution + const params → D3;
  scheduler `ForeignBlock` + executor + `RillProgram` instances + `compile_with` +
  feedback-legality → D4; adrift bindings (dsp always, model under `analog`) +
  correctness tests (direct-Algorithm compare, feedback rejection) → D5; docs → D6.
  Deferred items (param modulation, n→m/multi-output block built-ins, node wiring)
  match the design non-goals.
- **Placeholder scan:** none — full code for `builtin.rs`, IR additions, executor
  functions, adrift bindings, and all tests; existing-file changes are precise
  insertions with exact code.
- **Type consistency:** `SampleBuiltin`/`BuiltinSig`/`BuiltinKind`/`Registry`/
  `SignatureSource`/`NoSigs`, `Instr::CallSample{dst,srcs,instance}`/`CallBlock{dst,src,instance}`,
  `BuiltinInstance{name,params,kind}`, `Ir.builtins`, `Step::ForeignBlock`,
  `BuiltinInst<T>`, `infer_program_with`/`lower_with`/`new_with`/`compile_with`,
  `register_dsp_builtins`/`register_model_builtins`/`full_registry` are used
  consistently across tasks.

## Verification notes for the implementer

- Confirm exact constructor signatures before D5: `OnePole::new(FilterParams)`,
  `MoogLadder::new(cutoff: f32, resonance: f32)`, `Biquad::new(FilterParams)`,
  `WdfMoogLadderProcessor::<T, N>::new(sample_rate)` with public `cutoff`/`resonance`
  (mirror `rill-adrift/src/registration.rs`). Confirm `FilterParams { filter_type,
  cutoff, q, gain_db }` and `FilterType::{LowPass, HighPass}`.
- `WdfMoogLadderProcessor<T, BUF_SIZE>` — verify a fixed `BUF_SIZE` generic works
  for arbitrary process block lengths (its `Algorithm::process` iterates the given
  slices). If it internally assumes `BUF_SIZE`-sized arrays, pick `512` and note it.
- The `BuiltinInst<T>` single-vec indexing (D4c) keeps the IR `instance` field a
  direct index — do not split into two vecs.
```

