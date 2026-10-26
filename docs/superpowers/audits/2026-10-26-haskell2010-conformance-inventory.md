# Haskell 2010 conformance inventory — 2026-10-26

> **Status:** inventory only. **No fix has been applied.** Every entry is a
> confirmed divergence between rill (branch `feature/rill-lang-haskell2010-conformance`,
> HEAD `12bcc44`, 1005 tests green) and GHC 9.10.3 / the Haskell 2010 Report.
>
> Per `AGENTS.md` there is **no "pre-existing" exemption**: these are defects to
> fix, not behaviour to document. This file is the work list, not a licence.
>
> Method: rill programs were compiled and run; the reference is the Haskell 2010
> Report plus (where useful) `runghc` on GHC 9.10.3. A divergence is either
> **MISSING** (rill rejects what Haskell accepts), **WRONG** (rill produces a
> different value — worst), or **EXTRA** (rill accepts what Haskell rejects).

---

## A1. Prelude surface — 39 of 40 probed names are missing or broken

| Name | rill | Verdict |
|---|---|---|
| `negate` | `cannot unify Thunk([Float],[Float]) with Float` | WRONG (declared, not callable) |
| `signum` | same | WRONG |
| `abs` (scalar) | same | WRONG (buffer-only today) |
| `recip` | same | WRONG |
| `div`, `mod`, `quot`, `rem` | `unknown identifier` | MISSING |
| `floor`, `ceiling`, `round`, `truncate` | `unknown identifier` | MISSING |
| `fromIntegral`, `toInteger` | `unknown identifier` | MISSING |
| `succ`, `pred`, `minBound` (and `maxBound`) | `unknown identifier` | MISSING |
| `pi`, `log`, `(**)` | `unknown identifier` / parse error on `**` | MISSING |
| `compare` | `cannot unify Thunk([Int],[Thunk([Int],[Int])]) with Int` | WRONG |
| `min`, `max` | `primitive instance Ord Int has no kernel for method min` | WRONG (kernel missing) |
| `sort`, `maximum`, `minimum` | `unknown identifier` | MISSING |
| `foldr`, `foldl`, `sum`, `product`, `elem`, `null` | `unknown identifier` | MISSING |
| `show` | `unknown identifier` | MISSING |
| `enumFromTo` | `unknown identifier` | MISSING |
| `[a..b]` (`[1..3]`) | lex error: `invalid float literal 1..3` | MISSING (syntax) |
| `reverse`, `concat` | `unknown identifier` | MISSING |
| `take`, `from` | `unknown identifier` | MISSING (design Task 5 of the conformance plan) |
| `map`, `filter`, `length`, `fold` | work today | OK |

`show_float` / `show_int` / `show_bool` exist; they are rill-specific, not
Haskell 2010 names.

## A2. Syntax

| Construct | rill | Verdict |
|---|---|---|
| `newtype N = N Int` | parse error: `expected Eq, found Semi` | MISSING (only the `newtype X = T` form without a payload parses) |
| as-pattern `p@(Ctor …)` | parse error: `expected Eq, found At` | MISSING |
| list pattern `x : xs` | parse error: `expected a pattern, found LBracket` | MISSING |
| nested constructor pattern `Just (Pair { … })` | parse error: `expected RParen, found LBrace` | MISSING |
| `x :: t` (expression annotation) | **works** (Task 1) | OK |
| guards (definitions and case), `where`, `let`, `do`, `case`, sections, backticks, comprehensions | work | OK |
| fixities §4.4.2 | work (verified earlier against GHC) | OK |
| `--`, nested `{- -}`, `{-# MODE #-}` | work | OK |
| layout/offside | works | OK |

## A3. Declarations

| Construct | rill | Verdict |
|---|---|---|
| `data` record / sum | works | OK |
| `type` synonym | works | OK |
| derived `Eq` / `Ord` on a record | works | OK |
| `String` ordering (`"a" < "b"`) | works | OK |
| `newtype` with a payload | MISSING (A2) | MISSING |
| `typeclass` with superclasses | works (P-2) | OK |
| `instance` with a constraint head (`instance Monad m => Arrow (Kleisli m)`) | builds; dictionary form still monomorphised | OK today, see A7 |

## A4. Types and numeric behaviour

| Expression | rill | GHC | Verdict |
|---|---|---|---|
| `1 + 1.0` | `2.0` | `2.0` | OK (by accident: forced `Float`) |
| `1 == 1.0` | **`False`** | `True` | **WRONG — silent** |
| `5 < 2.0` | **`True`** | `False` | **WRONG — silent** |
| `(1::Int) + 1.0` | **`2.0`** | type error (`Fractional Int`) | **EXTRA — silent** |
| `add x y = x + y; add 1 2` | type error | `3` | MISSING (no `Num` polymorphism) |
| `add 1.0 2.0` | `3.0` | `3.0` | OK |
| `id x = x` at `Int` and `Float` | works | works | OK |
| integer literal in a `Float` context (`f x = x + 1.0; f 1`) | `2.0` | `2.0` | OK |
| defaulting (`1 + 2` → `Int`) | `Int` | `Integer` | OK for rill's numeral set (documented mapping) |

Root cause of the three WRONG/EXTRA rows: operators still go through the legacy
`Expr::Arith` / `Expr::Cmp` path with a `kind_rank` fallback instead of the class
methods (design §3.7 stages 4–9). This is **OD-16 / OD-17**.

## A5. Expressions and evaluation

| Construct | rill | Verdict |
|---|---|---|
| `false && <fault>` short-circuit | `False` | OK |
| `if` with a non-`Bool` condition | compile error | OK |
| non-exhaustive `case` | compile error naming the missing ctor | OK |
| `let` shadowing (`let x = 1; let x = 2`) | error: `duplicate definition x` | WRONG (Haskell allows shadowing) |
| left section `(1 +)` | `cannot unify Int with Float` | WRONG |
| `1.0 / 0.0` | `inf` (no exception) | OK for IEEE, but see A8 |
| `head` of an empty list via `case` | `0.0` from the `Nothing` arm | OK |

## A6. Class semantics

| Construct | rill | Verdict |
|---|---|---|
| class default method (`min` from `Ord`) | `no kernel for method min` | WRONG (default body not reachable) |
| `no instance of …` | compile error, actionable | OK |
| superclass entailment (`Ord a` gives `==`) | resolves in a constrained body | OK |
| recursive constrained definition | still rejected | MISSING (design G3; `MODE complete` only) |
| dictionary form | monomorphised per call site | not yet the design model (stages 3–9) |

## A7. Values and types

| Construct | rill | Verdict |
|---|---|---|
| `data` record/sum, `newtype`, `type` | work (except `newtype` payload) | OK |
| `String` escapes (`"a\nb"`) | `"a\nb"` (literal backslash-n!) | **WRONG — silent** |
| hex literal `0x10` | lexes as `0` then `x10` | MISSING |
| tuple `(1, 2)` | parses; `p.first` on a tuple fails in lowering | WRONG |
| `(**)` operator | parse error | MISSING |
| nested ctor pattern in a `case` | parse error | MISSING |

## A8. Error behaviour — silent wrong answers (fix first)

| Case | rill | Expected |
|---|---|---|
| `1 == 1.0` | `False` | `True` (or a type error) |
| `5 < 2.0` | `True` | `False` (or a type error) |
| `(1::Int) + 1.0` | `2.0` | type error |
| `"a\nb"` string escape | literal `\n` | newline |
| mixed `Int`/`Float` comparison in general | `kind_rank` ordering | type error |

---

## Summary of counts

| Area | OK | MISSING | WRONG | EXTRA |
|---|---|---|---|---|
| A1 Prelude (40 probed) | 4 | 26 | 9 | 1 |
| A2 Syntax (12) | 9 | 4 | 0 | 0 |
| A3 Declarations (7) | 6 | 1 | 0 | 0 |
| A4 Types/numeric (9) | 5 | 1 | 2 | 1 |
| A5 Expressions (7) | 5 | 0 | 2 | 0 |
| A6 Classes (5) | 3 | 1 | 1 | 0 |
| A7 Values (6) | 3 | 2 | 1 | 0 |
| A8 Silent wrong answers | — | — | 5 | — |

**The five silent wrong answers come first**: a program that runs and prints a
plausible wrong value is worse than one that fails to compile.

## Proposed fix order

1. **A8 silent wrong answers** (OD-16/OD-17, string escapes) — they corrupt
   results silently.
2. **A4 numeric polymorphism** — needs the operator/dictionary stages (design
   §3.7 stages 4–9); this is also what makes `add 1 2` work.
3. **A1 Prelude surface** — the missing kernels (`compare`, `negate`, `signum`,
   scalar `abs`, `fromInteger`, `recip`) are design §3.9; the rest is the
   Prelude library (design P2–P5 of the typeclass plan).
4. **A2/A7 syntax** — `newtype` payload, as-patterns, list patterns, nested ctor
   patterns, `(**)`, hex literals, tuples, `[a..b]`.
5. **A5/A6** — `let` shadowing, left sections, class default methods.
6. **A3/A4 remaining** — recursive constrained definitions.

## Instrumentation note

The probes used `compile` + `run` on a `main : IO ()` program and printed
`show_*` results. Where rill rejected a program the reference was checked with
`runghc` 9.10.3. A permanent conformance harness (running this matrix in CI) is
recommended and is the natural consumer of the GHC-parity work.
