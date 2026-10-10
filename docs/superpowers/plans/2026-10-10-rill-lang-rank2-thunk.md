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

## Task T1: Thunk trait — unify fragment runners

**Files:**
- Modify: `rill-lang/src/backend/interp.rs` (`Thunk` trait, `run_call`, merge `run_fragment`/`run_thunk_body`)
- Test: existing suite (no new behavior)

- [ ] **Step 1: Write a guard test**

Add to `rill-lang/tests/lazy_mode.rs` (or a new `tests/thunk_unify.rs`) a test that
exercises both eager and lazy paths so the merge is verified end-to-end:

```rust
#[test]
fn eager_and_lazy_calls_still_work() {
    // eager: a named function applied to args; lazy: a thunk over a closure.
    let src = "{-# MODE complete #-}\n\
        id2 = fn x -> x;\n\
        main = match (head (id2 [1.0, 2.0])) of { Nothing => 0.0; Just x => x };";
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

- [ ] **Step 2: Run to verify it passes before the merge**

Run: `cargo test -p rill-lang --test lazy_mode eager_and_lazy_calls_still_work`
Expected: PASS (baseline).

- [ ] **Step 3: Add the `Thunk` trait**

In `interp.rs`:

```rust
/// Common view of a callable value: the fragment to run and its captured env.
/// `Value::Closure` (eager) and `Value::Thunk` (lazy, memoized) both implement
/// it; the runner no longer branches on the value kind for fragment/env access.
trait Thunk {
    fn fragment(&self) -> u32;
    fn env(&self) -> ArenaRef;
}
```

Implement for `Value::Closure(env, fid)` and `Value::Thunk { fragment, env, .. }`.

- [ ] **Step 4: Merge `run_fragment` + `run_thunk_body` into `run_call`**

Create `run_call` with a parameter distinguishing the args source (caller
registers vs env-record tail split by `value_ins`). Keep steps 0-6 shared. The
function reads `fragment`/`env` through the `Thunk` trait where it currently
matches `Value::Closure`/`Value::Thunk`. Preserve the `max_call_regs` guard and
the memo semantics in `force()` (which stays Thunk-specific).

- [ ] **Step 5: Update call sites**

`ValueCallFunc` (interp.rs:987), `ValueCallLazy` (interp.rs:1011), `force`
(interp.rs:2233), `call_closure_single`/`call_closure_args` (interp.rs:2005/2031)
call `run_call` instead of the two merged functions. Remove the merged functions.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`
Expected: PASS — the full suite (incl. the guard test) is unchanged.

- [ ] **Step 7: Clippy + fmt + commit**

Run: `cargo clippy --all-features -p rill-lang`, `cargo fmt`.
Commit:

```bash
cargo fmt
git add rill-lang/src/backend/interp.rs rill-lang/tests/thunk_unify.rs
git commit -m 'refactor(rill-lang): Thunk trait — unify run_fragment/run_thunk_body'
```

---

## Self-review checklist

- [ ] §2.1 `forall` syntax (`TypeExpr::TForall`) → Task R1.
- [ ] §2.2 value-tyvar generalization (`free_vars`/`instantiate`) → Task R2.
- [ ] §2.3 `runST` rank-2 application, automatic escape rejection → Task R3.
- [ ] §3 Thunk trait + single runner → Task T1.
- [ ] §4 ordering (A then B) — tasks are sequential; no overlap.
- [ ] §5 testing — each task has a test file; the two bypass regressions covered.
- [ ] Superseded one-point scan (`5b2c84d`) removed in Task R3.