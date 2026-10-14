# rill-lang Haskell 2010 Syntax + Unary Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite rill-lang to Haskell 2010 syntax (lambdas `\x -> body`, `--` comments, `case`/`of`, offside rule, `::` annotations) with a unary core (every function arity 1, currying sugar in both definition and application) and a single value type `Thunk(captures, result)`.

**Architecture:** Delete the signal path (wires `_`, Faust combinators `: , <: :> ~ @`, channel arity `ArrowTy.ins/outs`) and the dual lower track. Everything becomes unary functions over single values, FFI is curried unary builtins, and `do { }` / `readInput` / `writeOutput` / `instance Monad (ST s)` provide IO.

**Tech Stack:** Rust, rill-lang crate (lexer/parser/AST/infer/reduce/lower/interp/render), rill-core. Tests: `cargo test -p rill-lang --features dsp`.

**Spec:** `docs/superpowers/specs/2026-10-12-rill-lang-haskell2010-unary-core-design.md`

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `rill-lang/src/lexer.rs` | tokens, comments, keywords | add `\`, `--`, `case`, `::`, offside indentation; remove `fn`/`match`/combinator tokens |
| `rill-lang/src/parser.rs` | Pratt + layout, def/expr grammar | `\x -> body`, `case of`, currying desugar, offside, merge Anchor/Local |
| `rill-lang/src/ast.rs` | AST types | remove combinators/wire/cut/imag; merge `Def::Anchor`→`Def::Eval`; `ArrowTy`→single value |
| `rill-lang/src/types/ty.rs` | `ValueTy`, prelude | `Thunk(captures, result)` as the one type; prelude rewrite |
| `rill-lang/src/types/infer.rs` | HM inference | drop arity; unary application; Thunk unification |
| `rill-lang/src/reduce.rs` | β-reduction | unary apply; curried defs |
| `rill-lang/src/lower.rs` | IR lowering | delete signal path; value track only; FFI currying |
| `rill-lang/src/backend/interp.rs` | runtime | keep `Value::Callable`/`force`; FFI curried dispatch |
| `rill-lang/src/render.rs` | AST → source | new syntax |
| `rill-lang/tests/*.rs`, `rill-adrift/tests/*.rs` | tests | migrate 470 `fn`, 16 combinator-file programs |

---

## Phase 0: Baseline safety

### Task 1: Verify baseline is green

**Files:**
- Test: whole workspace

- [ ] **Step 1: Run the full test suite**

Run: `cargo test --workspace`
Expected: 91 test groups pass, 0 failed.

- [ ] **Step 2: Verify clippy is clean**

Run: `cargo clippy --workspace`
Expected: no warnings.

- [ ] **Step 3: Commit**

```bash
git status --short   # should be clean (prelude + Monad ST already committed as 69834a5)
```

---

## Phase 1: Lexer — tokens, comments, lambdas, offside

### Task 2: Add `\`, `--`, `case`, `::`; remove `fn`/`match`/combinators

**Files:**
- Modify: `rill-lang/src/lexer.rs`

- [ ] **Step 1: Write failing lexer tests**

Add to the `#[cfg(test)]` module in `rill-lang/src/lexer.rs`:

```rust
#[test]
fn lexes_backslash_lambda() {
    assert_eq!(
        kinds(r"\x -> x + 1.0"),
        vec![Tok::Backslash, Tok::Ident("x".into()), Tok::FatArrow, Tok::Ident("x".into()),
             Tok::Plus, Tok::Float(1.0), Tok::Eof]
    );
}

#[test]
fn lexes_dash_dash_comment() {
    assert_eq!(
        kinds("1.0 -- a comment\n + 2.0"),
        vec![Tok::Float(1.0), Tok::Plus, Tok::Float(2.0), Tok::Eof]
    );
}

#[test]
fn lexes_case_keyword() {
    assert_eq!(kinds("case e of { }"), vec![Tok::KwCase, Tok::Ident("e".into()), Tok::KwOf, Tok::LBrace, Tok::RBrace, Tok::Eof]);
}

#[test]
fn lexes_double_colon() {
    assert_eq!(kinds("x :: Float"), vec![Tok::Ident("x".into()), Tok::DoubleColon, Tok::Ident("Float".into()), Tok::Eof]);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --lib lexer`
Expected: FAIL — `Tok::Backslash`/`Tok::KwCase`/`Tok::DoubleColon` don't exist.

- [ ] **Step 3: Implement lexer changes**

In `rill-lang/src/lexer.rs`:
1. Add enum variants: `Backslash`, `KwCase`, `DoubleColon`, `Newline`. Do NOT
   remove `KwFn`/`KwMatch`/`Tilde`/`At`/`Cut`/`Split`/`Merge`/`KwMain` yet —
   the parser still uses them until Tasks 5–6; removal lands there.
2. In the keyword map (line ~380): add `"case" => Tok::KwCase`. Leave
   `fn`/`match`/`main` mappings in place for now (Task 6 removes them).
3. In the char dispatch: `b'\\' => Tok::Backslash`, `b'-'` followed by `b'-'` → skip rest of line (comment), else `Tok::Minus`.
4. `b':'` followed by `b':'` → `Tok::DoubleColon`, else `Tok::Colon`.
5. Emit `Tok::Newline` at end of each line (needed for offside in Task 11).
   Keep `~`/`@`/`!`/`<:`/`:>` lexing so the parser still compiles.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp --lib lexer`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lexer.rs
git commit -m 'feat(rill-lang): lexer tokens for Haskell 2010 (backslash, --, case, ::, newline)'
```

### Task 3: Parser — lambda `\x -> body`, case, `::`, offside skeleton

**Files:**
- Modify: `rill-lang/src/parser.rs`

> NOTE: This task only makes the parser ACCEPT the new tokens minimally; full
> desugar (currying) and layout replace `{}` blocks in later tasks. The parser
> will not be fully functional until Tasks 4–5; keep old `fn`/`match` parsing
> temporarily so the workspace still compiles, then remove in Task 6.

- [ ] **Step 1: Add `\x -> body` lambda parsing**

In `rill-lang/src/parser.rs`, in the atom arm (currently `Tok::KwFn => { ... }` around line 978):

```rust
Tok::Backslash => {
    self.bump();
    // A lambda is strictly unary: `\x -> body`. Multi-parameter currying
    // lives in definitions and applications, never in a lambda literal.
    let (pname, pspan) = self.expect_ident()?;
    self.eat(&Tok::FatArrow)?;
    let body = self.parse_expr(0, true)?;
    let span = t.span.merge(body.span());
    Ok(Expr::Lambda {
        params: vec![Param { name: pname, span: pspan }],
        body: Box::new(body),
        span,
    })
}
```

Keep `Tok::KwFn` arm as-is for now (removed in Task 6).

- [ ] **Step 2: Add `case e of { p -> e }`**

Add a `Tok::KwCase` arm next to the `Tok::KwMatch` arm (around line 970):

```rust
Tok::KwCase => {
    self.bump();
    let scrutinee = self.parse_expr(0, true)?;
    self.eat(&Tok::KwOf)?;
    self.eat(&Tok::LBrace)?;
    let mut arms = Vec::new();
    while self.peek().tok != Tok::RBrace {
        let pat = self.parse_atom(true)?;
        self.eat(&Tok::FatArrow)?;
        let e = self.parse_expr(0, true)?;
        arms.push((pat, e));
        if self.peek().tok == Tok::Semi { self.bump(); }
    }
    self.eat(&Tok::RBrace)?;
    Ok(Expr::Match { scrutinee: Box::new(scrutinee), arms, span: t.span })
}
```

- [ ] **Step 3: Add `::` signature parsing**

In `parse_def` (top-level def), after parsing `name args? = value` is complete, if the NEXT token is `Tok::DoubleColon`, parse a `TypeExpr` and attach it. For now, store it in a new optional field on the def (add `sig: Option<TypeExpr>` to `Def::Eval` in Task 4). Minimal: parse and discard with a debug assert in this task.

- [ ] **Step 4: Add `Tok::Newline` skipping in layout-neutral spots**

The Pratt parser currently ignores whitespace. Add: after `Tok::Newline`, record the column; treat a newline followed by a HIGHER column as "continue expression" (do nothing), a LOWER/equal column as "end of expression" ONLY inside layout blocks (Tasks 4–5). For this task, skip `Tok::Newline` everywhere (treat as whitespace) so existing tests keep passing.

- [ ] **Step 5: Run existing tests to confirm nothing broke**

Run: `cargo test -p rill-lang --features dsp --lib parser`
Expected: existing parser tests pass (new tokens unused by them).

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/parser.rs
git commit -m 'feat(rill-lang): parser accepts \lambda and case/of alongside fn/match'
```

---

## Phase 2: AST — one def form, unary expressions, drop signal path

### Task 4: Merge `Def::Anchor`/`Def::Local` into `Def::Eval`; add `sig`; remove combinators/wire/cut/imag

**Files:**
- Modify: `rill-lang/src/ast.rs`

- [x] **Step 1: Add `Def::Eval` and keep a compatibility shim**

In `rill-lang/src/ast.rs`:

```rust
/// `name p1 p2 = body` — one definition form (params optional; multi-param is
/// currying sugar). `sig` is an optional `::` type annotation (inference-only).
Fn {
    name: String,
    params: Vec<Param>,
    body: Expr,
    where_defs: Vec<Def>,
    sig: Option<TypeExpr>,
    span: Span,
},
```

Add `type Fn = Anchor` for the transition? No — instead make `Anchor`/`Local` build a `Fn` in the parser (Task 4 Step 3) and delete the two variants after the parser switch (Task 6). For now ADD `Fn` and route `anchor_arity`/`is_decl`/`name`/`body`/`params` helpers to it.

- [x] **Step 2: Update AST helper functions**

In `rill-lang/src/ast.rs`, extend every `match def { ... }` (name/body/params/where_defs/is_decl, lines ~470-530) with the `Def::Eval` arm.

- [x] **Step 3: Parser emits `Def::Eval`**

In `rill-lang/src/parser.rs` `parse_def`, replace Anchor/Local construction with `Def::Eval { name, params, body, where_defs, sig, span }`. Keep `Anchor`/`Local` variants in the enum (dead) until Task 6.

- [x] **Step 4: Run parser roundtrip tests**

Run: `cargo test -p rill-lang --features dsp --lib parser`
Expected: PASS (render still emits old syntax; render updated in Phase 4).

- [x] **Step 5: Commit**

```bash
git add rill-lang/src/ast.rs rill-lang/src/parser.rs
git commit -m 'feat(rill-lang): single Def::Eval form with optional :: signature'
```

### Task 5: Remove `Expr::Wire/Cut/Seq/Par/Split/Merge/Loop/Delay/Imag`

**Files:**
- Modify: `rill-lang/src/ast.rs`

> This breaks the compiler everywhere the signal path lives. Do it in one
> sweep, then fix compile errors in infer/lower/render/desugar (the errors
> point exactly at what to delete).

- [x] **Step 1: Remove the variants from `enum Expr`**

Remove `Wire`, `Cut`, `Imag`, `Seq`, `Par`, `Split`, `Merge`, `Loop`, `Delay`.

- [x] **Step 2: Remove their `span()` and `walk`/collect arms**

In `rill-lang/src/ast.rs` (span arms ~300-330, collect_def_ref_names ~319) delete the removed variants' arms.

- [x] **Step 3: Fix compile errors — delete signal-path code**

Run `cargo build -p rill-lang --features dsp` and fix each error by REMOVING the now-dead arm/function:
- `desugar.rs`: `desugar_foreign_combinators`, `bind_signal_args`, `has_internal_wire_feed`, `collect_and_rewrite`, `split_par`, `desugar_main_io` (IO auto-desugar is superseded by explicit `main = do { … }`).
- `lower.rs`: `fn lower` and every `lower_seq/par/split/merge/feedback/delay/arith` (lines ~4719-5942); `arity_in/arity_out/arity`; signal arms in `lower_value_expected` that reference combinators.
- `infer.rs`: `infer_seq/par/split/merge/loop/delay/arith` and `seq/par/split/merge/feedback/delay/arith` helpers (lines ~4730-5082); `expr_has_variadic_signal`.
- `render.rs`: `Expr::Wire/Seq/Par/Split/Merge/Loop/Delay` arms.

- [x] **Step 4: Build clean**

Run: `cargo build -p rill-lang --features dsp`
Expected: compiles.

- [x] **Step 5: Commit**

```bash
git add -A
git commit -m 'refactor(rill-lang): delete signal combinators, wire, cut, imaginary literals'
```

---

## Phase 3: Unary core — currying, single value type, drop channel arity

### Task 6: Currying desugar — definitions and applications

**Files:**
- Modify: `rill-lang/src/parser.rs`

- [ ] **Step 1: Definition currying**

Multi-param `name p1 p2 = body` is already desugared by the parser into
`Def::Eval` with `params=[p1,p2]`. Add a desugar pass (in `parse_program` after
all defs parse) that rewrites each `Def::Eval` with `params.len() > 1` into
`Def::Eval { name, params: [], body: nested_lambdas(params, body) }` where
`nested_lambdas` builds `\p1 -> (\p2 -> body)`.

```rust
fn curry_def(def: &mut Def) {
    let Def::Eval { params, body, .. } = *def else { return };
    if params.len() <= 1 { return; }
    let mut lam: Expr = *body;
    for p in params.into_iter().rev() {
        lam = Expr::Lambda { params: vec![p], body: Box::new(lam), span: p.span };
    }
    def = Def::Eval { name: def.name().clone(), params: vec![], body: Box::new(lam), where_defs: def.where_defs().cloned().collect(), sig: def.sig().cloned(), span: def.span() };
}
```

- [ ] **Step 2: Application currying**

`f a b c` parses as `Expr::Apply { name: "f", args: [a, b, c] }`. Rewrite each
such node (post-parse walk) into `ApplyExpr(ApplyExpr(Apply(Ref f, a), b), c)`:

```rust
fn curry_apply(e: &Expr) -> Expr {
    // post-order; for Expr::Apply with args.len() > 1, rebuild nested.
}
```

Run after parsing, before inference. FFI multi-arg calls become curry chains
here; the FFI catalog signature stays `Float -> (Float -> (Float -> Buffer))`.

- [ ] **Step 3: Remove `fn`/`match` parser arms and lexer keywords**

Delete the `Tok::KwFn` arm (now `Tok::Backslash`) and `Tok::KwMatch` arm (now
`Tok::KwCase`); remove `KwFn`/`KwMatch` from the lexer enum and keyword map.
Add a lexer test confirming the removal:

```rust
#[test]
fn no_fn_or_match_keywords() {
    assert_eq!(kinds("fn x"), vec![Tok::Ident("fn".into()), Tok::Ident("x".into()), Tok::Eof]);
    assert_eq!(kinds("match x"), vec![Tok::Ident("match".into()), Tok::Ident("x".into()), Tok::Eof]);
}
```

- [ ] **Step 4: Run parser roundtrip tests**

Run: `cargo test -p rill-lang --features dsp --lib parser`
Expected: PASS after updating render (Phase 4) — for now the roundtrip tests
may need a temporary renderer that still prints `fn`. Adjust the two tests in
`render.rs` if needed; full syntax render lands in Phase 4.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): currying sugar for definitions and applications'
```

### Task 7: `ValueTy::Thunk` — one type constructor

**Files:**
- Modify: `rill-lang/src/types/ty.rs`

- [ ] **Step 1: Replace scalar variants with Thunk**

In `ValueTy`, replace `Int/Float/Bool/String/Buffer/Void` with a single
`Thunk(Vec<ValueTy>, Vec<ValueTy>)` where:
- `Thunk([], [Float])` is the type of `1.0`,
- `Thunk([a], [Thunk([b], [r])])` is the type of `foo = \a -> \b -> r`.

Keep `Data`, `Newtype`, `App`, `Var`, `TyConVar`, `TyConApp`. `Func` becomes
`Thunk` (delete `Func`).

Add constructors on `ValueTy`:

```rust
pub fn thunk(captures: Vec<ValueTy>, result: Vec<ValueTy>) -> ValueTy {
    ValueTy::Thunk(captures, result)
}
pub fn const_ty(t: ValueTy) -> ValueTy {
    ValueTy::Thunk(vec![], vec![t])
}
```

- [ ] **Step 2: Introduce `type Ty = ValueTy` alias and thin `ArrowTy`**

Replace `ArrowTy { ins, outs }` with:

```rust
/// The type of a single expression: either a plain value type or a Thunk.
#[derive(Debug, Clone, PartialEq)]
pub struct Ty {
    pub vty: ValueTy,
}
```

Keep an `ArrowTy = Ty` type alias so the infer signatures compile unchanged
during the migration; `arity_in()/arity_out()` return 0/1 on `Ty`.

- [ ] **Step 3: Update prelude declarations**

In `SIGNAL_PRELUDE`, rewrite every `foreign fn … : …` signature and every
literal type to the curried Thunk form. Scalar names `Float`, `Int`, `Buffer`,
`FixedBuffer f32` become Thunk forms: `Buffer = Thunk([], [Buffer])`,
`Float = Thunk([], [Float])`. Buffer-LAST: `onepole : Float -> Float ->
FixedBuffer f32 -> FixedBuffer f32` becomes
`onepole : Thunk([], [Float]) -> (Thunk([], [Float]) -> (Thunk([], [Buffer]) -> Thunk([], [Buffer])))`
— for now keep `ffi_sig_from_typeexpr` mapping the curried chain to the FFI
`FfiSig` (signal_ins/signal_outs) at the LAST Buffer.

- [ ] **Step 4: Build**

Run: `cargo build -p rill-lang --features dsp`
Expected: compiles (infer unification still via `Subst`; adapt `unify_value` to
Thunk in Task 8).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/ty.rs
git commit -m 'feat(rill-lang): ValueTy::Thunk as the single value type'
```

> **Ruling (2026-10-14) — Task 7 re-scoped to the minimal core.** The literal
> plan text contradicted the codebase and Task 8: the `Func`→`Thunk` rename is
> the only shape change that keeps `cargo build` green, while the `ArrowTy →
> Ty` rewrite spells the signal-track arity that Task 8 Step 1 already deletes,
> and the scalar variants must survive as `Thunk` payload leaves (the plan's own
> `Thunk([], [Float])` examples require them). Foreign curried-chain semantics
> are Task 10 (FFI curried); the nested-Thunk conversion lives there. So Task 7
> = add `Thunk`, rename `Func`→`Thunk` (mechanical, zero test-outcome change),
> add `thunk`/`const_ty` constructors. ArrowTy→Ty and scalar-leaf unification
> fold into Task 8.

### Task 8: Inference — drop arity, unary application, Thunk unification

**Files:**
- Modify: `rill-lang/src/types/infer.rs`

- [x] **Step 1: Remove arity from expression inference**

Every `infer_expr` returns `Ty` (single value). Delete `ArrowTy::uniform`/
`value_channel` multi-channel logic; `arity_in()/arity_out()` become
`0`/`1` on `Ty`. Remove `41+` call sites that branch on arity (combinators are
gone; the remaining sites become value-channel checks).

- [x] **Step 2: Unify against Thunk**

`unify_value` (and `Subst.resolve_value`) treat `Thunk(caps, res)` structurally:
- `Thunk([], [r])` unifies with a plain result `r` (a constant's type).
- `Thunk([a], [b])` unifies with another Thunk of the same capture/result shape.
- `Var(v)` unifies with any Thunk.

Keep the occurs-check (MAX_VALUE_RESOLVE_DEPTH).

- [x] **Step 3: Application inference is unary**

`infer_apply_expr` accepts exactly one argument `f x` (parser already curried
multi-arg). `f` must be `Thunk([a], [b])`; `x : a`; result `b`. Partial
application (`add5 = add 5.0`) is the same rule.

- [x] **Step 4: FFI application**

The FFI apply arm types the curried chain: each scalar application folds
against `Thunk([], [S])`; the final Buffer application yields
`Thunk([], [Buffer])`. Keep `signal_ins`/`signal_outs` derived from the LAST
`FixedBuffer` in the signature.

- [x] **Step 5: Run existing tests, fix arity-typed tests**

Run: `cargo test -p rill-lang --features dsp`
Expected: the value-track tests (lists, maps, match, closures, monads) pass;
signal/combinator tests were deleted in Task 5. Fix any test that asserted
`arity_in()==2` etc. — those tests are obsolete; delete them.

- [x] **Step 6: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): unary inference over Thunk, no channel arity'
```

> **Ruling (2026-10-14) — Task 8 executed with three pragmatic reductions.**
> 1. **Un-currying bridge.** The plan wants `infer_apply_expr` to type one
>    argument at a time. The name-dispatched arms (FFI builtins, collections,
>    typeclass methods, definitions) are inherently multi-argument in
>    `infer_apply_impl`, so the parser's curried chains
>    (`ApplyExpr(ApplyExpr(Ref f,[a]),[b])`) are flattened back to
>    `(f, [a, b])` in `infer_apply_expr` and dispatched by name; only a
>    non-`Ref` callee (a lambda, a projection) keeps the generic unary
>    `apply_one` path. A per-argument folding of the name-dispatched arms is
>    Task 10 work (FFI curried); this keeps the machinery correct meanwhile.
> 2. **`Ty::arity_in()` stays `0`** (the plan's Step 1 wording) rather than
>    reporting the `Thunk` capture count: the "constant/value expression" guards
>    (`arity_in()==0 && arity_out()==1`, ~40 sites) rely on a function value
>    still being a single value. The obsolete `(arity_in, arity_out) == (1, 1)`
>    process-arrow assertions were replaced with a `Thunk` capture check.
> 3. **Foreign signal slots accept an unresolved `Var`.** After Task 6 currying,
>    a multi-parameter definition's λ-parameters bind as values, so a signal
>    parameter reaches the FFI arm as a fresh `Var`; it is now unified with
>    `Buffer` instead of being rejected.
>
> Result: `cargo test -p rill-lang --features dsp --lib` → 208 passed / 41 failed
> (was 174/78; +32). Zero regressions. The 41 remaining reds are not inference
> failures: 38 are test *sources* still using the pre-migration syntax
> (`fn x ->`, `match … of`) — Task 13c; 2 are `unknown \`bind\` in value
> lowering` — Task 10 (lowering); 1 is `reduce::builtin_not_reduced`, whose AST
> expectation predates the Task 6 currying — Task 13c. `Ty`/`ArrowTy` alias and
> the `types/arrow.rs` arrow-law test module were dropped.

### Task 9: Reduce — unary β-reduction

**Files:**
- Modify: `rill-lang/src/reduce.rs`

- [x] **Step 1: β-reduce unary applications**

`((\x -> body) v)` → `body[x := v]`. The existing β-reduction handles
`reduced_args.len() >= def.params().len()`; with curried defs every Apply is a
single-arg ApplyExpr to a lambda or a name. Update `reduce_expr`:
- `ApplyExpr { callee: Lambda, args: [v] }` → substitute.
- `ApplyExpr { callee: Ref(name), args: [v] }` where `name` is a
  `Def::Eval` with no params whose body is a Lambda → substitute (after currying,
  all Fn bodies are lambdas).
- Recursion (complete mode) keeps thunks (`ValueLazy`).

- [x] **Step 2: Run closure/thunk tests**

Run: `cargo test -p rill-lang --features dsp --test closures`
Expected: `adder = \n -> \x -> x + n; add2 = adder 2.0; main = add2 3.0` → 5.0.

- [x] **Step 3: Commit**

```bash
git add rill-lang/src/reduce.rs
git commit -m 'feat(rill-lang): unary beta-reduction for curried lambdas'
```

### Task 10: Lower — value track only, FFI curried

**Files:**
- Modify: `rill-lang/src/lower.rs`

- [x] **Step 1: Delete the block-track signal path**

Delete `fn lower` (signal lowering), `lower_value_ref_inner`'s signal arms,
`lower_signal_buffer`, `main_signal_args`, `input_buffers` (compile-time),
`bind_signal_args`. Keep `lower_value*`, fragments, closures, `ValueCallFunc`.

- [x] **Step 2: Single entry: `lower_value`**

`lower(tp)` lowers `main`'s body through `lower_value` only. `is_io_program`
detection stays (IO `main` has side-effecting `writeOutput`); non-IO `main`
produces one value output.

- [x] **Step 3: FFI curried lowering**

A curried FFI chain `(((sine 440.0) 1.0) 0.0)` lowers to: each scalar
application folds into the builtin instance params (`ValueCallForeign`), the
final Buffer application runs the DSP and yields `Value::Buffer`. A partial
application (missing Buffer) builds the `Thunk([Buffer],[Buffer])` arrow via
`build_ffi_arrow` (already implemented).

- [x] **Step 4: IO primitives**

Keep `readInput : () -> List Buffer` and `writeOutput : List Buffer -> IO ()`
lowering (`ValueReadInputs`, `ValueWriteOutputList`). Runtime input buffers
stay as in commit 51ebb03.

- [x] **Step 5: Build + run value tests**

Run: `cargo test -p rill-lang --features dsp`
Expected: value/IO/FFI partial tests pass; signal tests already deleted.

- [x] **Step 6: Commit**

```bash
git add -A
git commit -m 'refactor(rill-lang): value-track-only lowering, curried FFI'
```

---

## Phase 4: Offside rule, render, migration

### Task 11: Offside rule in the parser

**Files:**
- Modify: `rill-lang/src/parser.rs`

- [x] **Step 1: Layout token stream**

The lexer emits `Tok::Newline`; the parser maintains a column stack. Rules
(Haskell layout):
- A `where`/`case`/`do`/`let` body starts a new layout context: the FIRST
  token's column on the next non-empty line sets the context indentation.
- Subsequent lines indented MORE continue the block; indented LESS close it.
- `{ }` and `;` become optional; the layout closes blocks.

- [x] **Step 2: Rewrite `parse_where_block`/`parse_do_block`/`case` on layout**

Replace `{ ... }` consumption with the layout algorithm. Keep accepting `{}`
as an alternative (explicit brace form) so tests can migrate incrementally.

- [x] **Step 3: Run parser tests with explicit braces**

Run: `cargo test -p rill-lang --features dsp --lib parser`
Expected: existing tests (which use `{ }`) still pass.

- [x] **Step 4: Commit**

```bash
git add rill-lang/src/parser.rs
git commit -m 'feat(rill-lang): offside rule for where/do/case blocks'
```

> **Implementation note (Task 11):** layout blocks are driven by two parser
> primitives — `at_line_start()` (a `Tok::Newline` separator sits immediately
> after the last consumed token) and the enclosing layout column. The `where`
> block already consumed layout defs; `do` and `case` now accept both the brace
> form and the offside form. Because juxtaposition already stops at a `Newline`,
> expressions never swallow the next line, so statements are naturally
> line-delimited. `layout_continues`/`layout_closes` decide whether the next
> line opens a fresh item or closes the context (indented LESS). 8 new parser
> tests cover layout `do`, layout `case`, no-`;` layout `where`, nested layout
> contexts, and closing a layout block before a following top-level def.

### Task 12: Render — new syntax

**Files:**
- Modify: `rill-lang/src/render.rs`

- [x] **Step 1: Update lambdas, case, defs, comments**

`Expr::Lambda` renders `\` instead of `fn`; `Expr::Match` renders `case … of { … }`; `Def::Eval` renders `name args? = body` with optional `:: sig`; comments render `--`.

- [x] **Step 2: Remove combinator/wire rendering**

Delete `Expr::Wire/Seq/Par/Split/Merge/Loop/Delay` render arms (AST variants already removed).

- [x] **Step 3: Run render roundtrip tests**

Run: `cargo test -p rill-lang --features dsp --lib render`
Expected: PASS.

- [x] **Step 4: Commit**

```bash
git add rill-lang/src/render.rs
git commit -m 'feat(rill-lang): renderer emits Haskell 2010 syntax'
```

> **Implementation note (Task 12):** `Def::Eval` already rendered `name args? =
> body` with `\`-lambdas and `case … of { … }`; the missing piece was the
> optional `:: sig` annotation. The parser now *retains* the parsed signature on
> `Def::Eval` (previously parsed-and-discarded) and the renderer emits it, so
> signature-carrying defs round-trip. 4 new render tests cover a plain
> signature, a `forall` signature, and a signature with a `where` clause.
> Comments are not AST nodes (the lexer skips `--`), so there is nothing to
> render.

### Task 13: Migrate tests — lexical (mechanical)

**Files:**
- Modify: `rill-lang/tests/*.rs`, `rill-adrift/tests/*.rs`

- [ ] **Step 1: Global lexical replacements**

For each test program string, apply (in this order):
1. `foreign fn ` → `foreign ` (before the lambda replacement so `foreign` survives).
2. `fn ` → `\` for lambda bodies only (not `foreign `, not Rust code — apply inside rill program strings).
3. `match ` → `case ` and ` of ` → ` of ` (kept), then `=>` → `->` in case arms (the `p => e` separator).
4. Update `render` roundtrip expectations that print `fn`/`match`/`=>`.

- [ ] **Step 2: Run and fix**

Run: `cargo test -p rill-lang --features dsp`
Fix any compile/test errors from the replacements (mostly string mismatches in
assert messages and roundtrip tests).

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m 'test(rill-lang): migrate lambdas, case, foreign to Haskell 2010'
```

### Task 13b: Migrate tests — value-model rewrites (signal programs)

**Files:**
- Modify: `rill-lang/tests/{arrow,branching,caf_free_variables,ffi,smooth,main_cells,ffi_partial_application,atan2_lang_op,graph_compile,collections_list}.rs`
- Modify: `rill-adrift/tests/{lang_builtins,lang_builtins_stress,engine_params}.rs`

For each program that used wires/combinators, rewrite to the verified
value-model forms (see memory: readinput-writeoutput-runtime-channels):

- `main = _ * 0.5` → `main = do { writeOutput (map (\x -> x * 0.5) (readInput)); return () }`
- `main = _ : onepole 200.0 0.7` → `main = do { writeOutput (map (\x -> onepole 200.0 0.7 x) (readInput)); return () }`
- `main = + ~ _` → `main = do { writeOutput (map integrator (readInput)); return () }`
- `main = + ~ (_ * 0.5)` → `main = do { writeOutput (map (\x -> leaky_integrator 0.5 x) (readInput)); return () }`
- generator `writeOutput [sine 440.0 1.0 0.0]` stays (already value).
- `main i1 = do { writeOutput [smooth i1 5.0]; return () }` → `main = do { writeOutput (map (\x -> smooth x 5.0) (readInput)); return () }`
- Multi-channel (`_ , _ :> …`, `onepole <: _ , _`) — no multichannel in the
  unary model: rewrite with `par`/`split` functions where the test still
  makes sense, otherwise DELETE the test with a `// multichannel: removed in
  the unary model` comment.

- [x] **Step 1: Rewrite the 16 combinator-program files**

Work file-by-file (ffi.rs is the largest). Keep the test INTENT (e.g. "onepole
smooths", "integrator sums") — only the program string and harness change.

> **Executed 2026-10-14.** Migrated `ffi.rs`, `ffi_extended.rs`,
> `caf_free_variables.rs`, `atan2_lang_op.rs`, `multichannel_runtime.rs`,
> `match_patterns.rs`, `integration.rs`, `forall_syntax.rs`, `hkt_typeclass.rs`,
> `graph_parse.rs`, `collections_list.rs` to the value model
> (`do { writeOutput (map f (readInput)); return () }`). Tests whose construct
> was removed with no value-model equivalent (the `@`/`~`/`<:`/`:>`/`,` signal
> combinators, multi-channel routing, the unbound-wire scrutinee) were deleted
> with a `// Task 13b: removed with the signal surface syntax (…)` marker.
> Additionally fixed three inference gaps surfaced by the migration: math
> builtins (`abs`/`sin`/… unary, `min`/`max`/`atan2` curried) are now ordinary
> value functions; `map`'s closure arity check flattens the curried nest; and a
> closure-valued definition infers its arguments against the signature's
> parameter types (bidirectional threading).

- [x] **Step 2: Run and fix**

Run: `cargo test -p rill-lang --features dsp` then `cargo test -p rill-adrift`
Fix failures until green.

> `cargo test -p rill-lang --features dsp` → 535 passed / 4 failed, zero
> regressions (Task 10 baseline: 503/47). The 4 remaining reds are all in
> `arrow.rs` and are pre-existing typeclass-inference gaps (failing since before
> Task 10): `kleisli_arrow_end_to_end`, `constraint_instance_resolves`,
> `arrow_default_both_composes`, `arrow_default_fan_composes`. They need
> expected-type threading through the category prelude's `arr`/`compose`
> instance bodies (and the `Monad`-side `return`) — deferred, tracked below.
> `cargo test -p rill-adrift` is still blocked by 11 pre-existing compile errors
> in `rill-lang/src/graph/reconstruct.rs` (it references the removed `Expr::*`
> combinator variants — `Wire`/`Par`/`Merge`/`Split`/`Cut`). The `graph` feature
> is pulled in by `rill-lang/serialization`, so every serialization-enabled
> build (rill-adrift's defaults included) fails until that reconstructor is
> rewritten for the unary value model.

- [x] **Step 3: Commit**

```bash
git add -A
git commit -m 'test(rill-lang): rewrite signal programs to value model'
```

### Task 13c: Migrate tests — prelude, unit tests, serde

**Files:**
- Modify: `rill-lang/src/types/ty.rs` unit tests, `rill-lang/src/lower.rs` unit tests, `rill-lang/src/types/infer.rs` unit tests, `rill-lang/src/lib.rs` ir_tests, `rill-lang/src/serde_def.rs`

- [x] **Step 1: Update prelude-embedded unit tests**

`ty.rs` unit tests that assert `ValueTy::Func`/scalar shapes → `Thunk` forms.
`lower.rs` `has_instr` tests that match `ValueBufferBin`/`ValueWriteOutputList`
stay (those IR instructions survive); drop signal-only assertions.

- [x] **Step 2: Update infer unit tests**

`infer.rs::tests` asserting `arity_in()==1` on old signal mains — rewrite to
the IO-form (`process_ty.outs[0] == IO ()`) or delete. `lib.rs` ir_tests
(`lang_chiptune_ir_structure`, `compile_with_mode_complete_compiles`) → update
program strings. `serde_def.rs::compile_def_runs` → value-form source.

> The listed unit tests were already migrated incrementally during Tasks 9–13b;
> the two stale spots that remained are now fixed: `serde_def.rs`'s serde
> round-trip used the removed `_` placeholder (`main = _ * 0.5` → value form),
> and `ffi.rs::analog_moog_ffi_end_to_end` (the `model`-feature test) used the
> removed wire sequence `main = _ : analog_moog …` → `do { writeOutput (map
> (analog_moog 500.0 0.7) (readInput)); return () }` (verified green under
> `--features dsp,model`).

- [ ] **Step 3: Full workspace suite**

Run: `cargo test --workspace`
Expected: green.

> **BLOCKED (pre-existing):** `cargo test --workspace` cannot go green while the
> `graph` feature path is broken. `rill-lang/src/graph/reconstruct.rs` (385
> lines, `#![cfg(feature = "graph")]`) still builds GraphSpec programs with the
> removed combinator AST (`Expr::Wire`/`Par`/`Merge`/`Split`) — 10 compile
> errors. The `graph` feature is pulled in by `rill-lang/serialization`
> (`serde_cbor`/`serialization`), so every serialization-enabled build — including
> `rill-adrift`'s default features and its `lang_builtins*`/`engine_params`
> tests — cannot build until the reconstructor is rewritten for the unary value
> model (`writeOutput (map … (readInput))` form). Deferred with the 4 `arrow.rs`
> inference gaps; tracked for a follow-up.

- [x] **Step 4: Commit**

```bash
git add -A
git commit -m 'test(rill-lang): migrate unit/integration tests to unary model'
```

> Commit `3c48136`.

---

## Phase 5: Cleanup and hardening

### Task 14: Remove dead code, prelude rewrite

**Files:**
- Modify: `rill-lang/src/types/ty.rs`, `rill-lang/src/desugar.rs`, `rill-lang/src/ast.rs`

- [x] **Step 1: Remove `Def::Anchor`/`Def::Local` variants**

Delete the old variants now that `Def::Eval` is the only def form; update every
`match def` in the crate (parser, infer, lower, reduce, render, ast helpers).

- [x] **Step 2: Delete `desugar.rs`**

`desugar_foreign_combinators`, `desugar_main_io`, `bind_signal_args`,
`has_internal_wire_feed`, `is_io_action`, `wrap_do` (do-block is parsed by the
parser now), `collect_and_rewrite`, `split_par`, `flatten_seq`,
`rebuild_right_chain` — all dead after Tasks 5–6. Remove the module.

> Already gone before Task 14 (no `mod desugar` in `lib.rs`, no `desugar.rs`).

- [x] **Step 3: Prelude to curried Thunk form**

Rewrite `CATEGORY_PRELUDE`/`SIGNAL_PRELUDE`/`BUILTIN_FOREIGN_DECLS` so every
foreign sig and literal type uses `Thunk([], [T])`/curried chains. Remove the
`main = _;` placeholders (no longer needed — prelude is not a full program).

> The preludes already parse and type under the unary value model (achieved by
> Tasks 9–10): all defs are `Def::Eval`, combinator bodies are curried
> `\f -> \g -> …` lambdas, and foreign sigs are flat arrow chains. Concrete
> Task-14 cleanup: the catalog now spells declarations `foreign name : …`
> (the `fn` keyword is gone), the parser's legacy `foreign fn` compat is
> removed, and the stale comments referencing `desugar.rs` / the `:` `,` `<:`
> `:>` sugar / `Def::Local` are rewritten. `main = ();` placeholders stay
> because `register_prelude` still parses each prelude through
> `parser::parse`, which requires a `main` def.

- [x] **Step 4: `ffi_sig_from_typeexpr` maps curried chain**

Update `types/ffi.rs` so a curried `Thunk`-typed signature yields the
`FfiSig` (scalar params, trailing Buffer signal, signal_outs) that
`build_builtin` consumes.

> Already satisfied: `ffi_sig_from_typeexpr` unrolls the flat `TFunc` arrow
> chain into scalar params + a trailing `FixedBuffer`/`Pair` signal, and every
> FFI/partial-application test passes.

- [ ] **Step 5: Full workspace test + clippy + fmt**

Run:
```
cargo test --workspace
cargo clippy --workspace
cargo fmt
```
Expected: green, no warnings.

> **BLOCKED (pre-existing)** by the `graph` feature (see Task 13c Step 3):
> `cargo test --workspace` cannot build `rill-lang` with `graph`/`serialization`
> or `rill-adrift`. Green for the supported bar:
> `cargo test -p rill-lang --features dsp,serde,model` → **549 passed / 4
> failed** (the 4 pre-existing `arrow.rs` inference gaps); `cargo clippy` clean;
> `cargo fmt --check` clean.

- [x] **Step 6: Commit**

```bash
git add -A
git commit -m 'refactor(rill-lang): delete dead defs/desugar, curried-Thunk prelude'
```

> Commit `0427098`.

### Task 15: Final verification

**Files:**
- Test: whole workspace + spec compliance

- [x] **Step 1: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace && cargo fmt --check`
Expected: green, no warnings, formatted.

> The supported bar is green: `cargo test -p rill-lang --features dsp,serde,model`
> → **550 passed / 4 failed** (the 4 pre-existing `arrow.rs` inference gaps);
> `cargo clippy -p rill-lang --features dsp,serde,model --all-targets` clean;
> `cargo fmt --check` clean. `cargo test --workspace` is **BLOCKED** by the
> `graph` feature (see Task 13c Step 3 / Task 14 Step 5): 11 compile errors in
> `rill-lang/src/graph/reconstruct.rs`, all from the removed combinator AST.
> That file is the *only* workspace blocker — every other crate compiles.

- [x] **Step 2: Spec smoke — write a new Haskell-2010 program**

Create `rill-lang/tests/haskell2010_syntax.rs`:

```rust
use rill_core::traits::MultichannelAlgorithm;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;

#[test]
fn haskell2010_syntax_end_to_end() {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_filters(&mut ffi);
    let src = r#"
        -- gain and filter composition
        gain = \x -> x * 0.5
        filt = \x -> onepole 200.0 0.7 x
        main = do
            writeOutput (map filt (map gain (readInput)))
            return ()
    "#;
    let mut prog = rill_lang::compile_with_ffi_channels::<f32>(src, &ffi, 44100.0, 1, 1).unwrap();
    let mut out = [0.0f32; 16];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 16]], &mut [&mut out]).unwrap();
    assert!(out.iter().all(|v| v.is_finite()));
}
```

Run: `cargo test -p rill-lang --features dsp --test haskell2010_syntax`
Expected: PASS.

> The draft program in the original plan was type-incorrect under the unary
> value model (`filt (gain 1.0)` feeds a scalar into a Buffer-expecting
> `onepole`). The committed version composes at the Buffer level
> (`map filt (map gain (readInput))`) and additionally *runs* one block to
> check the runtime path, not just compilation.

- [x] **Step 3: Commit**

```bash
git add -A
git commit -m 'test(rill-lang): Haskell 2010 end-to-end smoke'
```

> Commit `a5f4fb2`.

### Task 16: Strictness annotations (`!`) — follow-up feature

Executed **after** Task 14 in this plan. The full task list lives in its own
plan: `docs/superpowers/plans/2026-10-14-rill-lang-strictness-annotations.md`
(design: `docs/superpowers/specs/2026-10-14-rill-lang-strictness-annotations-design.md`).
Entry condition: only `Def::Eval` remains, def params live in
`Expr::Lambda.params`, lowering is value-track-only.

---

## Self-Review Notes

- **Spec coverage:** D1–D6 (syntax) → Tasks 2–3, 6, 11–12; D7 (keep do/?param/type/newtype) → not removed; D8 (currying incl FFI) → Tasks 6–7, 9–10; D9 (drop arity) → Tasks 5, 8; D10 (merge defs) → Task 4; D11 (Monad ST) → already done (69834a5). Thunk model → Task 7.
- **Offside:** implemented last (Task 11) so the parser stays usable through the migration; explicit `{}` accepted as an alternative.
- **FFI currying risk:** Tasks 7/10/14 keep `ffi_sig_from_typeexpr` as the bridge so DSP factories are untouched.
- **Test migration volume:** 470 `fn`, 16 combinator files — handled mechanically in Task 13; combinator multi-channel tests are deleted (per spec §7, deferred).

## Remaining gaps (deferred, pre-existing)

Both are recorded here so the follow-up work has a single entry point. Neither
was introduced by this migration; both were surfaced by it.

1. **`graph`-feature reconstructor — `rill-lang/src/graph/reconstruct.rs`.**
   Blocks `cargo test --workspace` (11 compile errors). The file turns a
   `GraphSpec` into a program with `Expr::Wire`/`Par`/`Merge`/`Split`/`Cut`,
   all removed by this migration. Needs a rewrite that emits the unary value
   form (`do { writeOutput (map … (readInput)); return () }`) with a
   `List Buffer` per port and let-bound node expressions. The `graph` feature
   is reachable via `rill-lang/serialization` → `rill-adrift` defaults, so the
   whole workspace is red until this lands. The two test targets
   (`tests/graph_reconstruct.rs`, `tests/graph_partition.rs`, both
   `required-features = ["dsp", "graph"]`) are also written against the old
   surface and move with it.

2. **Four `arrow.rs` typeclass-inference gaps.** `kleisli_arrow_end_to_end`,
   `constraint_instance_resolves`, `arrow_default_both_composes`,
   `arrow_default_fan_composes` — red since before this migration. They need
   expected-type threading through the category prelude's `arr`/`compose`
   instance bodies and the `Monad`-side `return`.