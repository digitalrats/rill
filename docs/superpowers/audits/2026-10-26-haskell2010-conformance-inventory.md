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

### A4b. Precision, buffer element types, and the numeric tower (measured 2026-10-26)

> **Status update 2026-10-26 (task 5.5pre-0, commit `0e78439`): the first three
> rows below are FIXED — as a rename, not a precision change.** Measurement showed
> the type was numerically *correct* GHC `Double` all along; only its name was
> wrong. `Float` is now `Double`, `Float` is an unknown-type error, and
> `show_float` became `show_double`. The remaining gap is that rill has **no
> single-precision `Float`**; that lands with the numeric tower (P3 in
> `2026-10-25-rill-lang-typeclass-layer.md`), not as an alias.

| Construct | rill (before 5.5pre-0) | GHC | Verdict then | Now |
|---|---|---|---|---|
| `Value::Float` payload | `f64` | `Float` is single precision (§6.4.1) | **WRONG — `Float` silently means `Double`** | **FIXED** — renamed `Double` |
| `Float` arithmetic precision | `1/3` → `0.3333333333333333`; `0.1+0.2` → `0.30000000000000004` | `:: Float`: `0.33333334`; `0.3` — but defaulted: **identical to rill's** | **WRONG name, right value** | **FIXED** by the rename |
| `Double` | did not exist | real type | MISSING | **FIXED** |
| single-precision `Float` | did not exist | `Float` | MISSING | **MISSING** — lands with P3 |
| `f :: FixedBuffer Zork -> FixedBuffer Zork` | compiles and runs | unknown type | **WRONG — silent** (element type is discarded) |
| `g :: FixedBuffer Int -> FixedBuffer Bool; g x = x` | compiles and runs | type error | **WRONG — silent** |
| `instance Num (Tape f32)` | accepted as *nominal* `f32` | accepted, but `f32` is a **type variable** (Report §2.4: `tycon → conid → large…`) — an instance for all `a`; verified with `Tape Int`/`Tape Bool` | WRONG (different meaning) |
| `FixedBuffer a` in an FFI signature | `FfiParam::Signal` — element type discarded | — | **WRONG** — `FixedBuffer f32`, `Float`, `Int`, `Zork` classify identically |
| `f32` as a type name in any position | accepted as `Data("f32", [])` | lexically impossible: a type constructor must start uppercase | **WRONG** — silently a type variable |
| elementwise `a / b` on a buffer | works | needs `Fractional a` | missing constraint |
| elementwise `sin`, `sqrt`, `exp`, `ln`, `tanh` on a buffer | works | needs `Floating a` | missing constraint |
| elementwise `a % b` on a `Float` buffer | works | `No instance for 'Integral Float'` | **EXTRA — silent** |
| `Floating` class | does not exist | `class Fractional a => Floating a` | MISSING |
| `Integral` class | does not exist | `class (Real a, Enum a) => Integral a` | MISSING |
| `Buffer` class | `typeclass (Num b) => Buffer b` — a `Num` subclass, so it licenses arithmetic it does not own | no such class; `Num`/`Fractional`/`Floating` are the tower | WRONG (wrong tower placement) |

Root cause of the `FixedBuffer` rows: **the element-type argument is discarded**.
`types/unify.rs:157` matches `ValueTy::Buffer` against `App("FixedBuffer", args)`
only when `args.is_empty()`; `types/ffi.rs:258` checks only `args.len() == 1`.
No element type is ever resolved — which is precisely why `f32` survived as a
rill type name, and why a name registry alone cannot catch it (`f32` is lowercase,
i.e. a type variable by Haskell's lexical rules).

Fix order: §Task 5.5pre-0 (precision) → 5.5pre-1 (element type) → 5.5a
(registry) → 5.5pre-2 (`Floating`/`Integral`, `Buffer` out of the tower).

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
| `Float` is really `Double` | f64 precision | single precision |
| `FixedBuffer Zork` | accepted | unknown type |
| `FixedBuffer Int` unified with `FixedBuffer Bool` | accepted | type error |
| `%` on `Float` elements | accepted | `No instance for 'Integral Float'` |

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
