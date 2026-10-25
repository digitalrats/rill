# Haskell 2010 Conformance — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the residual Haskell 2010 syntax gaps (S1–S12) and the lazy-list gap so `from`/`take` work over lazy `Cons` chains, preparing rill for the GHC-parity benchmark.

**Architecture:** Four ordered phases. Phase A fixes the **blockers** the design review found (Prelude recursion vs Strict mode, Prelude shadowing, capacity accounting) because every later phase depends on them. Phase B adds the lexer/parser constructs. Phase C makes list operations representation-agnostic and lazy where Haskell 2010 is lazy. Phase D adds `from`/`take` to the Prelude, operator values, and the acceptance tests.

**Tech Stack:** Rust, `rill-lang` crate; tests via `cargo test -p rill-lang --features dsp`.

**Spec:** `docs/superpowers/specs/2026-10-24-rill-lang-haskell2010-conformance-design.md`

## Global Constraints

- **North star:** Haskell 2010 semantics. A construct that Haskell 2010 defines must behave with the same meaning; there is no "rill-preferred" alternative.
- **Do not reopen** D1–D11 of `2026-10-12-rill-lang-haskell2010-unary-core-design.md`.
- **No `unsafe`** (`#![deny(unsafe_code)]` in `rill-lang`).
- **Existing tests must pass unchanged**, with one deliberate exception: nothing may change `deep_recursion_parity` (it pins user-vs-Prelude shadowing).
- Gates per task: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp`, `cargo fmt --check`.
- English docs/comments; commit messages single-quoted.
- Prefix every commit with `feat(lang):`/`fix(lang):`/`test(lang):`.

## Review Focus

1. **User recursion still rejected in Strict mode** — scoping `check_recursion` to Prelude defs must not accidentally let a user's recursive function through. Pinned by an existing Strict-mode test plus a new one.
2. **User definition beats Prelude definition** — before and after phases, `deep_recursion_parity` must produce `10.0 11.0 12.0`; a silent mistyping (inference uses Prelude scheme, lowering runs user body) is the failure mode.
3. **`take` terminates on an infinite `from`** — the demand-limiting property; a timeout or hang is the failure. Test with a bounded `n` (5) so a bug shows as a hang, not as a slow pass.
4. **Lazy producers stay lazy** — `take 5 (map f (from 1))` must terminate; if `map` materialises a strict `Value::List`, it hangs.
5. **A construct that "parses" but evaluates wrong** — every S-row test asserts a computed result, never just `compile().is_ok()`.
6. **Existing list behaviour unchanged** — `Value::List` paths keep their results (`length`, `map`, `fold` over literals and `listRange`).
7. **Capacity wall** — the benchmark's `n = 64` list size works; a smaller accidental wall (e.g. 20) is a defect.

---

## File Structure

| File | Responsibility |
|---|---|
| `rill-lang/src/types/infer.rs` | Prelude merge, `check_recursion` scoping, shadowing filter, guard totality |
| `rill-lang/src/lower.rs` | Prelude merge for lowering, `let` bindings, capacity accounting |
| `rill-lang/src/lexer.rs` | Backtick, `++`, nested block comments |
| `rill-lang/src/parser.rs` | `where` capture/layout, function guards, sections, arg patterns, standalone sigs, list comprehensions |
| `rill-lang/src/ast.rs` | Any new expression nodes (sections may reuse lambdas; comprehensions desugar) |
| `rill-lang/src/backend/interp.rs` | Representation-agnostic list ops, lazy producers, demand-limited `take` |
| `rill-lang/src/types/ty.rs` | `STD_PRELUDE` additions (`from`, `take`, `otherwise`) |
| `rill-lang/tests/haskell2010_syntax_gaps.rs` | S1–S12 tests |
| `rill-lang/tests/lazy_lists.rs` | Lazy list tests |

---

### Task 1: Blockers — Prelude recursion, shadowing, capacity

**Files:**
- Modify: `rill-lang/src/types/infer.rs` (`infer_program_with` ~1799–1810, `check_recursion` call ~1893, `prelude_names` ~1922–1935)
- Modify: `rill-lang/src/lower.rs` (`defs` map ~8149–8153)
- Test: `rill-lang/tests/prelude_shadowing.rs` (new), `rill-lang/tests/deep_recursion_parity.rs` (unchanged, must pass)

**Interfaces:**
- Consumes: nothing.
- Produces: a single shadowing rule available to both phases; `check_recursion` applied to user defs only.

- [ ] **Step 1: Write the failing test**

`rill-lang/tests/prelude_shadowing.rs`:
```rust
use rill_lang::{compile, ffi::ForeignRegistry};

#[test]
fn strict_mode_still_rejects_user_recursion() {
    let src = r#"loop n = loop n; main = do { write (Just "x") stdout; return () };"#;
    assert!(compile::<f32, 256>(src, ForeignRegistry::new()).is_err());
}
```

The user-vs-Prelude shadowing assertion is **not** written here: with no Prelude
`take` yet, the test cannot fail for the right reason. It is written and wired in
Task 5, Step 1 (`user_take_shadows_the_prelude_take`), once the Prelude `take`
exists. Task 1's regression evidence for B2 is `deep_recursion_parity` (Step 5).

- [ ] **Step 2: Fix B1 — scope `check_recursion` to user defs**

In `infer_program_with`, move the `check_recursion(&program.defs)` call **before** the block that prepends `prelude_defs`, or filter prelude names out of the list passed to it. Both give the same semantics; prefer the filter form so the check always sees the final user def list:

```rust
if mode == Mode::Strict {
    let user_only: Vec<_> = program.defs.iter()
        .filter(|d| !prelude_names.contains(d.name()))
        .cloned().collect();
    check_recursion(&user_only)?;
}
```
(`prelude_names` must be computed before the check; hoist it.)

- [ ] **Step 3: Verify B1 with a recursive Prelude def**

Temporarily add `from n = cons n (from (n + 1.0));` to `STD_PRELUDE`, then:
Run: `cargo test -p rill-lang --features dsp`
Expected: green (before the fix: 69+ failures "recursive function call"). Keep the Prelude line (Task 5 needs it) but ensure `from` is not yet exported if that complicates typing — the point is only that Strict mode no longer rejects it.

- [ ] **Step 4: Fix B2 — one shadowing rule in both phases**

Inference: delete the `prelude_names` filter that excludes user defs from `infer_def_group`; instead, **drop prelude defs whose name is also a user def** before merging. Lowering: build its `defs` map from the same merged list (prelude defs first, user defs last) so the last-wins behaviour equals the shadowing rule deliberately, not accidentally.

- [ ] **Step 5: Run the shadowing regression**

Run: `cargo test -p rill-lang --features dsp deep_recursion_parity`
Expected: `["10.011.012.0"]`.
Run: `cargo test -p rill-lang --features dsp`
Expected: green.

- [ ] **Step 6: Address B3 — capacity for recursive chains**

Read `lower.rs`'s capacity accounting (~8292–8330). The requirement: a recursive Prelude def must not inflate the *per-run* arena budget beyond what demand needs. If the accounting charges per call site (as it does), a recursive def with one allocation site is already O(1); verify by compiling `take 5 (from 1)` and running it. Record the measured working `n` in a comment; if `n = 64` fails, the plan's later task (Task 5, benchmark size) is the gate.

- [ ] **Step 7: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/tests/prelude_shadowing.rs
git commit -m 'fix(lang): scope recursion check to user defs and unify prelude shadowing'
```

---

### Task 2: Lexer — backticks, `++`, nested block comments

**Files:**
- Modify: `rill-lang/src/lexer.rs`
- Test: `rill-lang/tests/haskell2010_syntax_gaps.rs` (start the file)

**Interfaces:**
- Consumes: Task 1.
- Produces: `Tok::Backtick`, `Tok::PlusPlus`, block-comment skipping.

- [ ] **Step 1: Write the failing tests**

`haskell2010_syntax_gaps.rs`:
```rust
#[test]
fn backtick_infix_application() { /* add a b = a + b; 1.0 `add` 2.0 == 3.0 */ }
#[test]
fn list_append_operator() { /* [1,2] ++ [3] yields [1,2,3] via length/fold */ }
#[test]
fn nested_block_comment_is_skipped() { /* {- a {- b -} c -} main = ... runs */ }
#[test]
fn mode_pragma_still_works() { /* {-# MODE complete #-} parses as a pragma, not a comment */ }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --test haskell2010_syntax_gaps`
Expected: parse/lex errors for backtick, `++`, `{-`.

- [ ] **Step 3: Implement the lexer changes**

- `\`` → `Tok::Backtick`; the parser turns `` a `f` b `` into `f a b`.
- `++` → `Tok::PlusPlus` (check it is not already lexed as two `Plus` tokens and that `+` still works).
- Comment skipping: on `{-` (after pragma detection), scan to the matching `-}` with **nesting depth**; `{-#` is matched as a pragma **before** the comment path.

- [ ] **Step 4: Parser support for backtick and `++`**

- Backtick: parse `` e1 `name` e2 `` as application `name e1 e2`.
- `++`: as an infix operator desugaring to `append_list` (precedence per Haskell 2010: `++` binds tighter than comparison, looser than `:`).

- [ ] **Step 5: Verify and commit**

Run the tests; then the full gate.
```bash
git add rill-lang/src/lexer.rs rill-lang/src/parser.rs rill-lang/tests/haskell2010_syntax_gaps.rs
git commit -m 'feat(lang): lex backticks, list append and nested block comments'
```

---

### Task 2.5: Close the pre-existing conformance gaps (before Task 3)

> **Why this task exists:** a gap audit after Task 2 measured 13 remaining
> deviations. Three of them are **silent wrong answers** (a failed/absent
> operand coerces to `0`/empty instead of faulting), which the repo rule in
> `AGENTS.md` ("a divergence is a defect; no silent partial support") forbids
> outright. Nothing may proceed to Task 3 while these are open.

**Files:**
- Modify: `rill-lang/src/types/infer.rs`, `rill-lang/src/lower.rs`, `rill-lang/src/backend/interp.rs`, `rill-lang/src/parser.rs`, `rill-lang/src/types/ty.rs`
- Test: `rill-lang/tests/haskell2010_syntax_gaps.rs` (operator gaps), `rill-lang/tests/conformance_gaps.rs` (new)

**Interfaces:**
- Consumes: Task 2's fixity table and first-class `(++)`.
- Produces: operators and typeclass methods as first-class values; no silent
  wrong answers in list operations.

#### Measured gaps (audit 2026-10-24, all reproduced)

| # | Expression | Current | Required |
|---|---|---|---|
| G1 | `(+) 1.0 2.0` | `unknown '+' in value lowering` | `3.0` |
| G2 | `fold (+) 0 (listRange …)` | `fold expects a binary function` | the sum |
| G3 | `(+ 1.0)` section | parse error | `\x -> x + 1.0` |
| G4 | `(1.0 +)` section | parse error | `\x -> 1.0 + x` |
| G5 | `(++ [1.0])` section | parse error | `\x -> x ++ [1.0]` |
| G6 | `fold mappend …` | `method 'mappend' of 'Monoid' requires an argument` | method as a first-class value |
| G7 | ``1.0 `mappend` 2.0`` | **silently `0`** | the monoid result, or a loud type error |
| G8 | `fold (++) list [1.0,2.0]` | **silently `0`** | `[1.0,2.0]` |
| G9 | `length (cons 1.0 (cons 2.0 list))` | **silently `0`** | `2` |

G10–G13 are the `where`-capture, `let`-lambda, function-guard and list-comprehension
rows already scheduled as S1–S4/S11 in Tasks 3–4; they are **not** duplicated here.

- [ ] **Step 1: Write the failing tests**

`rill-lang/tests/conformance_gaps.rs`: one behavioural test per G1–G9, each asserting
a computed value that changes when the behaviour is wrong. For G7–G9 write the test
twice: once asserting the correct value, and once asserting that an *unsupported*
input **faults** rather than returning `0` (see Step 3).

- [ ] **Step 2: Operators as first-class values (G1–G5)**

The parser already yields `Expr::Ref("+")`; inference already types the arithmetic
operators (`types/infer.rs` ~3955, `ArrowTy::uniform(2,1,·)`). Lowering does not
produce a callable for an operator ref. Add operator values in lowering and make
`fold (+)` accept them (relax `expect_closure` for a builtin operator's known
arity). Sections `(+ e)` / `(e +)` / `(++ e)` desugar to unary lambdas in the parser
per Haskell 2010 §3.5.

- [ ] **Step 3: No silent wrong answers (G7–G9)**

Three defects share one root: a failed or absent operand yields `None`, and
downstream code coerces `None` to `0`/empty.

- Every bulk list op (`length`, `fold`, `map`, `filter`, `concat_map`,
  `append_list`) must **latch a clean value-track fault** when an argument is
  absent/bottom, instead of returning a zero result. The existing `latch_*` /
  `value-track fault` channel is the mechanism.
- `length` on `Value::Cons` must return the real length (this is the Task 5 rule,
  pulled forward only to the extent needed to stop the silent `0`; the full lazy
  consumer work stays in Task 5).
- Typeclass methods used as values (G6/G7): a bare method reference resolves by its
  class dictionary — implement it or reject it with a clear diagnostic; never `0`.

- [ ] **Step 4: Verify and commit**

Run the new test file, then:
```
cargo test -p rill-lang --features dsp
cargo clippy -p rill-lang --features dsp
cargo fmt --check
```
```bash
git add rill-lang/src rill-lang/tests/conformance_gaps.rs
git commit -m 'fix(lang): first-class operators and no silent list-operation answers'
```

---

### Task 2.6: `min` / `max` — scalar, first-class, and Ord-polymorphic

**Files:**
- Modify: `rill-lang/src/lower.rs` (scalar fallthrough at ~2181–2190; first-class operator table at ~7938), `rill-lang/src/types/infer.rs` (~4076), `rill-lang/src/backend/interp.rs` (`value_cmp_ref` already provides the order)
- Test: `rill-lang/tests/conformance_gaps.rs`

**Interfaces:**
- Consumes: Task 2.5's first-class operator machinery and the central
  absent-operand fault.
- Produces: `min`/`max` usable as scalar functions, as first-class values
  (`fold max`), and over any `Ord` type.

**Measured state (audit 2026-10-25):** every scalar form is an error —
`min 1.0 2.0`, `1.0 \`min\` 2.0`, `(min 1.0) 2.0`, `fold max …`, `min 1 2`,
`min "a" "b"` all fail with `unknown 'min' in value lowering` (or an arity
error for the partial application). The buffer form (`min _ 0.5`) works.

- [ ] **Step 1: Write the failing tests**

One behavioural test each, asserting the Haskell 2010 result:
```rust
#[test] fn min_max_scalar_float()      { /* min 1.0 2.0 == 1.0; max 1.0 2.0 == 2.0 */ }
#[test] fn min_max_backtick_and_partial() { /* 1.0 `min` 2.0; (min 1.0) 2.0 */ }
#[test] fn min_max_first_class()       { /* fold max 0.0 [1.0,3.0,2.0] == 3.0 */ }
#[test] fn min_max_ord_int()           { /* min 1 2 == 1 */ }
#[test] fn min_max_ord_string()        { /* min "a" "b" == "a" */ }
#[test] fn min_max_ord_record()        { /* min of two Pair values compares the first field */ }
#[test] fn min_max_arity_errors()      { /* min 1.0 == arity error */ }
```

- [ ] **Step 2: Run to verify they fail** — `unknown 'min' in value lowering`.

- [ ] **Step 3: Wire scalar `min`/`max`**

At `lower.rs:2181–2190` the code returns an error only for `atan2`; `min`/`max`
fall through and die later. Give `min`/`max` a scalar path (element-wise
`BinArith::Min`/`Max` already exists in the IR and interpreter for buffers; the
scalar case is the same comparison on two scalars). Keep `atan2`'s existing
error only if it stays genuinely unimplemented — and if it does, record it in
`DOCS-GAPS.md` (the rule forbids leaving it undocumented).

- [ ] **Step 4: Make `min`/`max` first-class values**

Extend the operator-value table (`lower.rs` ~7938, the one that already handles
`+ - * / % min max atan2` as `BinArith`) so a bare `min`/`max` resolves to a
callable. This is what `fold max` needs.

- [ ] **Step 5: Ord polymorphism**

`interp.rs::value_cmp_ref` already orders Int, Float, Bool, String, newtype,
sum, record, list, set, array, and map. Reuse it: `min`/`max` must compare by
`value_cmp`, not by f64 coercion, so `min "a" "b"` and record comparison work.
Ints stay on the Int track (do not widen to Float and back).

- [ ] **Step 6: Verify and commit**

```bash
cargo test -p rill-lang --features dsp
cargo clippy -p rill-lang --features dsp
cargo fmt --check
git add rill-lang/src rill-lang/tests/conformance_gaps.rs docs/superpowers/DOCS-GAPS.md
git commit -m 'feat(lang): scalar, first-class and Ord-polymorphic min/max'
```

---

### Task 3: Parser — where capture, guards, sections, arg patterns, standalone signatures

**Files:**
- Modify: `rill-lang/src/parser.rs`, `rill-lang/src/types/infer.rs` (guard totality), `rill-lang/src/lower.rs` (where scope)
- Test: `rill-lang/tests/haskell2010_syntax_gaps.rs`

**Interfaces:**
- Consumes: Task 2.
- Produces: S1, S3, S5, S8, S9 working.

- [ ] **Step 1: Write the failing tests** (one per construct, each asserting a computed result)

```rust
#[test] fn where_sees_function_parameters() { /* f x = g where { g = x + 1.0; }; f 1.0 == 2.0 */ }
#[test] fn function_guards_with_otherwise() { /* f x | x > 0.0 = 1.0 | otherwise = 2.0 */ }
#[test] fn guarded_function_without_fallback_is_rejected() { /* compile error */ }
#[test] fn operator_section_right() { /* (+ 1.0) applied to 2.0 == 3.0 */ }
#[test] fn operator_section_left() { /* (1.0 +) applied to 2.0 == 3.0 */ }
#[test] fn patterns_in_function_arguments() { /* f 0.0 = 1.0; f x = x */ }
#[test] fn standalone_type_signature() { /* f :: Float -> Float; f x = x */ }
```

- [ ] **Step 2: Run to verify they fail** — parse errors as listed in the design §1.2.

- [ ] **Step 3: Implement `where` capture (S1/S2)**

Make `where` defs resolve in an environment that includes the enclosing definition's λ-parameters. This is a lowering/inference scoping change: the where-def bodies must see the enclosing params, and the where-defined names must be visible in the enclosing body.

- [ ] **Step 4: Implement function guards (S3)**

Desugar `f p1 p2 | g1 = e1 | g2 = e2` to a definition over a `case`-like guarded alternative list. Extend the totality rule: a final `| otherwise` (or literal `True`) guard counts as an exhaustive fallback; without it, the guarded definition is non-exhaustive and rejected at compile time.

- [ ] **Step 5: Implement sections (S5) and operator values (S12)**

`(op e)` → `\x -> x op e`; `(e op)` → `\x -> e op x`; `(+)` → the binary function value. Inference must type `Expr::Ref(op)` as the operator's function type; lowering must produce a callable. `fold (+) …` must accept it.

- [ ] **Step 6: Implement arg patterns (S8) and standalone signatures (S9)**

`f 0.0 = …` translates per Haskell 2010's equation-matching into a `case` over the argument tuple (reuse the pattern matcher). `f :: T;` on its own line attaches to the following definition of the same name.

- [ ] **Step 7: Verify and commit**

```bash
git add rill-lang/src/parser.rs rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/tests/haskell2010_syntax_gaps.rs
git commit -m 'feat(lang): Haskell 2010 where, guards, sections, arg patterns and signatures'
```

---

### Task 3.5: Capture-avoiding instantiation of typeclass method bodies (OD-9)

> **Why this is a blocker:** `instantiate_method_call` (`lower.rs:5073`) wraps an
> instance method body in lambdas over the method's OWN parameter names without
> renaming them. A user definition whose name collides with a method parameter is
> captured: `f x = x + 1.0;` + IO `main` fails, `m x = …` fails, `return x = …`
> fails. The upcoming operator-as-class-method work routes **every** operator
> through this same path (`(+) a b`, `compare a b`, `fmap g xs`, `mappend a b`),
> which would multiply the capture surface from 2 names to dozens. Fix the
> substitution before the typeclass layer lands.

**Files:**
- Modify: `rill-lang/src/lower.rs` (`instantiate_method_call` ~5073; the operator-value helper at ~5150 already demonstrates the safe `$`-prefixed renaming)
- Test: `rill-lang/tests/typeclass_layer.rs` (new) or `haskell2010_syntax_gaps.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: method instantiation that is safe for any caller-side identifier.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn user_def_named_f_is_usable()      { /* f x = x + 1.0; IO main prints 2.0 */ }
#[test] fn user_def_named_m_is_usable()      { /* m x = x + 1.0 */ }
#[test] fn user_def_named_return_is_usable() { /* return x = x + 1.0 */ }
#[test] fn user_params_named_like_method_params() { /* g a b = a + b; method params are a/b */ }
#[test] fn method_called_with_colliding_local()   { /* local named `f`/`m` passed to a class method */ }
```

- [ ] **Step 2: Run to verify they fail** — OD-9's three shapes fail today.

- [ ] **Step 3: Rename method parameters to fresh reserved names**

When instantiating a method body, rename each of the method's own parameters to a
fresh `$`-prefixed name (the prefix cannot occur in source identifiers, so it can
never collide), and substitute consistently through the body. Then wrap in
lambdas over the fresh names. This mirrors `lower_operator_value`, which already
uses `$op_a`/`$op_b` for exactly this reason.

- [ ] **Step 4: Verify no existing behaviour changed**

The rename must be semantically transparent: every currently passing test stays
green, and the three OD-9 shapes now produce the expected values.

- [ ] **Step 5: Close OD-9 in the register and commit**

```bash
git add rill-lang/src/lower.rs rill-lang/tests/ docs/superpowers/DOCS-GAPS.md
git commit -m 'fix(lang): capture-avoiding method instantiation'
```

CAN START IF: the four constructs §3.11 desugaring needs are already verified
working — this was measured on 2026-10-25 (Strict mode, so `length` over the
result is meaningful):

| Needed construct | Verified |
|---|---|
| local function in `let`, single equation | ✅ `let ok x = cons x (list)` → `3` |
| local function in `let`, TWO equations (§3.11's `ok p = …; ok _ = …`) | ✅ → `3` |
| two equations at top level | ✅ → `3` |
| `concat_map` with a lambda | ✅ → `3` |
| guard inside a `let`-bound local function | ❌ **OD-11** — silently yields nothing |

So Task 4 is **not mechanically blocked** by the operator/typeclass unification —
it depends only on `let`-bound local functions and `concat_map`, which work. The
one real dependency inside Task 4 is OD-11 (guards in local functions), and
§3.11 does not need guards for the generator/guard/`let`-qualifier rules — the
guard qualifier desugars to `Expr::If`, not to a function guard.

**Ordering ruling (revised 2026-10-25, supersedes the earlier ruling):** the
typeclass layer's **P0–P3 runs BEFORE Task 4**. The earlier ruling ("Task 4
first") was based only on surface disjointness and missed the quality dependency,
measured afterwards:

- while `+`/`==` are built-in opcodes, `1 + 1.0` silently succeeds (Haskell 2010
  rejects it) and `1 == 1.0` returns `False` instead of being a type error;
- `f x y = x + y` cannot be `Num a => a -> a -> a` — it is pinned to `Float`, and
  calling it with `Int` is a hard error;
- therefore a comprehension such as `[x + 1 | x <- [1,2,3]]` fails on the
  Int/Float mismatch, and `[x * 2 | x <- xs]` only works for `Float`.

With `Num`/`Ord` as classes, operators resolve through the type system: the
ambiguity disappears and comprehensions work for both numeric types. So Task 4's
*quality* depends on the typeclass layer even though its *mechanism* does not.

Revised order: **typeclass P0–P3 → conformance Task 4 → Task 5 → typeclass P4–P5
→ conformance Task 6 → typeclass Task 6 (audit)**.

---

### Task 4: `let` bindings of function/record values, list comprehensions

**Files:**
- Modify: `rill-lang/src/lower.rs`, `rill-lang/src/types/infer.rs`, `rill-lang/src/parser.rs`
- Test: `rill-lang/tests/haskell2010_syntax_gaps.rs`

**Interfaces:**
- Consumes: Task 3's sections/guards.
- Produces: S4 (all `let` positions) and S11.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn let_binds_a_lambda_in_let_in() { /* let g = \x -> x + 1.0 in g 1.0 */ }
#[test] fn let_binds_a_record_in_let_in() { /* let p = Pair { first: 1.0, second: 2.0 } in p.first */ }
#[test] fn do_let_binds_a_lambda() { /* do { let g = \x -> x + 1.0; write … (g 1.0) } */ }
#[test] fn where_nested_in_let() { /* let f = g where { g = 1.0; } in f */ }
#[test] fn list_comprehension_generator() { /* [x | x <- [1,2,3]] */ }
#[test] fn list_comprehension_guard() { /* [x | x <- [1,2,3], x > 1] */ }
#[test] fn list_comprehension_let_qualifier() { /* [y | x <- [1,2], let y = x + 1.0] */ }
```

- [ ] **Step 2: Run to verify they fail.**

- [ ] **Step 3: Fix `let` lowering**

Bindings whose RHS is a lambda, record, or partial application must be lowered as real bindings in every position — `do`-`let`, `let … in`, lambda body, `where`. (One fix; the current restriction is not `do`-specific.)

- [ ] **Step 4: Implement list comprehensions (S11)**

Desugar `[e | q1, …, qn]` per Haskell 2010 §3.11: generator `p <- xs` → `concat_map (\p -> …) xs`; guard `b` → `if b then … else []`; `let p = e` → a `let` in the desugared body.

- [ ] **Step 5: Verify and commit**

```bash
git add rill-lang/src/lower.rs rill-lang/src/types/infer.rs rill-lang/src/parser.rs rill-lang/tests/haskell2010_syntax_gaps.rs
git commit -m 'feat(lang): let-bound values and list comprehensions'
```

---

### Task 5: Lazy list operations and the Prelude `from`/`take`

**Files:**
- Modify: `rill-lang/src/backend/interp.rs` (`pooled_elems_of` ~1984 and the builtin op arms ~2330–2600), `rill-lang/src/types/ty.rs` (`STD_PRELUDE`)
- Test: `rill-lang/tests/lazy_lists.rs` (new)

**Interfaces:**
- Consumes: Task 1's recursion scope.
- Produces: `from`/`take` in `STD_PRELUDE`; `Cons`-aware `length`/`map`/`fold`/`filter`/`concat_map`/`append_list`.

- [ ] **Step 1: Write the failing tests**

`rill-lang/tests/lazy_lists.rs`:
```rust
use rill_lang::{compile, ffi::ForeignRegistry};

#[test] fn from_and_head_work() { /* head (from 1.0) == 1.0 */ }
#[test] fn take_terminates_on_infinite_from() { /* length (take 5 (from 1.0)) == 5 */ }
#[test] fn fold_over_take_of_from() { /* fold (+) 0 (take 10 (from 1)) == 55 */ }
#[test] fn length_on_explicit_cons_chain() { /* length (1.0 : 2.0 : 3.0 : []) == 3 */ }
#[test] fn map_over_cons_chain() { /* fold over map (+1) of a cons chain */ }
#[test] fn take_of_map_of_from_terminates() { /* take 5 (map f (from 1)) — must terminate */ }
#[test] fn benchmark_list_size_works() { /* take 64 (from 1.0) and fold over it */ }
#[test]
fn user_take_shadows_the_prelude_take() {
    // The user's own `take` must win over the Prelude `take` in BOTH inference
    // and lowering. Before the Task-1 shadowing fix this mistyped silently
    // (Prelude scheme + user body). Repeated here so the rule is pinned where
    // it matters; `deep_recursion_parity` remains the end-to-end regression.
    let src = r#"{-# MODE complete #-}
take n xs = if n == 0.0 then list else cons (case (head xs) of { Nothing -> 0.0; Just x -> x }) (take (n - 1.0) (tail xs));
from n = cons n (from (n + 1.0));
main = do { write (Just (show_float (fold (\a -> \x -> a + x) 0.0 (take 3.0 (from 10.0))))) stdout; return () };
"#;
    let mut r = compile::<f32, 256>(src, ForeignRegistry::new()).unwrap();
    r.run().unwrap();
    assert_eq!(r.stdout(), ["33.0"], "user take: 10 + 11 + 12");
}
```

- [ ] **Step 2: Run to verify they fail** — `unbound operand`, `length == 0`, or a hang for the lazy-producer test; `user_take_shadows_the_prelude_take` fails only if Task 1's shadowing fix regressed.

- [ ] **Step 3: Make consumers representation-agnostic**

`length` and `fold` must accept `Value::Cons` by walking the chain (forcing tails as needed) as well as `Value::List`. Implement one helper used by both, so no op silently returns 0 on `Cons`.

- [ ] **Step 4: Make producers lazy**

`map`, `filter`, `concat_map`, and `append_list` over a `Cons` input must return a `Cons` chain (lazily, forcing per element as demanded), so `take k (map f (from 1))` terminates. Over a `Value::List` input they keep the current strict path and `Value::List` result.

- [ ] **Step 5: Add `from`/`take` to `STD_PRELUDE`**

```haskell
from n = cons n (from (n + 1.0));
take k xs = if k <= 0 then [] else case head xs of { Nothing -> []; Just x -> x : take (k - 1) (tail xs) };
```
Pin the exact numeric spelling (`Int` vs `Float`) by compiling the Prelude; adjust only the literal types, never the semantics. Add `otherwise = True;` if Section 3's guard desugaring requires it.

- [ ] **Step 6: Verify `n = 64` and commit**

Run: `cargo test -p rill-lang --features dsp --test lazy_lists`
Expected: green, including `benchmark_list_size_works`.
Run the full gate.
```bash
git add rill-lang/src/backend/interp.rs rill-lang/src/types/ty.rs rill-lang/tests/lazy_lists.rs
git commit -m 'feat(lang): lazy list operations and prelude from/take'
```

---

### Task 6: Final verification and deviation bookkeeping

**Files:**
- Test: whole crate
- Modify: `bench/ghc-parity/deviations.md` (rows 3 and 6 are now closed) — only if that file exists; otherwise note it in the task report.

- [ ] **Step 1: Full crate gates**

```bash
cargo test -p rill-lang --features dsp
cargo clippy -p rill-lang --features dsp
cargo fmt --check
```
Expected: all green, zero warnings.

- [ ] **Step 2: Verify every S-row has a passing test**

Run: `cargo test -p rill-lang --features dsp --test haskell2010_syntax_gaps -- --list`
Expected: tests named for S1–S12 all present; run them: green.

- [ ] **Step 3: Verify the benchmark precondition**

Compile and run the exact list sub-workload the GHC-parity benchmark will use (take/fold/map over `from` at `n = 64`), and confirm it terminates with the expected value. Record the value in the report.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m 'test(lang): verify Haskell 2010 conformance and lazy list surface'
```
