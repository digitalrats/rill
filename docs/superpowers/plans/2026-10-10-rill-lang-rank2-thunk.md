# rill-lang: Rank-N Polymorphism + Thunk Unification — Implementation Plan (Stage B sub-plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add full rank-N polymorphism (`forall` type expressions, value-type-variable generalization, `runST : (forall s. ST s a) -> a` with automatic escape rejection), then unify the interpreter's fragment runners behind a Rust `Thunk` trait.

**Architecture:** Part A (rank-N, type-system only): add `TypeExpr::TForall`, extend `Scheme.vars`/`instantiate` to value-type variables, type `runST`'s phantom param as a quantified var so escaping is rejected by generalization. Part B (Thunk, runtime only): a `Thunk` trait giving `fragment()`/`env()`/`memo()` for `Value::Closure` and `Value::Thunk`, merging `run_fragment` + `run_thunk_body` into one `run_call`. See `docs/superpowers/specs/2026-10-10-rill-lang-rank2-thunk-design.md`.

**Tech Stack:** Rust, crate `rill-lang`, branch `feature/rill-lang-arrays`. No new external dependencies.

---

## Conventions

- Verify after every task: `cargo test -p rill-lang`; before finishing `cargo clippy --all-features --workspace` + `cargo fmt`.
- Pre-commit hook runs `cargo fmt` — run it before every commit.
- Commit style: conventional commits, single-quoted `-m`.
- The superseded one-point `runST` implementation (commit `5b2c84d`) is in the tree. Its syntax/prelude/lowering stay; its **inference approach** (`type_var_bindings` + `vty_mentions_tyvar` escape scan) is replaced by real generalization. Read `git show 5b2c84d` before starting.
- The forked-rust dialect: no int→f32 literal coercion (write `&[2.0]`), `core::`/`std::` preludes, `matches!`/`vec![]`.

---

## Task R1: `forall` syntax — `TypeExpr::TForall`

**Files:**
- Modify: `rill-lang/src/ast.rs` (`TypeExpr` enum + helpers)
- Modify: `rill-lang/src/lexer.rs` (keyword `forall`)
- Modify: `rill-lang/src/parser.rs` (parse `forall a b. <type>`)
- Modify: `rill-lang/src/render.rs` (round-trip)
- Test: `rill-lang/tests/forall_syntax.rs`

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/forall_syntax.rs`:

```rust
use rill_lang::lexer::tokenize;
use rill_lang::parser::parse;

#[test]
fn forall_parses_and_roundtrips() {
    // A type signature with an explicit forall (currently parsed only in
    // typeclass method sigs; here we pin the standalone form).
    let src = "typeclass C a where { f: forall b. a -> b -> a; }; main = _;";
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes());
    assert!(prog.is_ok(), "forall in a type expression must parse: {prog:?}");
}
```

(Adjust the example to what the parser actually accepts for a `forall` in a
`TypeExpr`; the point is that `forall a b. <type>` parses and renders back.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test forall_syntax`
Expected: FAIL — `forall` not a recognized token/type form.

- [ ] **Step 3: Add `TForall` to `TypeExpr`**

In `ast.rs`, add to `enum TypeExpr`:

```rust
/// Explicit universal quantification: `forall a b. T`.
TForall(Vec<String>, Box<TypeExpr>),
```

Extend `arg_count`/`arg_types`/any exhaustive `match` on `TypeExpr` to handle the
new variant (a `forall`'s arg count is its inner type's).

- [ ] **Step 4: Lex + parse `forall`**

`lexer.rs`: keyword `forall`. `parser.rs`: in type parsing, when the token is
`forall`, read one or more type-var names until `.`, then parse the inner
`TypeExpr` → `TForall(names, inner)`. Allow it anywhere a `TypeExpr` is parsed.

- [ ] **Step 5: Render round-trip**

`render.rs`: `forall a b. <inner>`.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add rill-lang/src/ast.rs rill-lang/src/lexer.rs rill-lang/src/parser.rs rill-lang/src/render.rs rill-lang/tests/forall_syntax.rs
git commit -m 'feat(rill-lang): forall type expressions (TypeExpr::TForall)'
```

---

## Task R2: Value-type-variable generalization

**Files:**
- Modify: `rill-lang/src/types/infer.rs` (`free_vars`, `instantiate`, helpers)
- Modify: `rill-lang/src/types/ty.rs` (if `free_vars`/`instantiate` move or need subst access)
- Test: `rill-lang/tests/value_polymorphism.rs`

- [ ] **Step 1: Write the failing test**

Create `rill-lang/tests/value_polymorphism.rs`:

```rust
use rill_lang::lexer::tokenize;
use rill_lang::parser::parse;
use rill_lang::types::infer::infer_program;
use rill_lang::mode::Mode;

#[test]
fn polymorphic_value_fn_instantiates_fresh() {
    // id2 used at two different types must instantiate fresh each time.
    let src = "{-# MODE complete #-}\n\
        id2 = fn x -> x;\n\
        main = match (head (id2 [1.0, 2.0])) of { Nothing => 0.0; Just x => x };";
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes()).unwrap();
    assert!(infer_program(&prog, Mode::Complete).is_ok(),
        "a polymorphic value function must generalize its type variable");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test value_polymorphism`
Expected: FAIL or the polymorphic use errors (value channel stays monomorphic).

- [ ] **Step 3: Extend `free_vars` for value-tyvars**

In `infer.rs`, add `value_free_vars(t: &ValueTy, subst: &Subst) -> Vec<TypeVarId>`:
traverse `ValueTy` (Var / TyConVar / TyConApp-head / Func args+rets / Data·Newtype·App
args), collect free ids resolved through `subst.value_map`, dedup. In `Ctx::free_vars`
(currently only `Scalar::Var` from `elem`), also collect value-tyvars from each
channel's `vty` and append to the returned `vars`.

- [ ] **Step 4: Extend `instantiate` for value-tyvars**

Add `value_instantiate(t: &ValueTy, fresh: &HashMap<TypeVarId, TypeVarId>) -> ValueTy`
that replaces quantified `Var(v)`/`TyConVar(v)`/`TyConApp(v, args)` heads with
fresh vars (recursively over Func/Data/Newtype/App args). In `Ctx::instantiate`,
build the fresh map for value vars too and apply it to `Rate::Value` channels
(replace `Channel::value(b.vty.clone())` with
`Channel::value(value_instantiate(&b.vty, &remap))`).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p rill-lang`
Expected: PASS (value polymorphism + no regression). If the full suite regresses,
narrow: only generalize `TyConVar`-kind value vars (the phantom case), keep
ordinary `ValueTy::Var` monomorphic — report as DONE_WITH_CONCERNS with the
regression evidence.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add rill-lang/src/types/infer.rs rill-lang/src/types/ty.rs rill-lang/tests/value_polymorphism.rs
git commit -m 'feat(rill-lang): generalize value-type variables in Scheme.vars/instantiate'
```

---

## Task R3: `runST` as syntax sugar over a forall-typed function

**Files:**
- Modify: `rill-lang/src/parser.rs` (desugar `runST (fn s -> body)` → `Expr::Apply { name: "runST", args: [lambda] }`, like `do {}` → `bind`)
- Modify: `rill-lang/src/types/ty.rs` (TypeEnv: `runST` built-in signature `(forall s. ST s a) -> a`)
- Modify: `rill-lang/src/types/infer.rs` (rank-N application in `infer_apply_impl`; remove `Expr::RunST` special form + one-point scan)
- Modify: `rill-lang/src/lower.rs` (remove `Expr::RunST` special lowering; `runST` lowers as an ordinary function)
- Test: `rill-lang/tests/st_monad.rs`, `rill-lang/tests/rank2_poly.rs`

> **Design decision (JIT-friendly):** `runST` is NOT a special form. The parser
> desugars `runST (fn s -> body)` into an ordinary call `Apply runST [fn s -> body]`
> (the same pattern as `do {}` → `bind`). `runST` is a built-in function whose
> signature `(forall s. ST s a) -> a` lives in the `TypeEnv`. Rank-N application
> lives in `infer_apply_impl`: the argument lambda is inferred with `s` as a fresh
> quantified var; the result cannot mention `s`, so escaping is rejected
> automatically. `forall s.` is purely type-level and erased at lowering, so a
> future JIT (FragmentIr → native call) sees only an ordinary function call — no
> special IR instruction, no special JIT case.

- [ ] **Step 1: Write the failing tests**

`rill-lang/tests/st_monad.rs`:

```rust
use rill_core::traits::MultichannelAlgorithm;
use rill_lang::builtin::Registry;

fn run(src: &str) -> rill_lang::program_engine::ProgramEngine<f32, 256> {
    let reg = Registry::<f32>::new();
    rill_lang::compile_graph::<f32, 256>(src, &reg, 44100.0).unwrap()
}

#[test]
fn runst_region_erases_phantom() {
    let mut p = run("{-# MODE complete #-}\nmain = runST (fn s -> 1.0);");
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut p, &[], &mut [&mut out]);
    assert_eq!(out[0], 1.0);
}

#[test]
fn runst_region_rejects_escaping_phantom() {
    let src = "{-# MODE complete #-}\nmain = runST (fn s -> (s));";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let r = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Complete);
    assert!(r.is_err(), "ST result must not escape the runST region");
}
```

`rill-lang/tests/rank2_poly.rs` (the two one-point bypasses):

```rust
#[test]
fn phantom_chain_through_higher_id_var_rejected() {
    let src = "{-# MODE complete #-}\nmain = (fn t -> runST (fn s -> [t, s])) 1.0;";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let r = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Complete);
    assert!(r.is_err(), "phantom s must not unify with an outer variable");
}

#[test]
fn phantom_shadows_outer_value_binder() {
    let src = "{-# MODE complete #-}\ns = 2.0;\nmain = runST (fn s -> (s));";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let r = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Complete);
    assert!(r.is_err(), "phantom s must shadow an outer value named s");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rill-lang --test st_monad --test rank2_poly`
Expected: FAIL (the one-point `Expr::RunST` special form still in the tree, or `runST`
unknown as a built-in).

- [ ] **Step 3: Desugar `runST` in the parser**

In `parser.rs`, replace the `Tok::KwRunST` special-form arm (currently ~line 998
building `Expr::RunST`) with sugar: parse `runST` + parenthesized lambda, and emit
`Expr::Apply { name: "runST".into(), args: vec![lambda], span }`. This is exactly
how `do {}` builds `bind` (see the `parse_do_block` pattern). Verify the parse
produces `Apply runST [fn s -> body]`.

- [ ] **Step 4: Add `runST`'s forall signature to the TypeEnv**

In `types/ty.rs`, register `runST` as a built-in function whose signature is the
`TypeExpr` `(forall s. ST s a) -> a`. It is NOT a foreign fn (FFI is for DSP
algorithms and its `ffi_sig_from_typeexpr` hits the `other` arm for forall). Model
it on how `pure`/`return`/`mempty` (result-directed typeclass methods) carry their
signatures in `TypeclassInfo.methods` — `runST` gets a TypeEnv entry that
`infer_apply_impl` consults when the callee is `runST`.

- [ ] **Step 5: Rank-N application in `infer_apply_impl`**

In `infer.rs` `infer_apply_impl` (the user-defined/built-in application path,
~line 3975), add handling for `runST` (and, generally, a callee whose signature
contains a `forall`-bound argument):
1. `runST`'s signature is `(forall s. ST s a) -> a`: the ARGUMENT is polymorphic
   in `s`.
2. Infer the argument lambda with `s` as a fresh quantified var — bind `s` in
   `ctx.locals` (save/restore like the `Expr::Lambda` arm) as a value channel
   whose `vty` is `ValueTy::TyConVar(s_id)`, so it shadows outer binders AND
   `Ref("s")` resolves to it.
3. Infer the lambda body with `s` visible.
4. The result `a` must NOT mention `s` — because `s` is quantified (fresh, not a
   free var of the enclosing scope), a result that unified with `s` fails the
   scheme generalization and the application is rejected automatically.
5. Return `ArrowTy::value_channel(a)` (the phantom erased).

If a generic rank-N mechanism (callee sig with a `forall`-bound arg) is too much,
implement it specifically for `runST` first and document the generalization point.

- [ ] **Step 6: Remove the one-point special form**

Delete `Expr::RunST` from the AST if no longer referenced (check ast.rs, parser.rs,
reduce.rs, render.rs, lower.rs, infer.rs). Remove the `type_var_bindings` fallback
in `infer_ref_inner` and the `vty_mentions_tyvar`-based escape check from
`5b2c84d` (keep `vty_mentions_tyvar` only if still used, or remove it). Remove the
`Expr::RunST` arms from lower.rs (no-op lowering) — `runST` now lowers as an
ordinary `Apply` (a normal fragment call).

- [ ] **Step 7: Run the tests**

Run: `cargo test -p rill-lang --test st_monad --test rank2_poly`
Expected: PASS — both runST tests AND both bypass regressions rejected.

- [ ] **Step 8: Full suite + commit**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`,
`cargo clippy -p rill-lang`, `cargo fmt`.
Commit:

```bash
cargo fmt
git add rill-lang/src/parser.rs rill-lang/src/ast.rs rill-lang/src/types/ty.rs rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/src/reduce.rs rill-lang/src/render.rs rill-lang/tests/st_monad.rs rill-lang/tests/rank2_poly.rs
git commit -m 'feat(rill-lang): runST as sugar over a forall-typed function — rank-N application
Parser desugars runST (fn s -> body) into an ordinary Apply; runST has signature
(forall s. ST s a) -> a in the TypeEnv; rank-N application in infer_apply_impl
rejects escaping automatically. JIT-friendly: no special IR instruction.'
```

---

## Task T1: Unify `Closure`/`Thunk` into one `Value::Callable` + single runner

**Files:**
- Modify: `rill-lang/src/arena.rs` (`Value::Callable { fragment, env, cell: Option }`; `ValueKind`; `visit_refs`; `clone_pooled`; `detach_payload`; tests)
- Modify: `rill-lang/src/backend/interp.rs` (single `run_call`; `force()` over `Callable`; `ValueMakeClosure`/`ValueLazy`/`ValueCallLazy` build `Callable`; `ValueCallFunc`; `drop_value_children`; `recount_children`)
- Test: `rill-lang/tests/thunk_unify.rs`, existing suite

> **Design decision (no trait):** `Value::Closure(env, fid)` and
> `Value::Thunk { fragment, env, cell }` differ only in the memo cell. Unify
> into one variant:
> ```rust
> /// A callable value: captured env + body fragment + optional memo cell.
> /// `cell: None` = eager closure (no memo); `cell: Some(c)` = lazy thunk
> /// (memoized, call-by-need).
> Callable { fragment: u32, env: ArenaRef, cell: Option<ArenaRef> }
> ```
> The single runner `run_call` takes the args-source as a parameter (caller
> registers for eager, env-record tail split by `value_ins` for lazy). This is
> codebase preparation for the track-unification tasks (B4+): one callable
> value, one dispatch path.

- [ ] **Step 1: Write a guard test**

Create `rill-lang/tests/thunk_unify.rs`:

```rust
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn eager_and_lazy_calls_still_work() {
    // eager: a named function applied to args; lazy: a thunk over a closure.
    let src = "{-# MODE complete #-}\n\
        id2 = fn x -> x;\n\
        main = match (head (id2 [1.0, 2.0])) of { Nothing => 0.0; Just x => x };";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 1.0),
        other => panic!("expected Float(1.0), got {other:?}"),
    }
}

#[test]
fn closure_and_thunk_are_one_variant() {
    // Both construction paths (MakeClosure, ValueLazy, ValueCallLazy) now
    // produce Value::Callable; the arena has no Closure/Thunk variants.
    // (Compile a program using a lambda + a lazy let to exercise both.)
    let src = "{-# MODE complete #-}\n\
        id2 = fn x -> x;\n\
        main = match (head (let y = id2 [1.0, 2.0] in y)) of { Nothing => 0.0; Just x => x };";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 1.0),
        other => panic!("expected Float(1.0), got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify they pass before the change**

Run: `cargo test -p rill-lang --test thunk_unify`
Expected: PASS (baseline — both paths work under the current `Closure`/`Thunk`).

- [ ] **Step 3: Replace the `Value` variants**

In `arena.rs`, replace `Value::Closure(ArenaRef, u32)` and
`Value::Thunk { fragment, env, cell }` with:

```rust
/// A callable value: captured env + body fragment + optional memo cell.
/// `cell: None` = eager closure (no memo); `cell: Some(c)` = lazy thunk
/// (memoized, call-by-need). Unifies the former `Closure` and `Thunk`.
Callable { fragment: u32, env: ArenaRef, cell: Option<ArenaRef> },
```

Update `ValueKind` (merge `Closure`/`Thunk` into one kind or keep both names
pointing at the same kind), `visit_refs`, `clone_pooled`, `detach_payload`, and
any arena tests that construct `Closure`/`Thunk`.

- [ ] **Step 4: Update the interpreter**

In `interp.rs`:
- `recount_children` / `drop_value_children`: `Callable { env, cell, .. }` — recount/drop `env`; recount/drop `cell` if `Some`.
- `ValueMakeClosure` (interp.rs:1084): build `Callable { fragment, env, cell: None }`.
- `ValueLazy` (interp.rs:1308): build `Callable { fragment, env, cell: Some(cell) }`.
- `ValueCallLazy` (interp.rs:1025): build `Callable { fragment, env: combined, cell: Some(cell) }`.
- `ValueCallFunc` (interp.rs:987): read `Callable { fragment, env, cell: None }`, call `run_call` with args from registers.
- `force()` (interp.rs:2233): match `Callable { fragment, env, cell: Some(cell) }` — the lazy path (memo/black-hole/cycle) is unchanged; `Callable { cell: None }` returns as WHNF (the current non-Thunk `return r`).

- [ ] **Step 5: Merge the two runners into `run_call`**

Create a single `run_call(prog, frag, env_ref, args: ArgsSource, dst, drops)` where
`ArgsSource` distinguishes caller registers (`&[usize]`) from the env-record tail
(split by `frag.sig.value_ins`). Share steps 0-6 (scratch reserve → env→cells →
watermark → args copy → run blocks → result → drain). Preserve the `max_call_regs`
guard. `run_thunk_body`'s env-tail logic becomes the lazy `ArgsSource`; the old
`run_fragment`'s register logic the eager one. Update `force()` to call `run_call`
for the lazy path, and `call_closure_single`/`call_closure_args` (interp.rs:2005/2031)
for the eager path. Remove `run_fragment` and `run_thunk_body`.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`
Expected: PASS — the full suite (incl. the guard test) is unchanged.

- [ ] **Step 7: Clippy + fmt + commit**

Run: `cargo clippy --all-features -p rill-lang`, `cargo fmt`.
Commit:

```bash
cargo fmt
git add rill-lang/src/arena.rs rill-lang/src/backend/interp.rs rill-lang/tests/thunk_unify.rs
git commit -m 'refactor(rill-lang): unify Closure/Thunk into Value::Callable + single run_call'
```

---

## Task T2: `remap_value_instr` without cloning (fragment-call hot path)

**Files:**
- Modify: `rill-lang/src/backend/interp.rs` (`remap_value_instr`; `exec_value_instr` to take a register offset; `run_call` and `run_value_track` call sites)
- Test: existing suite + `tests/thunk_unify.rs`

> **Design decision:** `remap_value_instr` (interp.rs:2506, ~174 lines) clones
> every `ValueInstr` on EVERY fragment call (`run_fragment`/`run_thunk_body` →
> `run_call`), and `ValueInstr` carries `Vec` payloads (`ValueConstructRecord`
> fields, `ValueConstructSum` payload, `ValueCallFunc`/`ValueCallLazy` args,
> `ValueListLit` elems, `ValueCallBuiltin` args) — so the clone allocates on the
> RT path. `run_value_track` (the program-level path) already avoids this by
> calling `exec_value_instr` with absolute registers. T2 gives fragments the
> same treatment: pass the `base` offset into `exec_value_instr` instead of
> cloning.

- [ ] **Step 1: Write a failing performance-guard test (optional) or a correctness test**

The behavior is unchanged; the guard is that a fragment call with a `Vec`-carrying
instruction still produces the right value. Add to `tests/thunk_unify.rs`:

```rust
#[test]
fn fragment_call_with_record_construction_still_works() {
    // Exercises a ValueConstructRecord (Vec payload) inside a fragment call —
    // the instruction must not be mis-remapped when base is threaded.
    let src = "{-# MODE complete #-}\n\
        mk = fn x -> Pair { first: x, second: x };\n\
        main = match (head (mk [1.0, 2.0])) of { Nothing => 0.0; Just x => x };";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = rill_core::traits::MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 1.0),
        other => panic!("expected Float(1.0), got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it passes before the change**

Run: `cargo test -p rill-lang --test thunk_unify`
Expected: PASS (baseline).

- [ ] **Step 3: Thread `base` into `exec_value_instr`**

Change `exec_value_instr(prog, instr, drops)` to
`exec_value_instr(prog, instr, base, drops)`, and inside, read every register
field as `reg + base` (dst/a/b/src/cell/closure_slot/args/fields/payload/elems)
instead of relying on a pre-remapped clone. Register that are NOT fragment-local
(main-track cells, `cell` capture indices, main-cell indices) stay unoffset —
mirror the current `remap_value_instr` comments.

- [ ] **Step 4: Update the call sites**

- `run_value_track` (program path, interp.rs:177): `base = 0`.
- `run_call` (the merged fragment runner): `base = <fragment's register base>` (the current `base` watermark), call `exec_value_instr(prog, instr, base, drops)` directly on the ORIGINAL `ValueInstr` — no clone.
- Delete `remap_value_instr` if nothing else uses it (check for other callers).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`
Expected: PASS — behavior unchanged, no clone on the fragment-call path.

- [ ] **Step 6: Clippy + fmt + commit**

Run: `cargo clippy --all-features -p rill-lang`, `cargo fmt`.
Commit:

```bash
cargo fmt
git add rill-lang/src/backend/interp.rs rill-lang/tests/thunk_unify.rs
git commit -m 'perf(rill-lang): thread register base into exec_value_instr — drop remap cloning'
```

---

## Self-review checklist

- [ ] §2.1 `forall` syntax (`TypeExpr::TForall`) → Task R1.
- [ ] §2.2 value-tyvar generalization (`free_vars`/`instantiate`) → Task R2.
- [ ] §2.3 `runST` rank-2 application, automatic escape rejection → Task R3.
- [ ] §3 Thunk/Closure unified into `Value::Callable` + single runner → Task T1.
- [ ] §4 ordering (A then B) — tasks are sequential; no overlap.
- [ ] §5 testing — each task has a test file; the two bypass regressions covered.
- [ ] Superseded one-point scan (`5b2c84d`) removed in Task R3.
- [ ] T2 (`remap_value_instr` without cloning) → fragment-call hot path, codebase prep for JIT and track unification.