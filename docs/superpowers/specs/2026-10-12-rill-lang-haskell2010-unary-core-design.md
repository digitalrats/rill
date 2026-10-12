# rill-lang — Haskell 2010 Syntax + Unary Core Design

**Date:** 2026-10-12
**Status:** Design (draft for review)
**Scope:** rill-lang syntax and execution model unification

## 1. Goal

Riduce rill-lang to a **unary lambda calculus**: every function has arity 1
(currying), every expression is a single value, and the syntax follows
Haskell 2010 (the 2010 revision that formally supports the
higher-kinded typeclasses we already use — `Functor f`, `Kleisli m`). This
eliminates the signal path (wires, combinators, channel
arity) entirely and unifies inference/lowering around one model.

The purpose is **simplification and unification** of the syntax and the
execution system, not feature growth.

## 2. Design decisions (confirmed)

| #   | Decision                                                                                                                                                                                                                        |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | Lambdas: `\x -> body` (replaces `fn x -> body`). A lambda is strictly unary; multi-parameter currying lives only in definitions (`foo x y = body` ≡ `foo = \x -> \y -> body`) and applications (`f a b` ≡ `((f a) b)`). |
| D2  | Comments: `--` (replaces `//`).                                                                                                                                                                                                 |
| D3  | Case: `case e of { p -> e; ... }` (replaces `match e of { p => e }`).                                                                                                                                                           |
| D4  | Offside rule: layout (indentation) delimits blocks; `{}`/`;`/`where {}` removed.                                                                                                                                                |
| D5  | Type signatures `::` as annotations/constraints; type is still inferred (Hindley–Milner).                                                                                                                                       |
| D6  | Remove: `fn`, `match`, wire `_`, combinators (Seq/Par/Split/Merge/Loop/Delay), `!` (Cut), imaginary literals `3i`.                                                                                                              |
| D7  | Keep: `do { ... }` (monadic bind), `?param` (actor params), `type` (synonym) and `newtype` (distinct type — semantically different, both kept), `data`/`typeclass`/`instance`, `:` (cons only), `,` (tuples → Pair), `foreign`. |
| D8  | **Currying of everything, including FFI**: `foo x y = x + y` ≡ `foo = \x -> \y -> x + y`; `sine 440.0 1.0 0.0` ≡ `(((sine 440.0) 1.0) 0.0)`.                                                                                    |
| D9  | **Remove channel arity** (ArrowTy ins/outs): every expression is a single value (0→1) or `Func([a], [b])` (arity 1). Multichannel signal processing disappears with wires.                                                      |
| D10 | Merge `Anchor`/`Local` into one def form `name args? = value`.                                                                                                                                                                  |
| D11 | `instance Monad (ST s)` works (partial head; already implemented).                                                                                                                                                              |

## 3. Current state (baseline)

- Lexer: 768 lines (tokens incl. `fn`, `match`, `:`, `~`, `@`, `,`, `<:`, `:>`, `!`, `?`).
- Parser: 2103 lines (Pratt; `parse_do_block`, `parse_where_block`, layout not used).
- AST: 9 `Def` forms, 30+ `Expr` variants incl. 6 combinators + `Wire` + `Cut`.
- Inference: `ArrowTy { ins: Vec<ValueTy>, outs: Vec<ValueTy> }` with arity; 41+ uses of `arity_in()`.
- Reduce: β-reduction (inlines 0-param locals; applies λ-params).
- Lower: dual track — signal (`fn lower`, combinators, `ValueBuffer*` block ops) and value (`lower_value`). 1252-line signal path.
- FFI: buffer-LAST catalog, 3–9 arg signatures, partial application for Buffer arrows.
- Prelude: `SIGNAL_PRELUDE` has `seq`/`par`/`split`/`merge`; `CATEGORY_PRELUDE` has typeclasses/instances incl. `Monad (ST s)`.

## 4. Target model

### 4.1 Unary core

- Every definition: `name = expr` where `expr` may be a lambda `\x -> body`.
  Multi-parameter definitions are LEFT-ASSOCIATIVE currying sugar, at the
  definition site only: `foo x y = body` ≡ `foo = \x -> \y -> body`.
- Every application is also left-associative currying sugar:
  `foo 1.0 2.0` ≡ `((foo 1.0) 2.0)`. There is no n-ary application type —
  `foo : Thunk([Float], Thunk([Float], [Float]))`.
- Every expression infers to a single `ValueTy`. `ArrowTy` is reduced to
  `Option<ValueTy>` / a single value channel.
- Partial application is natural: `add5 = add 5.0` where `add = \x -> \y -> x + y`.

### 4.2 Thunk as the one value type

Everything is a `Thunk` — a callable value with captures. The single type
constructor is `Thunk(captures, result)` where `captures` is the FULL list of
captured bindings (environment fields + parameters) and `result` is the type of
the forced value:

| Value | Type | Runtime |
|---|---|---|
| `1.0` | `Thunk([], [Float])` | callable, empty env, body yields `1.0` |
| `\x -> x + 1` | `Thunk([Float], [Float])` | callable capturing `x` |
| closure `\y -> x + y` (`x` from env) | `Thunk([Float; env], [Float])` | callable with an env field |
| HOF `\f -> \x -> f x` | `Thunk([Thunk..], [Thunk..])` | callable whose result is a callable |
| lazy `a = a + 1` | `Thunk([], [Float])` + memo cell | lazy thunk, call-by-need |

Consequences:

- `ValueTy::Int/Float/String/Bool/Buffer/Void` collapse into
  `Thunk([], [T])` — the type of a constant is a zero-capture thunk yielding
  that value. The one constructor replaces all scalar types + `Func`.
- `ValueTy::Func(args, rets)` becomes `Thunk(args, rets)` (curried: exactly one
  argument type, or zero for a thunk).
- Runtime already has the carrier: `Value::Callable { fragment, env, cell }`
  unifies eager closure and lazy thunk (memo/black-hole via `ThunkCell`).
  A forced constant is a flat `Value::Float`; the type `Thunk([], [Float])`
  describes the COMPUTATION that yields it — static description, distinct
  from the runtime representation.
- `force` (already implemented) drives everything: lazy thunk memoizes,
  eager callable is WHNF, a flat value is already WHNF.

### 4.3 FFI currying

- `foreign fn sine : Float -> Float -> Float -> FixedBuffer f32` becomes a
  curried chain of unary builtins: `sine : Float -> (Float -> (Float -> Buffer))`.
- `sine 440.0 1.0 0.0` is left-associative currying sugar for
  `(((sine 440.0) 1.0) 0.0)`. Each scalar application may fold
  eagerly (constant); the final Buffer application runs the DSP.
- Buffer-LAST stays as the convention: the final arg is the Buffer input; a
  partial application `lowpass 1000.0 0.7` is a `Thunk([Buffer], [Buffer])`
  arrow.

### 4.3 IO / do-notation

- `do { xs <- readInput; ys <- process xs; writeOutput ys; return () }`:
  `readInput : () -> List Buffer`, `process : List Buffer -> ST s (List Buffer)`
  (or a plain value), `writeOutput : List Buffer -> IO ()`. Monadic bind via
  `instance Monad (ST s)` / `instance Monad IO`.
- `?param` reads a named control parameter (scalar value).

## 5. Component changes

### 5.1 Lexer (`lexer.rs`)
- Add `\` (Backslash) token; add `--` comment.
- Remove `fn`, `match` keywords; add `case`, `::`.
- Remove combinator tokens `~`, `@`, `<:`, `:>`, `!`. Keep `:`, `,`, `?`, `->`.
- Offside: track indentation (column) per line for layout.

### 5.2 Parser (`parser.rs`)
- Layout-aware parsing (offside rule) replacing `{}`/`;`/`where {}` blocks.
  `let ... in` and local defs use layout, as in Haskell.
- Parse `\x -> body` lambdas (strictly unary).
- Parse `case e of { p -> e; ... }`.
- Desugar multi-arg application `f x y z` → `((f x) y) z`.
- Desugar `name p1 p2 = body` → `name = \p1 -> \p2 -> body`.
- Merge Anchor/Local AST nodes into one `Def::Fn` (params optional).
- Drop combinator/wire/cut expressions from the grammar.

### 5.3 AST (`ast.rs`)
- Remove `Expr::Wire`, `Expr::Cut`, `Expr::Seq/Par/Split/Merge/Loop/Delay`,
  `Expr::Imag`.
- Remove `Def::Anchor` (fold into `Def::Fn`).
- Reduce `ArrowTy` to a single value type (or remove it, threading `ValueTy`
  directly).

### 5.4 Inference (`infer.rs`)
- Remove arity (`arity_in`/`arity_out`, `ArrowTy.ins/outs`).
- Every expression yields one `ValueTy`. Lambdas yield `Func([a],[b])`.
- Unification is purely value-level; `Subst` maps type vars to `ValueTy`.
- FFI application follows curried signatures.

### 5.5 Reduce (`reduce.rs`)
- β-reduce unary applications: `((\x -> body) v)` → `body[x := v]`.
- Keep thunks (complete mode) for recursion/laziness.

### 5.6 Lower (`lower.rs`)
- Delete the signal path (`fn lower`, `lower_seq/par/split/merge/feedback/
  delay/arith`, `bind_signal_args`, `has_internal_wire_feed`, block-track
  `ValueBuffer*` ops for combinators).
- Keep value track: `lower_value` only; FFI currying; `ValueCallFunc` for
  closures; `ValueReadInputs`/`ValueWriteOutputList` for IO.
- Buffers remain ordinary values (`Value::Buffer`).

### 5.7 Prelude
- Keep `seq`/`par`/`split`/`merge` as pure functions (they are already
  lambdas over Buffer arrows).
- Keep `instance Monad (ST s)`, `instance Monad IO`.

## 6. What this buys

- One execution model: unary functions, thunks, closures. No dual tracks.
- Inference reduces to HM over single values (no channel arity bookkeeping).
- FFI is uniform: curried unary builtins, partial application for free.
- `do`/ST/IO compose through Monad instances.
- The signal path, combinators, and channel arity — the largest source of
  complexity — are deleted.

## 7. Out of scope (later, if needed)

- Faust combinators as syntax (`:`, `,`, `<:`, `:>`, `~`, `@`) — deferred;
  `seq`/`par`/`split`/`merge` exist as functions.
- Wires `_` and multichannel arity — deferred; inputs via `readInput`.

## 8. Migration

- 69 test files use wire/combinator programs; migrate to the value model
  (`readInput`/`writeOutput`/`map`/curried functions).
- Prelude `main = _;` placeholders become `main = 1.0` or are removed.
- Render module updated for the new AST.

## 9. Risks

- Offside rule: largest parser change; must handle tabs, nested layouts,
  and the existing `do`-block / `case` layout.
- FFI currying: 3–9 arg builtins; eager scalar folding must preserve the
  single-Buffer-input convention and `signal_ins`/`signal_outs` metadata.
- Test migration volume (69 files) — mechanical but large.