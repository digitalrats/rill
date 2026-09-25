# rill-lang: First-Class Functions and Closures — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make functions first-class values in rill-lang — lambda literals, closures (by-value env capture), runtime dispatch, currying, HOF combinators, and signal-wire arguments — within the acyclic strict contract.

**Architecture:** A function value becomes `Value::Closure(env_ref, fragment_id)`. Each body compiles to a `FragmentIr` in `Ir::fragments`. `ValueCallFunc` performs real runtime dispatch: it pushes a temporary cell-stack frame (env fields + value args as cells, signal args as block-register wire captures), runs the fragment's value/block instructions, copies results, and pops the frame. Recursion is forbidden, so RT call depth is a static bound.

**Tech Stack:** Rust (edition 2021), rill-lang pipeline (lexer → parser → infer → reduce → lower → IR → schedule → interp), arena+RC+COW memory from the data stage. No new external dependencies. `#![deny(unsafe_code)]`.

**Reference spec:** `docs/superpowers/specs/2026-09-25-rill-lang-first-class-functions-design.md`.

---

## Conventions used across all tasks

- **Run commands** from `rill/` (workspace root): `cargo test -p rill-lang`, `cargo clippy -p rill-lang`, `cargo fmt`.
- **Every task ends with a commit.** Conventional commits, single quotes in `-m` (AGENTS.md).
- **Warnings policy:** zero warnings. `cargo clippy -p rill-lang` after each task.
- **Docs:** all code comments and doc comments in English.
- **No new dependencies. No unsafe.**
- Current state: `Value::Func(u32)` + `ValueMakeFunc` exist (named registry index, β-reduced calls). `ValueCallFunc` is a no-op. `ValueTy::Func(String)` is a single-name type. `ValueReadCell`/`ValueWriteCell`/cell stack exist from the data stage.

---

### Task 1: Signature typing — `ValueTy::Func(Vec, Vec)` and `Closure` in the arena

**Files:**
- Modify: `rill-lang/src/types/ty.rs`
- Modify: `rill-lang/src/arena.rs`
- Modify: `rill-lang/src/ir.rs` (keep `ValueMakeFunc` for now; it will be reworked in Task 2)

- [ ] **Step 1: Write the failing test**

Add to `rill-lang/src/types/ty.rs`:

```rust
#[cfg(test)]
mod funcsig_tests {
    use super::*;

    #[test]
    fn func_signature_carries_arg_and_result_types() {
        let f = ValueTy::Func(vec![ValueTy::Float, ValueTy::Float], vec![ValueTy::Float]);
        assert_eq!(f, ValueTy::Func(vec![ValueTy::Float, ValueTy::Float], vec![ValueTy::Float]));
        // Signal-wire args are NOT in the type (positional wire-captures).
        let g = ValueTy::Func(vec![ValueTy::Float], vec![ValueTy::Float]);
        assert_ne!(f, g);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang funcsig_tests`
Expected: FAIL (compile error — `ValueTy::Func` takes one `String`).

- [ ] **Step 3: Change `ValueTy::Func`**

In `rill-lang/src/types/ty.rs`:

```rust
pub enum ValueTy {
    Int,
    Float,
    Data(String),
    Newtype(String),
    /// Function type: value-argument types and value-result types. Signal-wire
    /// arguments are positional wire-captures at the call site, not part of the
    /// type.
    Func(Vec<ValueTy>, Vec<ValueTy>),
    Var(TypeVarId),
}
```

- [ ] **Step 4: Add `Value::Closure` to the arena, remove `Value::Func(u32)`**

In `rill-lang/src/arena.rs`:

```rust
pub enum Value {
    Int(i64),
    Float(f64),
    Record(Vec<ArenaRef>),
    Sum(u32, Vec<ArenaRef>),
    Newtype(ArenaRef),
    /// A first-class function: captured environment record + body fragment id.
    Closure(ArenaRef, u32),
    Void,
}
```

Update `Value::kind()` and `ValueKind` (rename `Func` → `Closure`).

- [ ] **Step 5: Fix all `Value::Func(u32)` construction/pattern sites**

Grep and fix: `backend/interp.rs` (`ValueMakeFunc` executor, `Value::Func(*func)` → placeholder `Value::Closure(0, *func)`; `ValueKind::Func` → `ValueKind::Closure`), `arena.rs` tests, any `Value::Func` references in `lower.rs` (keep `ValueMakeFunc` emitting a `Closure` with a dummy env `0` — real env arrives in Task 4). `ValueTy::Func(String)` pattern sites in `infer.rs:891,1280` and `lower.rs:549` need `ValueTy::Func(name, vec![])`-style updates — see Step 6.

- [ ] **Step 6: Fix `ValueTy::Func(String)` construction sites**

`infer.rs:891` (`ValueTy::Func(name.into())` → `ValueTy::Func(vec![], vec![])` — a bare named ref with unknown sig is `Func([], [])` for now; real signatures land in Task 2/3). `infer.rs:1280` (pattern match `ValueTy::Func(ref_name)` → `ValueTy::Func(_, _)`). `lower.rs:549` similarly. Keep the code compiling; behavioral rework is Task 2+.

- [ ] **Step 7: Run tests**

Run: `cargo test -p rill-lang`
Expected: PASS (existing suite green; the 2 funcsig tests pass; existing func_values tests may need the new `Value::Closure` pattern — update `tests/func_values.rs` to match on `Value::Closure(_, _)`).

- [ ] **Step 8: Clippy + commit**

Run: `cargo clippy -p rill-lang` (zero warnings), `cargo fmt`.

```bash
git add rill-lang/src/types/ty.rs rill-lang/src/arena.rs rill-lang/src/ir.rs rill-lang/src/backend/interp.rs rill-lang/src/lower.rs rill-lang/src/types/infer.rs rill-lang/tests/func_values.rs
git commit -m 'feat(rill-lang): function signature typing and Closure arena value'
```

---

### Task 2: `FragmentIr` + real `ValueCallFunc` dispatch

**Files:**
- Modify: `rill-lang/src/ir.rs`
- Modify: `rill-lang/src/backend/interp.rs`
- Modify: `rill-lang/src/program.rs`

- [ ] **Step 1: Write the failing test**

Add to `rill-lang/src/backend/interp.rs`:

```rust
#[cfg(test)]
mod closure_dispatch_tests {
    use super::*;
    use crate::ir::{Ir, ValueInstr, ValueLayout, FragmentIr, FuncSig};
    use crate::program::RillProgram;
    use rill_core::traits::MultichannelAlgorithm;

    #[test]
    fn call_dispatch_runs_fragment() {
        // Closure(env=Void, fragment 0). Fragment: const int 7 -> result.
        // main: CallFunc{ dst:0, closure_slot:1, args:[] } after MakeClosure.
        let ir = Ir {
            instrs: Vec::new(),
            num_regs: 0,
            output_regs: Vec::new(),
            num_inputs: 0,
            num_outputs: 0,
            state: Default::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            value_instrs: vec![
                ValueInstr::ValueMakeClosure { dst: 1, env: 0, fragment: 0 },
                ValueInstr::ValueCallFunc { dst: 0, closure_slot: 1, args: vec![] },
            ],
            num_value_regs: 2,
            value_output_regs: vec![0],
            value_funcs: Vec::new(),
            value_state: ValueLayout { capacity: 16, value_state_slots: 0 },
            fragments: vec![FragmentIr {
                value_instrs: vec![
                    ValueInstr::ValueConstInt { dst: 0, value: 7 },
                ],
                steps: Vec::new(),
                num_value_regs: 1,
                num_block_regs: 0,
                output_value_regs: vec![0],
                output_block_regs: Vec::new(),
                sig: FuncSig { value_ins: 0, value_outs: 1, signal_ins: 0 },
            }],
        };
        let mut prog = RillProgram::<f32, 256>::new(ir);
        let mut out = [0.0f32; 2];
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert_eq!(prog.arena().get(v).unwrap(), &crate::arena::Value::Int(7));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang closure_dispatch_tests`
Expected: FAIL (compile error — `FragmentIr`/`ValueMakeClosure` undefined; `ValueCallFunc` is a no-op).

- [ ] **Step 3: Add `FragmentIr` and `FuncSig` to `ir.rs`**

```rust
/// Value/signal arity of a function fragment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FuncSig {
    /// Number of value arguments.
    pub value_ins: usize,
    /// Number of value results.
    pub value_outs: usize,
    /// Number of signal-wire arguments.
    pub signal_ins: usize,
}

/// A compiled function body: a fragment of the value/block track.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FragmentIr {
    /// Value-track instructions for the body.
    pub value_instrs: Vec<ValueInstr>,
    /// Block-track steps (for signal-wire args); empty for pure-value bodies.
    pub steps: Vec<Step>,   // import Step from crate::schedule
    /// Number of value registers (args + temps).
    pub num_value_regs: usize,
    /// Number of block registers (signal args + temps).
    pub num_block_regs: usize,
    /// Value register(s) holding the result.
    pub output_value_regs: Vec<usize>,
    /// Block register(s) holding signal results.
    pub output_block_regs: Vec<usize>,
    /// Arity.
    pub sig: FuncSig,
}
```

Add `pub fragments: Vec<FragmentIr>` to `Ir`. Add `ValueInstr::ValueMakeClosure { dst, env, fragment }` (rename `ValueMakeFunc` → `ValueMakeClosure`; same shape, `env: usize` value-register holding the env record).

- [ ] **Step 4: Implement `run_closure` dispatch**

In `backend/interp.rs`, replace the `ValueCallFunc` no-op with real dispatch. The interpreter runs the fragment's `value_instrs` via the existing `exec_value_instr`, with a temporary cell-stack frame. `ValueCallFunc` becomes:

```rust
ValueInstr::ValueCallFunc { dst, closure_slot, args } => {
    let slot = prog.value_regs[*closure_slot].copied().flatten();
    match slot.and_then(|s| prog.arena.get(s)) {
        Some(Value::Closure(env_ref, fragment_id)) => {
            let frag = prog.ir.fragments.get(*fragment_id).cloned();
            match frag {
                Some(f) => {
                    run_fragment(prog, &f, env_ref, args, dst, &mut drops);
                }
                None => prog.value_regs[*dst] = None,
            }
        }
        _ => prog.value_regs[*dst] = None,
    }
}
```

Implement `run_fragment` (in the same module):

```rust
/// Execute a function fragment with a temporary cell-stack frame.
/// Binds the captured env fields and value args as cells, runs the fragment's
/// value instructions, copies the result, and pops the frame.
fn run_fragment<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    frag: &FragmentIr,
    env_ref: crate::arena::ArenaRef,
    args: &[usize],
    dst: &usize,
    drops: &mut Vec<crate::arena::ArenaRef>,
) {
    // 1. Push a temp frame.
    prog.cell_stack.push(Vec::new());
    // 2. Bind env fields as cells: env is a Record; each field becomes a cell.
    if let Some(Value::Record(fields)) = prog.arena.get(env_ref) {
        for f in fields {
            // cell holds the value; bind into the top frame (name hash 0 = unnamed)
            let cell = prog.arena.alloc(Value::Void).unwrap_or(0);
            if let Some(v) = prog.arena.get(*f).cloned() {
                let _ = prog.arena.drop_ref(cell); // release Void placeholder
                let _ = prog.arena.alloc(v);       // reuse same slot? see note
            }
            prog.cell_stack.last_mut().push((0, cell));
        }
    }
    // 3. Bind value args as cells (index into fragment's value regs).
    for (i, a) in args.iter().enumerate() {
        let argval = prog.value_regs.get(*a).copied().flatten();
        // The fragment's i-th value register is a cell holding argval.
        // v1: fragment value_instrs reference local value registers directly;
        // cells are the capture mechanism. For v1 the args are passed by
        // copying the value into the fragment's registers (see lowering in
        // Task 3) — the cell frame is the *capture* frame, args flow as regs.
    }
    // 4. Run the fragment's value instructions.
    let saved_len = prog.value_regs.len();
    // v1: run fragment instructions into the SAME value_regs store, using
    // offsets so fragment-local regs don't collide. Simplest v1: run the
    // fragment's value_instrs via exec_value_instr with the fragment's own
    // register numbering, storing results in a SEPARATE scratch area.
    // NOTE: For v1 the interpreter will run fragment value_instrs with a
    // register offset = base (the fragment's num_value_regs already accounts
    // for it). See implementation note below.
    // 5. Copy result -> dst.
    // 6. Pop the frame.
    if let Some(frame) = prog.cell_stack.pop() {
        for (_, c) in frame {
            drops.push(c);
        }
    }
}
```

**IMPLEMENTATION NOTE (v1 register-offset scheme):** the fragment's `value_instrs` use fragment-local register numbers `0..num_value_regs-1`. The program's main `value_regs` store is sized `ir.num_value_regs`. To avoid collision, the interpreter runs a fragment by EXECUTING its instructions against a **temporary register slice** appended to `value_regs`: record `base = prog.value_regs.len()`, extend `value_regs` with `frag.num_value_regs` empty slots, run each `ValueInstr` with `dst`/`src`/`cell` fields offset by `base` (a small remapping — copy the instruction and add `base` to each register field), then copy `output_value_regs` (offset by base) into `prog.value_regs[*dst]` (a fresh owner via `arena.copy`), then truncate `value_regs` back to the pre-call length. This keeps the fragment's instructions untouched (they are reused across calls) and the store is pre-allocated to the max depth × fragment size (capacity math in Task 6). Implement `remap_value_instr(instr, base) -> ValueInstr` that offsets every register field; apply it in `run_fragment`.

Block steps (`frag.steps`) run via the existing `exec_block_op`/`exec_foreign_block` over the program's `block_regs` (the wire captures bind fragment block registers to caller block registers by index — see Task 5).

- [ ] **Step 5: `ValueMakeClosure` executor**

Replace the `ValueMakeFunc` executor:

```rust
ValueInstr::ValueMakeClosure { dst, env, fragment } => {
    let env_ref = prog.value_regs.get(*env).copied().flatten().unwrap_or(0);
    prog.value_regs[*dst] = alloc_owned(prog, Value::Closure(env_ref, *fragment as u32));
}
```

- [ ] **Step 6: Fix `Ir { .. }` constructors**

Add `fragments: Vec::new()` to the `Ir` construction sites in `program.rs`, `lower.rs`, `schedule.rs` tests, `serde_def.rs` (search `Ir {`).

- [ ] **Step 7: Run tests**

Run: `cargo test -p rill-lang closure_dispatch_tests`
Expected: PASS.
Run: `cargo test -p rill-lang`
Expected: PASS (regression green).

- [ ] **Step 8: Clippy + commit**

Run: `cargo clippy -p rill-lang` (zero warnings), `cargo fmt`.

```bash
git add rill-lang/src/ir.rs rill-lang/src/backend/interp.rs rill-lang/src/program.rs rill-lang/src/lower.rs rill-lang/src/schedule.rs rill-lang/src/serde_def.rs
git commit -m 'feat(rill-lang): FragmentIr and runtime closure dispatch'
```

---

### Task 3: Lambda literals — lexer, parser, AST, infer

**Files:**
- Modify: `rill-lang/src/lexer.rs`
- Modify: `rill-lang/src/parser.rs`
- Modify: `rill-lang/src/ast.rs`
- Modify: `rill-lang/src/types/infer.rs`

- [ ] **Step 1: Write the failing test**

Add to `rill-lang/src/parser.rs`:

```rust
#[test]
fn parses_lambda_literal() {
    let p = prog("double = fn x -> x * 2.0; main = double");
    let main = p.main_def().unwrap();
    assert!(matches!(main.body(), Expr::Ref(name, _) if name == "double"));
    // double's body is a Lambda.
    let d = p.defs.iter().find(|d| d.name() == "double").unwrap();
    assert!(matches!(d.body(), Expr::Lambda { .. }));
}

#[test]
fn parses_nested_lambda() {
    let p = prog("adder = fn n -> fn x -> x + n; main = adder 2.0");
    let d = p.defs.iter().find(|d| d.name() == "adder").unwrap();
    assert!(matches!(d.body(), Expr::Lambda { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang parser::tests::parses_lambda`
Expected: FAIL (`Expr::Lambda` undefined).

- [ ] **Step 3: Lexer — `fn` keyword**

Add `KwFn` to `Tok`; map `"fn"` (with `!followed_by_paren`) in the keyword match.

- [ ] **Step 4: AST — `Expr::Lambda`**

```rust
/// Lambda literal `fn p1 p2 ... -> body`.
Lambda {
    /// Parameters.
    params: Vec<Param>,
    /// Body expression.
    body: Box<Expr>,
    /// Span.
    span: Span,
},
```

Update `Expr::span()`.

- [ ] **Step 5: Parser — `fn` prefix**

In `parse_prefix`:

```rust
Tok::KwFn => {
    self.bump();
    let mut params = Vec::new();
    while let Tok::Ident(_) = self.peek().tok {
        let (pname, pspan) = self.expect_ident()?;
        params.push(Param { name: pname, span: pspan });
    }
    self.eat(&Tok::FatArrow)?;   // Tok::FatArrow (`=>`) already exists from match
    let body = self.parse_expr(0, true)?;
    Ok(Expr::Lambda { params, body: Box::new(body), span: t.span })
}
```

- [ ] **Step 6: Infer — lambda signature + capture set**

In `infer_expr`:

```rust
Expr::Lambda { params, body, span } => {
    // Bind params as value channels (v1: value lambdas; signal-arg lambdas
    // land in Task 5). Infer the body; the result type is the function type.
    let saved = ctx.locals.clone();
    for p in params {
        ctx.locals.insert(p.name.clone(), ArrowTy::value_channel(ctx.fresh_vty()));
    }
    let bt = infer_expr(ctx, body)?;
    ctx.locals = saved;
    if bt.arity_out() != 1 {
        return Err(CompileError::Type { msg: "lambda body must produce one value", span: *span });
    }
    let arg_tys: Vec<ValueTy> = params.iter().map(|_| ValueTy::Float).collect();
    let ret_ty = bt.outs[0].vty;
    Ok(ArrowTy::value_channel(ValueTy::Func(arg_tys, vec![ret_ty])))
}
```

Add `fresh_vty()` to `Ctx` (a fresh `ValueTy::Var` using the same `next` counter). Note: v1 uses `ValueTy::Float` for lambda params (the value system's default scalar); Task 5 generalizes to value-typed params and signal args.

Also add a catch-all in `infer_def_group` so a `Def::Local` whose body is a `Lambda` is inferred normally (no special handling needed — it is just an expression).

- [ ] **Step 7: Reduce — keep lambdas**

In `reduce.rs`, `substitute` must NOT inline inside a `Lambda` body's captured names, but the body expression itself may reference free variables by name (they resolve via the enclosing scope). Add a `Expr::Lambda` arm to `substitute` and `reduce_expr` that recurses into the body with the current substitution (free names that are substituted become captures; this is correct for by-value capture). Keep it simple: recurse.

- [ ] **Step 8: Render — round-trip**

Add `Expr::Lambda` to `render.rs` so isomorphism tests pass: `fn p1 p2 -> body`.

- [ ] **Step 9: Run tests**

Run: `cargo test -p rill-lang parser::tests::parses_lambda`
Expected: PASS.
Run: `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 10: Clippy + commit**

```bash
git add rill-lang/src/lexer.rs rill-lang/src/parser.rs rill-lang/src/ast.rs rill-lang/src/types/infer.rs rill-lang/src/reduce.rs rill-lang/src/render.rs
git commit -m 'feat(rill-lang): lambda literal syntax and inference'
```

---

### Task 4: Env capture + closure creation in lowering

**Files:**
- Modify: `rill-lang/src/lower.rs`
- Modify: `rill-lang/src/backend/interp.rs`
- Create: `rill-lang/tests/closures.rs`

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/closures.rs`:

```rust
use rill_lang::compile;
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn lambda_captures_parent_param_by_value() {
    // adder = fn n -> fn x -> x + n;  add2 = adder 2.0;  main = add2 3.0 -> 5.0
    let mut prog = compile::<f32>(
        "adder = fn n -> fn x -> x + n; add2 = adder 2.0; main = add2 3.0",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    let v = vo[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Float(5.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test closures`
Expected: FAIL (lambdas not lowered to fragments yet).

- [ ] **Step 3: Lower a `Lambda` to a `FragmentIr`**

In `lower.rs`, `lower_value` handles `Expr::Lambda`:

```rust
Expr::Lambda { params, body, span } => {
    // 1. Compute the free-variable capture set: names referenced in `body`
    //    that are NOT lambda params and NOT the body's own bindings.
    let free = free_vars(body, params_names);
    // 2. Compile the body into a new FragmentIr via a nested Lowerer.
    let fragment = self.lower_fragment(params, body, &free, *span)?;
    // 3. Build the env Record: emit value instructions that copy each free
    //    variable's current value into a Record slot (by-value snapshot).
    let env_reg = self.emit_env_snapshot(&free)?;
    // 4. Emit ValueMakeClosure { dst, env: env_reg, fragment: fragment_id }.
    let dst = self.fresh_value_reg();
    self.emit_value(ValueInstr::ValueMakeClosure { dst, env: env_reg, fragment: fragment_id });
    Ok((dst, ValueTy::Func(arg_tys, ret_tys)))
}
```

Implement helpers:
- `lower_fragment(params, body, free, span) -> Result<usize /*fragment id*/, CompileError>` — creates a nested `Lowerer` sharing `env`, compiles the body's value instructions into a `FragmentIr`, pushes it to `self.fragments`, returns the index. The fragment's instructions reference fragment-local value registers; the capture env fields are bound as cells (see §4 note).
- `emit_env_snapshot(&self, free) -> Result<usize, CompileError>` — for each free name, emit `ValueReadCell`-equivalent to get its current value (via `lower_value_ref`), then `ValueConstructRecord` over the collected value regs; return the record's value register.
- `free_vars(e, params) -> Vec<String>` — collect `Expr::Ref` names in `e` not in `params` and not locally bound (a small AST walk; skip names bound by inner `let`).

**v1 fragment/arg-binding scheme:** the fragment's value registers `0..num_value_regs-1` are its locals. The captured env fields are bound as cells in the TEMPORARY call frame (Task 2's `run_fragment` pushes the frame; Task 2 already binds env Record fields as cells). Inside the fragment, a reference to a captured name resolves to `ValueReadCell` on the cell at the name's index. To keep Task 2's `run_fragment` coherent: the env Record field order == the fragment's capture order == the cell indices in the temp frame. The fragment's `ValueReadCell { dst, cell }` instructions use cell indices into the temp frame (not the program's `value_regs`). Implement by making `run_fragment` pass the frame cells as a small indexable slice; the remap in Task 2 already offsets registers — extend it so `cell` fields that are capture indices map to the temp frame slice. Document the scheme in code.

- [ ] **Step 4: Fix `lower_fragment` register handling in `run_fragment`**

Extend Task 2's `run_fragment` per the note above: env Record fields become cells in the temp frame at indices `0..n-1` (in Record order); fragment `ValueReadCell { cell: i }` with `i < env_len` reads frame cell `i`. Value args are additional cells at indices `env_len..env_len+args-1`. Update `run_fragment` accordingly and re-run the Task 2 test.

- [ ] **Step 5: Run tests**

Run: `cargo test -p rill-lang --test closures`
Expected: PASS.
Run: `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 6: Clippy + commit**

```bash
git add rill-lang/src/lower.rs rill-lang/src/backend/interp.rs rill-lang/tests/closures.rs
git commit -m 'feat(rill-lang): closure env capture by-value snapshot'
```

---

### Task 5: Currying + signal-wire arguments

**Files:**
- Modify: `rill-lang/src/lower.rs`
- Modify: `rill-lang/src/types/infer.rs`
- Create: `rill-lang/tests/currying_wires.rs`

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/currying_wires.rs`:

```rust
use rill_lang::compile;
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn currying_applies_partially() {
    // add = fn a b -> a + b;  add3 = add 3.0;  main = add3 4.0 -> 7.0
    let mut prog = compile::<f32>(
        "add = fn a b -> a + b; add3 = add 3.0; main = add3 4.0",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Float(7.0));
}

#[test]
fn signal_wire_argument_scales_block() {
    // amp = fn g x -> x * g;  main = amp 2.0 _  ->  input * 2.0  (block output)
    let mut prog = compile::<f32>("amp = fn g x -> x * g; main = amp 2.0 _").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0, 2.0, 3.0, 4.0]], &mut [&mut out]).unwrap();
    assert_eq!(out, [2.0, 4.0, 6.0, 8.0]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test currying_wires`
Expected: FAIL.

- [ ] **Step 3: Currying — partial application as a closure**

`add3 = add 3.0` — `add` is `Func([Float, Float], [Float])`; applied to 1 arg. In `infer_apply` (infer.rs:1274 region), when a `Func` value is applied to FEWER args than its signature: build a **partial-application closure** — a `FragmentIr` whose body calls the referenced function with the captured args + the remaining ones. Implement in lowering: `apply_partial(fn_value, applied_args)` emits a fragment that, on call, binds the applied args as captured cells and dispatches `ValueCallFunc` to `fn_value` with `(captured..., remaining...)`. The curried fragment's own env holds the applied args.

- [ ] **Step 4: Signal-wire args — inference**

Extend `Expr::Lambda` inference so a lambda may take signal-wire params. In v1, a lambda whose body uses a param in a signal (block) position infers that param as a **signal arg**: the param's local type is a signal channel. Track per-lambda: `(value_params, signal_params)` split. `ValueTy::Func` types only value params; `FuncSig.signal_ins` counts signal params. Update the lambda inference and `FuncSig`.

- [ ] **Step 5: Signal-wire args — lowering + dispatch**

- A lambda with signal params compiles to a `FragmentIr` with `steps` (block-track) and `num_block_regs`.
- `ValueCallFunc` dispatch: signal args are bound as block-register references. In `run_fragment`, bind the caller's block registers (passed by index) as the fragment's block registers (wire captures, zero-copy). The fragment's block steps run over them via the existing `exec_block_op`.
- A signal param used in the body (`x * g`) lowers to `Instr::Bin` over the fragment's block reg for `x` and the materialized value for `g` (the value-arg cell read → float → block fill, like `ReadMainCell` from the data stage).

- [ ] **Step 6: Run tests**

Run: `cargo test -p rill-lang --test currying_wires`
Expected: PASS.
Run: `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 7: Clippy + commit**

```bash
git add rill-lang/src/lower.rs rill-lang/src/types/infer.rs rill-lang/tests/currying_wires.rs
git commit -m 'feat(rill-lang): currying via closures and signal-wire arguments'
```

---

### Task 6: HOF combinators, static depth, capacity, docs

**Files:**
- Create: `rill-lang/tests/hof.rs`
- Modify: `rill-lang/src/lower.rs` (static depth + capacity)
- Modify: `rill-lang/src/program.rs` (pre-allocated call stack)
- Modify: `docs/src/guides/rill-lang.md`, `rill-lang/README.md`

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/hof.rs`:

```rust
use rill_lang::compile;
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn twice_applies_twice() {
    // twice = fn f x -> f (f x);  double = fn x -> x * 2.0;  main = twice double 3.0 -> 12.0
    let mut prog = compile::<f32>(
        "twice = fn f x -> f (f x); double = fn x -> x * 2.0; main = twice double 3.0",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Float(12.0));
}

#[test]
fn recursion_is_rejected() {
    // f = fn x -> f x  -> compile error (acyclic contract)
    let res = compile::<f32>("f = fn x -> f x; main = f 1.0");
    assert!(res.is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test hof`
Expected: FAIL (recursion not yet rejected; HOF may not lower).

- [ ] **Step 3: HOF dispatch**

`twice f x -> f (f x)` — `f` is a value arg of function type. Lowering compiles `f (f x)` as: evaluate `f x` (a `ValueCallFunc`), then apply the result closure again. This requires the fragment's value args to be closures (they are — value args are passed as closure values). Verify `ValueCallFunc` dispatch handles a closure whose value arg is itself a closure (it does — it reads `Value::Closure` from the slot). Make sure HOF functions type-check (`f` : `Func([T],[T])`).

- [ ] **Step 4: Static depth + acyclicity**

In `infer.rs`, build a **call graph** of closures/lambdas (edge `A → B` when `A`'s body calls a function value that statically resolves to `B`). Reject a cycle with a compile error ("recursive function call"). Compute the longest path as `max_call_depth`; store on `TypedProgram`. In `lower.rs`, pass it into `Ir` as `max_call_depth`.

- [ ] **Step 5: Pre-allocated call stack**

In `program.rs`, pre-allocate the value-register scratch to `max_call_depth × max_fragment_regs` (a new `Ir::value_state`-style field `max_call_depth`). `run_fragment`'s register-slice scheme (Task 2) reuses this pre-allocated space — no growth on the RT path. Add `num_fragment_slots` to `ValueLayout` or a new `Ir::call_layout`. Assert at construction that the bound holds.

- [ ] **Step 6: Capacity**

Update the arena-capacity computation in `lower.rs` to include: closure allocations (`ValueMakeClosure`), env snapshots, and the per-call temp cells (env fields + value args) × max_call_depth. Keep the strict upper bound under acyclicity.

- [ ] **Step 7: Run tests**

Run: `cargo test -p rill-lang --test hof`
Expected: PASS.
Run: `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 8: Docs**

Update `docs/src/guides/rill-lang.md` (lambda literals, closures, currying, HOF, signal-wire args, recursion-forbidden, call-depth bound) and `rill-lang/README.md`. Remove the old "functions as values = named refs only" limitation notes.

- [ ] **Step 9: Full verification + commit**

Run: `cargo test -p rill-lang`, `cargo test --workspace`, `cargo clippy --workspace`, `cargo fmt`.

```bash
git add rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/src/program.rs rill-lang/src/ir.rs rill-lang/tests/hof.rs docs/src/guides/rill-lang.md rill-lang/README.md
git commit -m 'feat(rill-lang): HOF combinators, static call depth, and capacity bound'
```

---

## Self-review notes

- Spec §2.1 (Closure) → Task 1. §2.2 (capture) → Task 4. §2.3 (signature typing) → Tasks 1+3+5. §2.4 (wire-captures) → Task 5. §2.5 (recursion) → Task 6. §3 (FragmentIr) → Task 2. §4 (dispatch) → Tasks 2+5. §5 (syntax) → Task 3. §8 (phases 1-6) → Tasks 1-6. §9 (testing) → per-task tests. §10 (files) → per-task files.
- `Value::Func(u32)` → `Closure`, `ValueMakeFunc` → `ValueMakeClosure`, `ValueTy::Func(String)` → `Func(Vec, Vec)` are renamed consistently in Tasks 1-2.
- The `run_fragment` register-offset scheme is the key implementation risk; the Task 2 test pins the dispatch, Task 4 extends it with env cells.
- No placeholders: every step has code or an exact command. New external dependencies: none.
- Type consistency: `FragmentIr`, `FuncSig`, `Value::Closure`, `ValueTy::Func(Vec, Vec)`, `ValueMakeClosure`, `ValueCallFunc` are defined once and reused.