# rill-lang Named Parameters + Smoothing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Add RT-safe named parameters to rill-lang: a `param("name", default[, min, max])` primitive backed by mutable slots, dynamic parameterization of imported built-ins, a native `smooth(x, ms)` primitive, and `LangNode` integration so servos/LFO/MIDI automation drive rill-lang params.

**Architecture:** `param()` lowers to `Instr::ReadParam { dst, idx }` reading a `RillProgram.params: Vec<f64>` slot (combinational block op). Built-in args may be a `param(...)` → recorded as `(arg_pos → param_idx)` bindings pushed each block via a `set_param` hook (`SampleBuiltin::set_param`, new `BlockBuiltin<T>: Algorithm<T>`). `smooth(x, ms)` lowers to a native one-pole recurrence (`ReadState`/`WriteState` + a coefficient baked from `ms` + sample rate). Parameter writes arrive via the graph drain on the I/O thread just before `process()`, so a plain `Vec<f64>` is race-free.

**Tech Stack:** Rust 2021, `max_width=100`, `#![deny(unsafe_code)]`. rill-lang core: no new deps.

**Design:** `docs/superpowers/specs/2026-07-06-rill-lang-named-parameters-design.md`.
**Branch:** `feature/rill-lang`.

**Locked decisions:**
- **String literals** are added (`Tok::Str`, `Expr::Str`). `param` and `smooth` are **reserved forms** handled in inference/lowering (not the registry).
- `param(name, default)` or `param(name, default, min, max)` — name is a string literal, the rest constant floats. Repeated `param("x", …)` references **share one slot** (dedup by name; first occurrence's default/range wins).
- `param()`/`ReadParam` is **combinational** (block op). Built-in param pushes and `smooth` state are the only new stateful behaviors.
- Built-in args are **const or a single `param(...)`** (no composed expressions on built-in args).
- `smooth` bakes the sample rate → `lower_with(tp, sigs, sample_rate)`; SR change requires recompile (documented).
- Param writes are same-thread as `process()` (graph drain) → plain `Vec<f64>`, no atomics.

---

## File Structure

```
rill-lang/src/
  lexer.rs, ast.rs, parser.rs   # string literals
  ir.rs                          # ReadParam, ParamDef, Ir.params; BuiltinInstance.param_bindings
  types/infer.rs                 # param/smooth forms; Expr::Str
  lower.rs                       # ReadParam emit + dedup; smooth expansion; builtin param bindings; sample_rate arg
  schedule.rs                    # ReadParam (combinational)
  backend/interp.rs              # ReadParam exec; per-block builtin param push
  program.rs                     # params + set_param/param_index/params_meta; ParamDef; BlockBuiltin box
  builtin.rs                     # SampleBuiltin::set_param; BlockBuiltin<T> trait
  lib.rs                         # compile_with passes sample_rate to lower
rill-adrift/src/
  lang_builtins.rs               # set_param impls; BlockBuiltin wrappers
  lang_node.rs                   # advertise params; set_parameter routing; from_source_with(registry, sr)
  registration.rs                # rill/lang node builds with full_registry + sample_rate
```

---

## Task P1: String literals

**Files:** `rill-lang/src/lexer.rs`, `ast.rs`, `parser.rs` (+ tests in each).

- [ ] **Step 1 (lexer):** Add `Str(String)` to `Tok`. In `tokenize`, before the single-char section, handle `"`: collect bytes until the closing `"` (no escapes in MVP); unterminated → `CompileError::Lex`. Push `Tok::Str(text)` with the span covering the quotes.

```rust
// inside the while loop, after the number branch:
if c == b'"' {
    let mut j = i + 1;
    while j < bytes.len() && bytes[j] != b'"' {
        j += 1;
    }
    if j >= bytes.len() {
        return Err(CompileError::Lex { msg: "unterminated string literal".into(), span: Span::new(start, bytes.len()) });
    }
    let text = src[i + 1..j].to_string();
    i = j + 1;
    out.push(Token { tok: Tok::Str(text), span: Span::new(start, i) });
    continue;
}
```

Add a lexer test: `tokenize(r#""cutoff""#)` yields `[Str("cutoff"), Eof]`; unterminated errors.

- [ ] **Step 2 (ast):** Add `Str(String, Span)` to `Expr`; add it to the `span()` match arm (`Expr::Str(_, s) => *s`).

- [ ] **Step 3 (parser):** In `parse_atom`, add `Tok::Str(s) => Ok(Expr::Str(s, t.span))`. Add a parser test: `f("x")` parses to an `Apply` whose `args[0]` is `Expr::Str("x", _)`.

- [ ] **Step 4:** `cargo test -p rill-lang lexer parser` → pass; `cargo test -p rill-lang` → all pass; `cargo clippy -p rill-lang --all-targets` → clean; `cargo fmt`.

- [ ] **Step 5: Commit**
```bash
git add rill-lang/src/lexer.rs rill-lang/src/ast.rs rill-lang/src/parser.rs
git commit -m 'feat(rill-lang): string literals (Tok::Str, Expr::Str)'
```

---

## Task P2: `param()` primitive (expression values, end-to-end)

**Files:** `rill-lang/src/ir.rs`, `types/infer.rs`, `lower.rs`, `schedule.rs`, `backend/interp.rs`, `program.rs`. Tests in `lower.rs`/`interp.rs`.

- [ ] **Step 1 (ir.rs):** Add:
```rust
/// A named runtime parameter definition (a mutable control slot).
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    /// Parameter name.
    pub name: String,
    /// Initial/default value.
    pub default: f64,
    /// Minimum (clamp lower bound).
    pub min: f64,
    /// Maximum (clamp upper bound).
    pub max: f64,
}
```
Add `Instr::ReadParam { dst: Reg, idx: usize }` (doc: "read a named parameter slot; constant within a block"). Add `pub params: Vec<ParamDef>` to `Ir`. Update the `Ir { .. }` literal in `lower.rs` to include `params: Vec::new()` (P2 populates it).

- [ ] **Step 2 (infer.rs):** In `infer_apply`, BEFORE the registry/user-def logic, reserve `param`:
```rust
if name == "param" {
    // param("name", default) or param("name", default, min, max)
    if args.len() != 2 && args.len() != 4 {
        return Err(CompileError::Type { msg: "param expects (name, default[, min, max])".into(), span });
    }
    if !matches!(args[0], Expr::Str(_, _)) {
        return Err(CompileError::Type { msg: "param name must be a string literal".into(), span: args[0].span() });
    }
    for a in &args[1..] {
        let at = infer_expr(ctx, a)?;
        if at.arity_in() != 0 || at.arity_out() != 1 {
            return Err(CompileError::Type { msg: "param default/min/max must be constants".into(), span: a.span() });
        }
    }
    return Ok(Type { ins: vec![], outs: vec![Scalar::Float] });
}
```
Add an `Expr::Str` arm to `infer_expr` returning an error ("string literal is only valid as a `param` name") — it should never be inferred except as `param`'s first arg (handled above without recursing into it).

- [ ] **Step 3 (lower.rs):** Give `Lowerer` a `params: Vec<ParamDef>` and a `param_names: std::collections::HashMap<String, usize>` (name→idx dedup). Add a helper `const_f64` if not already present (D3 added it). In `Lowerer::lower`, handle `param` in the `Expr::Apply` arm BEFORE built-ins:
```rust
if name == "param" {
    let pname = match &args[0] { Expr::Str(s, _) => s.clone(), _ => unreachable!("checked in infer") };
    let default = const_f64(&args[1]).unwrap_or(0.0);
    let (min, max) = if args.len() == 4 {
        (const_f64(&args[2]).unwrap_or(f64::NEG_INFINITY), const_f64(&args[3]).unwrap_or(f64::INFINITY))
    } else {
        (f64::NEG_INFINITY, f64::INFINITY)
    };
    let idx = *self.param_names.entry(pname.clone()).or_insert_with(|| {
        let i = self.params.len();
        self.params.push(ParamDef { name: pname.clone(), default, min, max });
        i
    });
    let dst = self.fresh_reg();
    self.emit(Instr::ReadParam { dst, idx });
    return Ok(vec![dst]);
}
```
Set `params: lw.params` in the returned `Ir`.

- [ ] **Step 4 (schedule.rs):** In `instr_dst`, add `Instr::ReadParam { dst, .. } => Some(dst)`. `instr_srcs` returns empty for it (no arm needed if the fallthrough is `_ => Vec::new()`; otherwise add). `is_stateful` stays false for it. (Result: ReadParam is a combinational singleton → Block op.) Handle any non-exhaustive `match Instr` compile errors.

- [ ] **Step 5 (program.rs):** Add `pub(crate) params: Vec<f64>` and `pub(crate) params_meta: Vec<crate::ir::ParamDef>` to `RillProgram`. In `new`/`new_with`, initialise: `params_meta = ir.params.clone()`, `params = ir.params.iter().map(|p| p.default).collect()`. Add:
```rust
/// Index of a named parameter, if present.
pub fn param_index(&self, name: &str) -> Option<usize> {
    self.params_meta.iter().position(|p| p.name == name)
}
/// Set a parameter by index (clamped to its range). RT-safe (plain store).
pub fn set_param(&mut self, idx: usize, value: f64) {
    if let Some(def) = self.params_meta.get(idx) {
        self.params[idx] = value.clamp(def.min, def.max);
    }
}
/// Current value of a parameter by index.
pub fn param(&self, idx: usize) -> f64 {
    self.params.get(idx).copied().unwrap_or(0.0)
}
/// Metadata for all parameters (name, default, range).
pub fn params_meta(&self) -> &[crate::ir::ParamDef] {
    &self.params_meta
}
```

- [ ] **Step 6 (interp.rs):** Add `ReadParam` handling:
  - `exec_block_op`: `Instr::ReadParam { dst, idx } => { let v = T::from_f64(prog.params[idx]); prog.block_regs[dst][..n].fill(v); }`
  - `exec_sample_region`: `Instr::ReadParam { dst, idx } => prog.block_regs[dst][i] = T::from_f64(prog.params[idx])`
  - `eval_sample_scalar` (reference): `Instr::ReadParam { dst, idx } => prog.regs_scalar[dst] = prog.params[idx]`

- [ ] **Step 7 (tests):** In `interp.rs` `mod tests` add: `param_default_applies` (`process = _ * param("g", 0.5);` halves), `set_param_changes_output` (build, `prog.set_param(prog.param_index("g").unwrap(), 2.0)`, then process doubles), `param_range_clamps` (`param("g",0.5,0.0,1.0)`, set 5.0 → clamps to 1.0), `param_shared_slot` (`process = param("k",2.0) * param("k",2.0);` yields one param, output 4.0 for input arity 0→1 — note this is a (0→1) program; adjust to a valid form, e.g. `process = _ * param("k",2.0) + param("k",2.0);` and assert one entry in `params_meta`).

- [ ] **Step 8:** `cargo test -p rill-lang` → all pass; clippy clean; fmt.

- [ ] **Step 9: Commit**
```bash
git add rill-lang/src/ir.rs rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/src/schedule.rs rill-lang/src/program.rs rill-lang/src/backend/interp.rs
git commit -m 'feat(rill-lang): param() named parameters (ReadParam slot, set/get, range clamp)'
```

---

## Task P3: `smooth(x, ms)` + sample rate in lowering

**Files:** `rill-lang/src/types/infer.rs`, `lower.rs`, `lib.rs`. Tests in `lower.rs`/integration.

- [ ] **Step 1 (lower signature):** Change `lower_with(tp, sigs)` → `lower_with(tp, sigs, sample_rate: f32)`; `lower(tp)` delegates with `44_100.0`. In `lib.rs`, `compile` calls `lower(&typed)` (default SR) and `compile_with` calls `lower_with(&typed, registry, sample_rate)`. Give `Lowerer` a `sample_rate: f32` field.

- [ ] **Step 2 (infer):** In `infer_apply`, reserve `smooth` (after `param`, before registry):
```rust
if name == "smooth" {
    if args.len() != 2 {
        return Err(CompileError::Type { msg: "smooth expects (signal, ms)".into(), span });
    }
    let x = infer_expr(ctx, &args[0])?;      // a signal sub-expression
    if x.arity_out() != 1 {
        return Err(CompileError::Type { msg: "smooth's first argument must produce one signal".into(), span: args[0].span() });
    }
    let ms = infer_expr(ctx, &args[1])?;
    if ms.arity_in() != 0 || ms.arity_out() != 1 {
        return Err(CompileError::Type { msg: "smooth time (ms) must be a constant".into(), span: args[1].span() });
    }
    return Ok(Type { ins: x.ins.clone(), outs: vec![Scalar::Float] });
}
```

- [ ] **Step 3 (lower):** In `Lowerer::lower`'s `Expr::Apply` arm, handle `smooth` (after `param`, before built-ins). Lower `args[0]` to a signal reg; fold `ms`; compute the one-pole coefficient; emit the recurrence using a state slot:
```rust
if name == "smooth" {
    let xregs = self.lower(&args[0], inputs)?;
    let x = xregs[0];
    let ms = const_f64(&args[1]).ok_or_else(|| CompileError::Type {
        msg: "smooth time must be a constant".into(), span: args[1].span(),
    })?;
    let sr = self.sample_rate as f64;
    let a = if ms <= 0.0 { 1.0 } else {
        let tau = ms / 1000.0;
        1.0 - (-1.0 / (tau * sr)).exp()
    };
    // y = prev + a*(x - prev);  state <- y
    let slot = self.state_slots;
    self.state_slots += 1;
    let prev = self.fresh_reg();
    self.emit(Instr::ReadState { dst: prev, slot });
    let diff = self.fresh_reg();
    self.emit(Instr::Bin { dst: diff, op: BinArith::Sub, a: x, b: prev });
    let acoef = self.fresh_reg();
    self.emit(Instr::Const { dst: acoef, value: a });
    let scaled = self.fresh_reg();
    self.emit(Instr::Bin { dst: scaled, op: BinArith::Mul, a: acoef, b: diff });
    let y = self.fresh_reg();
    self.emit(Instr::Bin { dst: y, op: BinArith::Add, a: prev, b: scaled });
    self.emit(Instr::WriteState { slot, src: y });
    return Ok(vec![y]);
}
```
(`self.state_slots` is the same counter feedback lowering uses; confirm its name.)

- [ ] **Step 4 (tests):**
  - `lower`: `smooth(_, 10.0)` allocates one state slot and emits Read/WriteState.
  - integration (`tests/`): `smooth_step_response` — compile `process = smooth(param("t", 0.0), 5.0);` with `compile_with(..., 48000.0)`; set param `t` to 1.0; process a block; assert output ramps from ~0 toward 1 monotonically and is < 1 at the first sample (smoothing) and closer to 1 later. Also `smooth` hybrid vs `process_reference` equivalence.

- [ ] **Step 5:** `cargo test -p rill-lang` (+ `--features serde` if touched) → pass; clippy clean; fmt.

- [ ] **Step 6: Commit**
```bash
git add rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/src/lib.rs
git commit -m 'feat(rill-lang): smooth(x, ms) one-pole primitive (native IR); lower takes sample_rate'
```

---

## Task P4: Dynamic built-in parameters

**Files:** `rill-lang/src/builtin.rs`, `ir.rs`, `lower.rs`, `program.rs`, `backend/interp.rs`. Tests in `interp.rs`.

- [ ] **Step 1 (builtin.rs):** Add `fn set_param(&mut self, _index: usize, _value: T) {}` to `SampleBuiltin`. Add:
```rust
/// A whole-buffer built-in with settable params.
pub trait BlockBuiltin<T: Transcendental>: rill_core::traits::Algorithm<T> {
    /// Set a parameter by index (default no-op).
    fn set_param(&mut self, _index: usize, _value: T) {}
}
```
Change the block factory type + `register_block` + `Entry::build_block` to produce `Box<dyn BlockBuiltin<T>>` instead of `Box<dyn Algorithm<T>>`.

- [ ] **Step 2 (ir.rs):** Add `pub param_bindings: Vec<(usize, usize)>` to `BuiltinInstance` (doc: "(arg_position, param_idx) — dynamic param drivers"). Update its constructions in `lower.rs`.

- [ ] **Step 3 (lower.rs):** In the built-in branch of `Lowerer::lower`, replace the const-only param folding with const-or-param handling:
```rust
let mut params = Vec::with_capacity(args.len());
let mut param_bindings = Vec::new();
for (pos, a) in args.iter().enumerate() {
    if let Expr::Apply { name: pn, args: pargs, .. } = a {
        if pn == "param" {
            // dynamic: use default as initial value, bind slot
            let pname = match &pargs[0] { Expr::Str(s, _) => s.clone(), _ => return Err(/* type err */) };
            let default = const_f64(&pargs[1]).unwrap_or(0.0);
            let (min, max) = if pargs.len() == 4 {
                (const_f64(&pargs[2]).unwrap_or(f64::NEG_INFINITY), const_f64(&pargs[3]).unwrap_or(f64::INFINITY))
            } else { (f64::NEG_INFINITY, f64::INFINITY) };
            let idx = *self.param_names.entry(pname.clone()).or_insert_with(|| {
                let i = self.params.len();
                self.params.push(ParamDef { name: pname.clone(), default, min, max });
                i
            });
            params.push(default);
            param_bindings.push((pos, idx));
            continue;
        }
    }
    let v = const_f64(a).ok_or_else(|| CompileError::Type {
        msg: format!("param to `{name}` must be a constant or a param(...)"), span: a.span(),
    })?;
    params.push(v);
}
let instance = self.builtins.len();
self.builtins.push(BuiltinInstance { name: name.clone(), params, kind: sig.kind, param_bindings });
```
(The `infer` side already accepts a `param(...)` arg because the built-in branch there type-checks each arg as arity 0→1, which `param` satisfies. Verify: `param("cutoff", 1000.0)` infers as (0→1) — yes. No infer change needed for dynamic args.)

- [ ] **Step 4 (program.rs):** Change `BuiltinInst::Block` to hold `Box<dyn crate::builtin::BlockBuiltin<T>>`. In `new_with`, `build_block` now yields `BlockBuiltin`. Add a per-block push method:
```rust
pub(crate) fn apply_builtin_params<T2>(&mut self) { /* see interp */ }
```
Actually implement the push in interp (needs T). Keep the data on `RillProgram` (`ir.builtins[*].param_bindings`, `params`, `builtins`).

- [ ] **Step 5 (interp.rs):** Add a once-per-block pass at the START of both `run_block_hybrid` and `run_block_reference` (before executing steps/samples):
```rust
fn push_builtin_params<T: Transcendental>(prog: &mut RillProgram<T>) {
    for (instance, bi) in prog.ir.builtins.iter().enumerate().collect::<Vec<_>>() {
        // collect to avoid borrow conflict, or index by range
    }
}
```
Simplify to avoid the borrow issue by iterating indices:
```rust
fn push_builtin_params<T: Transcendental>(prog: &mut RillProgram<T>) {
    let count = prog.ir.builtins.len();
    for instance in 0..count {
        let bindings = prog.ir.builtins[instance].param_bindings.clone();
        for (arg_pos, param_idx) in bindings {
            let v = T::from_f64(prog.params[param_idx]);
            match &mut prog.builtins[instance] {
                crate::program::BuiltinInst::Sample(b) => b.set_param(arg_pos, v),
                crate::program::BuiltinInst::Block(b) => b.set_param(arg_pos, v),
            }
        }
    }
}
```
Call `push_builtin_params(prog)` first thing in `run_block_hybrid` and `run_block_reference`. (Cloning the small `Vec<(usize,usize)>` per block is acceptable for correctness; if clippy/RT-strictness objects, restructure to index without clone — but note param_bindings are tiny and this is control-path setup. Prefer: iterate with a split that avoids the clone if easy; otherwise keep and note.)

> **RT note:** the `.clone()` of `param_bindings` allocates per block. To stay
> allocation-free, instead read bindings by index without cloning: capture
> `let (arg_pos, param_idx) = prog.ir.builtins[instance].param_bindings[k];` inside
> a `for k in 0..len` loop (the tuple is `Copy`). Use THAT form, not the clone.

- [ ] **Step 6 (tests):** In `interp.rs` define a test `SampleBuiltin` whose output = `input[0] * self.k` with `set_param(0, v) => self.k = v`. Register it (`kind: Sample, num_params: 1`). Compile `process = _ : gain(param("g", 1.0));` with a registry; process with param default → identity; `set_param("g", 0.5)` → halves. Confirms dynamic push works and hybrid==reference.

- [ ] **Step 7:** `cargo test -p rill-lang` → all pass; clippy clean; fmt.

- [ ] **Step 8: Commit**
```bash
git add rill-lang/src/builtin.rs rill-lang/src/ir.rs rill-lang/src/lower.rs rill-lang/src/program.rs rill-lang/src/backend/interp.rs
git commit -m 'feat(rill-lang): dynamic built-in params (BlockBuiltin::set_param, per-block push, param bindings)'
```

---

## Task P5: `rill-adrift` bindings + `LangNode` integration

**Files:** `rill-adrift/src/lang_builtins.rs`, `lang_node.rs`, `registration.rs`. Tests in `rill-adrift/tests/`.

- [ ] **Step 1 (lang_builtins.rs):** Update built-ins for `set_param`.
  - `OnePoleBuiltin`/`MoogBuiltin` (`SampleBuiltin`): implement `set_param` — index 0 = cutoff, 1 = q/resonance → update the inner filter's coefficient (use its setter / rebuild `FilterParams` + `init`). For OnePole store the `FilterParams` so `set_param` can rebuild; for Moog use `set_cutoff`/`set_resonance`.
  - Block built-ins now must be `BlockBuiltin`. Wrap Biquad: `struct BiquadBuiltin<T> { inner: Biquad<T>, sr: f32, cutoff: f32, q: f32 }`, impl `Algorithm<T>` (delegate all methods to `inner`) + `BlockBuiltin<T>` (`set_param(0)=cutoff`, `set_param(1)=q` → rebuild `FilterParams` + `init`). Update `register_block` factories to return `Box::new(BiquadBuiltin{..})`. Same for `analog_moog` (`MoogLadder` wrapper with set cutoff/resonance).

- [ ] **Step 2 (lang_node.rs):** Make `LangNode` use the registry + sample rate and advertise params.
  - Add fields `registry: Option<std::sync::Arc<rill_lang::builtin::Registry<T>>>` and `sample_rate: f32`.
  - `from_source(source)` → keep (empty registry, 44_100.0 via `compile`).
  - Add `from_source_with(source, registry: std::sync::Arc<Registry<T>>, sample_rate: f32)` using `rill_lang::compile_with(source, &registry, sample_rate)`; store both.
  - `set_parameter`: keep `"source"` (recompile using the stored registry+sr if present, else `compile`). Add a fallback: if the name matches `self.program.param_index(name)`, call `self.program.set_param(idx, value.as_f32()? as f64)` and return Ok. Otherwise the existing "unknown parameter" error.
  - `get_parameter`: for a name that is a param, return `ParamValue::Float(self.program.param(idx) as f32)`; keep `"source"`.
  - `metadata()`: build `NodeMetadata` and set `.parameters` from `self.program.params_meta()`:
    ```rust
    let mut md = /* existing */;
    md.parameters = self.program.params_meta().iter().map(|p| {
        rill_core::traits::ParamMetadata::new(&p.name, rill_core::traits::ParamType::Float, rill_core::traits::ParamValue::Float(p.default as f32))
            .with_range(p.min as f32, p.max as f32, 0.0)
    }).collect();
    md
    ```
    (Confirm `ParamType::Float` path; guard non-finite min/max by substituting sensible bounds, e.g. skip `.with_range` when min/max are infinite.)

- [ ] **Step 3 (registration.rs):** The `rill/lang` ctor builds with the registry + sample rate:
```rust
#[cfg(feature = "lang")]
factory.register_fn("rill/lang", |id, params| {
    let source = params.get("source").and_then(|v| v.as_str()).unwrap_or("process = _;");
    let reg = std::sync::Arc::new(crate::lang_builtins::full_registry::<f32>());
    let mut n = crate::lang_node::LangNode::<f32, BUF_SIZE>::from_source_with(source, reg, params.sample_rate)
        .unwrap_or_else(|_| crate::lang_node::LangNode::identity());
    Node::set_id(&mut n, id);
    Node::init(&mut n, params.sample_rate);
    NodeVariant::Processor(Box::new(n))
});
```
(Check the existing `register_lang` fn from the DSP built-ins increment and update it. `full_registry` requires `analog` for `analog_moog` — that's already feature-gated inside it.)

- [ ] **Step 4 (tests):** `rill-adrift/tests/lang_params.rs` (`#![cfg(feature = "lang")]`):
  - `param_controls_gain`: `full_registry`, `compile_with("process = _ * param(\"g\", 1.0);", &reg, 48000.0)`; process → identity; `set_param(param_index("g"), 0.25)`; process → quartered.
  - `dynamic_cutoff_changes_filter`: `_ : lowpass(param("cutoff", 500.0), 0.7)`; process a bright signal at cutoff 500 vs after `set_param("cutoff", 5000.0)` → assert the high-cutoff output has more high-frequency energy (or simply differs measurably).
  - `lang_node_advertises_and_sets_params`: build a `LangNode::<f32,64>::from_source_with("process = _ * param(\"g\", 1.0);", Arc::new(full_registry()), 48000.0)`; assert `metadata().parameters` contains `"g"`; `set_parameter("g", Float(0.5))`; run a block via `Processor::process` (fill input port, call process, read output) → halved.

- [ ] **Step 5:** `cargo test -p rill-adrift --features "lang analog"` → pass; `cargo build -p rill-adrift` (default) → builds; clippy clean; fmt.

- [ ] **Step 6: Commit**
```bash
git add rill-adrift/src/lang_builtins.rs rill-adrift/src/lang_node.rs rill-adrift/src/registration.rs rill-adrift/tests/lang_params.rs
git commit -m 'feat(rill-adrift): lang node advertises/sets params; dynamic-param built-in bindings'
```

---

## Task P6: Docs + changelog + full verification

**Files:** `docs/src/guides/rill-lang-language.md`, `rill-lang/README.md`, `CHANGELOG.md`.

- [ ] **Step 1:** Add a "Parameters" section to the language guide: `param("name", default[, min, max])`, `smooth(x, ms)`, using `param(...)` as a built-in arg for dynamic control, how params are set (`RillProgram::set_param` / the `rill/lang` node's `set_parameter` / servo/LFO/MIDI automation by name), control-rate semantics (block boundaries), and the SR-recompile note for `smooth`. Add a short README note.

- [ ] **Step 2:** `CHANGELOG.md` bullet under rill-lang:
```markdown
- **Named parameters + smoothing.** `param("cutoff", 1000.0)` exposes RT-safe
  control-rate parameter slots (settable via `RillProgram::set_param` and, on the
  `rill/lang` graph node, by name — so servos/LFO/MIDI automate them for free);
  `smooth(x, ms)` is a native one-pole for zipper-free changes; built-in args may
  be `param(...)` for dynamic parameterization (`lowpass(param("cutoff"), 0.7)`).
```

- [ ] **Step 3:** Full verify: `cargo fmt --check`; `cargo clippy --workspace --all-targets`; `cargo test --workspace`; `cargo test -p rill-lang --features serde`; `cargo test -p rill-adrift --features "lang analog"`; `mdbook build docs/` (page not a stub).

- [ ] **Step 4: Commit**
```bash
git add rill-lang/README.md docs/src/guides/rill-lang-language.md CHANGELOG.md
git commit -m 'docs(rill-lang): document named parameters + smoothing'
```

---

## Self-Review checklist (completed while writing)

- **Spec coverage:** string literals → P1; `param()` slots + set/get/clamp → P2; `smooth` native IR + SR-in-lower → P3; dynamic built-in params (`BlockBuiltin::set_param`, bindings, per-block push) → P4; `LangNode` metadata/set_parameter + automation + dynamic bindings → P5; docs → P6. Non-goals (composed built-in args, audio-rate modulation, param persistence) match the design.
- **Placeholder scan:** none — code for lexer/AST/parser, IR types, `param`/`smooth` lowering, param methods, executor arms, `BlockBuiltin`, and tests provided; existing-file changes are precise insertions. The one flagged risk (per-block `param_bindings.clone()`) has an explicit allocation-free alternative called out.
- **Type consistency:** `Tok::Str`/`Expr::Str`, `Instr::ReadParam`/`ParamDef`/`Ir.params`, `param_index`/`set_param`/`param`/`params_meta`, `BuiltinInstance.param_bindings`, `BlockBuiltin<T>`/`set_param`, `lower_with(..., sample_rate)`, `from_source_with`, `push_builtin_params` are consistent across tasks.

## Verification notes for the implementer

- Confirm `rill_core::traits::ParamType::Float`, `ParamValue::Float`, and `ParamMetadata::new(...).with_range(min,max,step)` paths (`rill-core/src/traits/param.rs`). Guard infinite `min`/`max` in metadata (skip range or use finite fallbacks).
- Confirm the state-slot counter field name in `Lowerer` (used by feedback lowering) reused by `smooth`.
- Keep the `param_bindings` per-block push **allocation-free** (index the tuple, don't clone the Vec).
- `Biquad`/`OnePole`/`MoogLadder` coefficient setters: verify method names (`set_cutoff`/`set_resonance`, or rebuild `FilterParams` + `Algorithm::init`) before wiring `set_param`.
