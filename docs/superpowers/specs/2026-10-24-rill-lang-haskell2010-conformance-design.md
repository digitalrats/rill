# rill-lang: Haskell 2010 Conformance — residual syntax gaps and lazy lists

> **Status:** Design approved-in-conversation (2026-10-24). Implements the
> remaining gaps of the north star declared in
> `2026-10-12-rill-lang-haskell2010-unary-core-design.md`; does **not** re-open
> that document's settled decisions (D1–D11).
> **Date:** 2026-10-24
> **Scheduled before:** the GHC-parity benchmark
> (`2026-10-24-rill-lang-ghc-parity-benchmark-design.md`) and the memory-layers
> implementation, because the benchmark's list workload needs `from`/`take`.

---

## 0. Executive summary

The unary-core migration removed the signal language, but it left a set of
**Haskell 2010 syntax constructs unimplemented**. The user's north star is full
**Haskell 2010 + GHC Prelude** conformance, so each of these is a defect, not a
design choice. In addition, lazy lists are half-implemented: `Value::Cons` works
for `head`/`tail`/`cons`, but every bulk list operation (`length`, `map`, `fold`,
`filter`, …) only understands the array-backed `Value::List`, so `from` is usable
only until its result meets a bulk op, and `take` cannot be written.

This design closes both gaps with one rule: **where Haskell 2010 has a construct,
rill parses and evaluates it with the same meaning.**

---

## 1. Verified current state (2026-10-24)

All rows verified by compiling/running a minimal program on HEAD.

### 1.1 Already conformant

`if/then/else` (including inside `case` arms and `let`), `let … in`,
`do` with offside layout, `case … of` (including multi-line and wildcard
patterns), **guards inside `case` arms**, `where` on `main` in braces form,
`--` line comments, string/number/list literals, inline `::` signatures,
`typeclass`/`instance`, lambdas, currying, `main : IO ()`.

### 1.2 Residual syntax deviations

| # | Haskell 2010 construct | rill behaviour | Evidence |
|---|---|---|---|
| S1 | `f x = g where { g = x + 1.0; }` — `where` sees the function's parameters | type error `unknown identifier x` | where-defs do not capture λ params |
| S2 | `where` nested inside `let`/`do`; parameter capture in any `where` | type error / unknown identifier | top-level offside `where` **does** parse (corrected 2026-10-24) |
| S3 | Guards on a **function definition**: `f x \| c = e \| otherwise = e2` | parse error `expected Eq, found Pipe` | guards exist only in `case` arms |
| S4 | `let g = \x -> …` / `let p = Pair {…}` — **any** `let`, not only `do`-`let` | `unknown g` / `unknown p` in value lowering | `let` bindings of function/record values are not lowered |
| S5 | Operator section `(+ 1.0)` | parse error | — |
| S6 | Backtick infix `` 1.0 `add` 2.0 `` | lex error (no backtick token) | — |
| S7 | `++` (list concatenation) | parse error | `append_list` exists as a function only |
| S8 | Pattern matching in function arguments: `f 0.0 = …` | parse error | — |
| S9 | Separate-line type signature `f :: T;` before the definition | parse error | only the inline `f x :: T = …` form parses |
| S10 | Block comments `{- … -}` (nested per Haskell 2010) | parse error | only `--` exists |
| S11 | List comprehension `[x \| x <- xs]` | parse error | — |
| S12 | Operators as first-class values: `(+)`, `fold (+)` | `unknown '+' in value lowering` / `fold expects a binary function` | parser already yields `Expr::Ref("+")`; inference/lowering gap |

### 1.3 Lazy list gap

- `Value::Cons(h, t)` is understood only by `force`, `head`, `tail`, `LazyCons`,
  `cons`, and `trace`.
- `length`, `map`, `fold`, `filter`, `concat_map`, `append_list` (and anything
  else routing through `pooled_elems_of`) understand only `Value::List { elems }`
  and `Set`. On a `Cons` they return `None`, producing
  `primitive effect with an unbound operand`; `length` silently returns `0`.
- Consequence: `from n = cons n (from (n + 1.0))` works for `head`/`tail`, but
  `take` (necessarily recursive over a `Cons`) cannot reduce its result, and
  strict producers make `take k (map f (from 1))` impossible.
- `from` and `take` are **not** in `STD_PRELUDE`. (`listRange` is the only
  built-in list generator.)

### 1.4 Blocking preconditions found by the design review (2026-10-24)

These are defects that must be fixed **as part of** this work, not worked around:

| # | Defect | Where |
|---|---|---|
| B1 | Prelude defs are prepended into `program.defs` **before** `check_recursion` runs, so a recursive `from`/`take` in `STD_PRELUDE` fails Strict mode ("recursive function call") and breaks the whole suite. | `types/infer.rs` (`infer_program_with`, `check_recursion`) |
| B2 | A user definition that shadows a Prelude name is **excluded from inference** (`prelude_names` filter) but still **wins in lowering** (last-wins map). The program is typed as the Prelude's scheme and executed as the user's body — silently wrong, no diagnostic. `tests/deep_recursion_parity.rs` defines its own `take`, so this path is live. | `types/infer.rs` (`prelude_names`), `lower.rs` |
| B3 | Static arena-capacity accounting rejects recursive thunk chains: `take 20 (from 1)` fails with "value-track resource exhausted"; `length (take 50 (from 1))` with "value arena capacity". The benchmark's list size (`n` up to 64) is above the wall. | `lower.rs` capacity accounting |
| B4 | Deep recursion over an explicit cons chain overflows a default 2 MiB test thread at ~11 elements (not a value bug, a stack-size limit). | runtime / test harness |

---

## 2. Goals, non-goals, anti-goals

### Goal

**G1 — Syntax conformance.** Every construct in §1.2 S1–S11 parses and evaluates
with the meaning Haskell 2010 gives it, and each is covered by a test that runs a
program using the construct.

**G2 — Lazy-list conformance.** The Prelude bulk operations work on both
representations (`Value::List` and lazy `Value::Cons`), so a list produced by
`from` can be consumed by `fold`, `map`, `take`, `length`, etc.

**G3 — `from`/`take` in the Prelude** with GHC Prelude signatures:
`from :: Int -> [a]`, `take :: Int -> [a] -> [a]` (spelled with rill's current
numeric/structural types), so the benchmark's list workload is idiomatic.

### Non-goals

- Type-class *semantics* beyond what already works (Haskell 2010's full class
  system is out of scope; only surface syntax + the list gap are in scope).
- Changing the ST/IO/value execution model (settled by the unary-core and
  world-state designs).
- Performance work; the memory-layers plan owns that.
- Re-opening D1–D11 of the unary-core design.
- Implementing `where` capture for parameters *and* overloading it with new
  semantics — only the Haskell 2010 meaning is implemented.

### Anti-goals

- Implementing a construct differently "because rill's model prefers it".
- Special-casing the benchmark: `from`/`take` must be general Prelude functions.
- Silent partial support (work for `head` but not `fold`) — the list ops must be
  representation-agnostic.
- Breaking existing tests: the current test suite is the compatibility gate.

---

## 3. Design

### 3.1 Lexer additions (S6, S7, S10)

| Construct | Token | Note |
|---|---|---|
| `` ` `` | `Tok::Backtick` | Backtick-quoted identifiers become infix operators. |
| `++` | reuse/add `Tok::PlusPlus` | Distinct from `+`; Haskell 2010 list append. |
| `{- … -}` | comment skipping in the lexer | **Nested** per Haskell 2010 §2.2; must not confuse `{-#` pragmas (pragma detection runs first). |

`{-` inside an existing pragma `{-# … #-}` stays a pragma — the pragma form is
matched before the comment form.

### 3.2 Parser additions (S1–S5, S8, S9, S11)

- **S1/S2 — `where` capture and layout.** A `where` block attaches to the
  enclosing definition; its definitions resolve in an environment that includes
  the enclosing definition's λ-parameters. Both the braces form and the offside
  layout form parse. This is a scoping fix (lowering/inference), not a new AST
  node.
- **S3 — function guards.** `f x | g1 = e1 | g2 = e2` desugars to a `case`-like
  guarded alternative list over the argument tuple, exactly as Haskell 2010
  specifies; `otherwise` is a Prelude constant `True`. **`| otherwise` (and a
  literal `True` guard) counts as an exhaustive fallback** for the totality
  check: a guarded definition whose final guard is `otherwise`/`True` is total,
  and one without such a fallback is a compile error (matching the existing
  `case` totality rule, which must be extended to recognise the
  `otherwise`/`True` guard rather than skipping every guarded arm).
- **S4 — `let` bindings (any position).** A `let` binding whose right-hand side
  is a lambda, record, or partial application must be lowered as a real binding,
  in every position (`do`-`let`, `let … in`, inside a lambda body, inside
  `where`). The restriction is **not** specific to `do`: `let g = \x -> … in g 1`
  currently fails in Strict mode too, and S11's `let` qualifiers hit the same
  path. This is one fix in lowering/inference, not four.
- **S5 — operator sections.** `(op e)` and `(e op)` produce the corresponding
  unary lambda (`\x -> x op e` / `\x -> e op x`); `(- e)` is unary negation (the
  Haskell 2010 special case).
- **S8 — patterns in function arguments.** `f 0.0 = …` compiles to a
  guarded/`case`-based definition per Haskell 2010's equation-matching
  translation; the existing pattern matcher is reused.
- **S9 — standalone signatures.** `f :: T;` before `f x = …` is accepted and
  attached to the definition (Haskell 2010 allows the signature on its own
  line).
- **S11 — list comprehensions.** `[e | q1, q2, …]` desugars to nested
  `concat_map`/`map`/`filter` per Haskell 2010 §3.11; generators (`p <- e`),
  guards (`b`), and `let` qualifiers are supported.

### 3.3 Lazy list operations (G2)

Every bulk list operation must accept **both** representations:

- `Value::List { elems }` — array-backed, as today.
- `Value::Cons(h, t)` — lazy pair of head/tail refs, forced on demand.

Rules:

1. A single helper converts a list value into an **element stream** consumed by
   the operation: for `List` it iterates the Vec; for `Cons` it walks the chain,
   forcing each tail as it goes.
2. **Producers must be lazy where Haskell 2010 is lazy.** `map`, `filter`,
   `concat_map`, and `append_list` over a `Cons` chain must return a `Cons`
   chain, not a strict `Value::List` materialisation — otherwise
   `take k (map f (from 1))` cannot terminate. Ops over a `Value::List` keep
   their fast strict path and their `Value::List` result.
3. **Consumers** (`length`, `fold`) may walk the whole chain; an infinite chain
   is only valid when the chain was demand-limited upstream (`take`).
4. `take` is the **demand limiter**: it forces exactly `k` head cells and never
   the `(k+1)`-th tail ("Demand-limited `take`" is a pinned test: must
   terminate on `from`).
5. `head`/`tail` keep their current `Cons` behaviour; the array-backed path is
   unchanged.
6. No operation may silently treat `Cons` as empty (`length` returning 0 is the
   current bug this fixes).

### 3.4 Prelude additions (G3)

Add to `STD_PRELUDE`:

```haskell
from n = cons n (from (n + 1.0));
take k xs = if k <= 0 then [] else case head xs of { Nothing -> []; Just x -> x : take (k - 1) (tail xs) };
```

Notes:

- The concrete integer/Float spelling follows the language's current numeric
  types (`Int` for lengths in the Prelude's array code; `Float` for the value
  track). The plan pins the exact spelling against what the type checker accepts,
  verified by compiling the Prelude.
- `take` must be demand-limiting through the `Cons` path (see §3.3 rule 4);
  otherwise it hangs on `from`.

**Prelude recursion vs. Strict mode (B1).** `check_recursion` must run over
**user definitions only**, not the Prelude. The Prelude is trusted library code;
its recursion is the same category as the existing prelude `map`/`fold` bodies.
Concretely: call `check_recursion` before prepending prelude defs (or filter
prelude names out of the check). This changes no user-visible rule: a *user*
recursive function in Strict mode still fails.

**Name resolution and shadowing (B2).** One rule, applied identically in
inference and lowering: **a user definition always shadows the Prelude
definition of the same name, and the user body is what both types and runs.**
The `prelude_names` filter that currently excludes user defs from inference is
removed; instead, when a user def has a Prelude name, the Prelude def is dropped
from the merged definition list before any phase runs. `tests/deep_recursion_parity.rs`
(which defines its own `take`) therefore keeps its exact behaviour, and becomes
the pinned regression test for this rule.

**Capacity and stack (B3/B4).** Static arena accounting must not reject a
program merely because a *recursive* Prelude function exists in it; the budget is
charged for allocation *sites*, and a recursive chain's live set is bounded by
its demand (`take k` ⇒ O(k) live cells). If the static model cannot express
that, the correct fallback is a **documented, tested capacity error** at a
pinned `n`, not a silent wrong answer — but the benchmark's stated `n` must be
inside the working range, so this design requires `n = 64` to work on the
benchmark's list workload. Deep-recursion stack overflow is a **documented
limit** (`run_on_stack`, already in `program_runner.rs`); tests that need depth
beyond the default use it.

### 3.4a Operators as values (S12)

`(+)`, `(-)`, `(==)`, and the other arithmetic/comparison operators are
first-class values of function type, as in Haskell 2010. The parser already
produces `Expr::Ref("+")`; inference and lowering must give `Ref("+")` the
operator's binary function type and a callable runtime value. This is required
by acceptance criterion 2 (`fold (+) …`) and by `map (+)`-style code.

### 3.5 Tests

New test files (names pinned in the plan):

- `rill-lang/tests/haskell2010_syntax_gaps.rs` — one test per S1–S12, each a
  compiled+run program asserting its Haskell 2010 meaning.
- `rill-lang/tests/lazy_lists.rs` — `from`/`take` with `fold`, `map`, `length`,
  `filter`, `append_list`; **`take 5 (map f (from 1))` must terminate**
  (lazy producer); `length` on an explicit `Cons` chain returns its real length;
  `fold (+) 0 (take n (from 1))` (S12) yields the expected sum; the benchmark's
  list size (`n = 64`) works.
- **`tests/deep_recursion_parity.rs`** is the pinned regression test for the
  Prelude-shadowing rule (B2): its user-defined `take` must keep producing
  `["10.011.012.0"]`.
- The existing `haskell2010_syntax.rs`, `match_patterns.rs`, `graph_parse.rs`
  must keep passing unchanged (S2 is corrected: top-level offside `where`
  already parses — the new tests cover parameter capture and nesting instead).

---

## 4. Risks and mitigations

| Risk | Mitigation |
|---|---|
| `where` capture changes scoping and breaks existing programs | Capture only the enclosing λ-params (Haskell 2010 semantics); run the full suite. |
| Guarded functions desugar differently from `case`, breaking totality checks | Reuse the existing guarded-`case` machinery; extend totality to treat `otherwise`/`True` as exhaustive. |
| `take` on an infinite `from` hangs | Demand-limiting `take` is a pinned test (must terminate), not an afterthought. |
| Lazy producers change list performance | `Value::List` keeps its fast strict path and result; only `Cons` inputs produce `Cons`. |
| Prelude recursion breaks Strict mode (B1) | `check_recursion` runs over user defs only; regression test: user recursion still rejected. |
| Prelude shadowing silently mistypes programs (B2) | One shadowing rule applied in both inference and lowering; `deep_recursion_parity` pins it. |
| Static arena accounting rejects recursive chains (B3) | Budget charged per allocation site; `n = 64` must work; fallback is a documented capacity error at a pinned `n`, never a silent wrong answer. |
| Deep recursion overflows the test stack (B4) | Documented limit; use `run_on_stack` where depth is needed; tests stay within the default or use the helper. |
| `{-` collides with `{-#` pragmas | Pragma matching runs first; a test pins `{-# MODE complete #-}` still works. |
| Scope creep into full typeclasses | Non-goal; only syntax + list ops + operator values. |
| Prelude spelling mismatch (Int vs Float) | Pinned by compiling the Prelude in the first task. |

---

## 5. Acceptance criteria

1. Every §1.2 construct (S1–S12) has a passing test that runs a program.
2. `from` and `take` are in `STD_PRELUDE`; `take 5 (from 1)` terminates and
   yields `[1,2,3,4,5]`; `fold (+) 0 (take n (from 1))` yields the expected sum.
3. `length`/`map`/`fold`/`filter`/`concat_map`/`append_list` return correct
   results on an explicit `Cons` chain (no `unbound operand`, no silent 0), and
   `take k (map f (from 1))` terminates.
4. `cargo test -p rill-lang --features dsp` green **including**
   `deep_recursion_parity` (user `take` wins over the Prelude) and every existing
   test unchanged otherwise.
5. `cargo clippy -p rill-lang --features dsp` clean; `cargo fmt --check` clean.
6. The GHC-parity benchmark's list workload can be written with `from`/`take` at
   its stated list size (`n = 64`), and the corresponding deviation rows are
   deleted.
7. Strict mode still rejects **user** recursion (`check_recursion` scoped to user
   defs, not the Prelude).

---

## 6. Relationship to other work

- **Before** the GHC-parity benchmark: its list workload needs `from`/`take`
  (`bench/ghc-parity/deviations.md` row 3 disappears).
- **Before** the memory-layers plan: the benchmark's baseline must be taken on
  conformance-complete code so the measurement is not confounded.
- **Independent of** docs-reconciliation: the docs fix describes today's code;
  after this work lands, the `from`/`take` gap row in `DOCS-GAPS.md` is closed.
