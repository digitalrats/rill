# rill-lang: First-Class Functions and Closures — Design

> **Status:** Approved — 2026-09-25.
> **Date:** 2026-09-25
> **Branch:** `feature/rill-first-class-functions`
> **Scope:** Make functions first-class values in rill-lang: lambda literals,
> named-function references with runtime dispatch, partial application
> (currying), and higher-order combinators (HOF). Closures capture their free
> variables by-value (snapshot) into an environment record. Signal wires are
> positional wire-captures at the call site. Recursion is forbidden, preserving
> the acyclic strict contract and a statically-bounded RT call depth.

## 1. Problem statement

The previous stage (data + arena memory model) added named-function references
as values: `Value::Func(u32)` (an index into `Ir::value_funcs`), created by
`ValueMakeFunc`, with `ValueCallFunc` a **no-op** (calls β-reduce at compile
time). This supports `f = double; main = f 21.0` only when the call target is
statically known. It does NOT support:

1. **Lambda literals** — anonymous functions `fn x -> expr` as expressions.
2. **Closures** — capturing free variables (including enclosing function
   parameters) at creation time.
3. **Runtime dispatch** — calling a function value whose target is not known
   until the value flows (passed to a HOF, returned from a function, stored in
   a record).
4. **Partial application / currying** — `add3 = add 3` producing a function
   value.
5. **Signal-wire arguments** — functions that take a signal block per tick.
6. **Higher-order combinators** — `map f xs`, `twice f x`, etc.

## 2. Semantic model

### 2.1 A function value is a closure

A first-class function is a **closure**: a pair of a captured environment
(`env_ref`, an arena `Record`) and a reference to a compiled body fragment
(`fragment_id`). The closure is an ordinary arena value:

```rust
// arena.rs
enum Value {
    // ... existing Int, Float, Record, Sum, Newtype, Void ...
    /// A first-class function: captured environment + body fragment.
    Closure(ArenaRef, u32),   // (env_ref, fragment_id)
}
```

- **env** — a by-value snapshot of the free variables visible at the closure's
  creation point, including enclosing function/λ-parameters. Each captured name
  is copied into a `Record` field slot at creation (an RC owner). This is
  ordinary arena+COW — no new memory machinery.
- **fragment_id** — an index into `Ir::fragments: Vec<FragmentIr>`.
- **`Value::Func(u32)` is replaced by `Closure`.** A named function with no
  free variables is a closure with an empty (`Void`) env. One mechanism for all
  function values.

### 2.2 Capture rules

| Captured entity | Mechanism |
|---|---|
| Free **value** variable (incl. enclosing fn/λ-params) | by-value snapshot into env Record at closure creation |
| Free **signal wire** | NOT captured at creation — bound per-call at the call site (see §2.4) |
| Top-level variable in the same scope | by-value snapshot (same rule) |

Scope is lexical: an inner lambda sees the outer function's parameters
(`adder n = fn x -> x + n` — `n` is captured when the inner `fn` is created).
At top level, a lambda captures same-level variables.

### 2.3 Signature typing

```rust
// types/ty.rs
enum ValueTy {
    // ... existing ...
    /// Function type: (value-arg types, value-result types).
    /// Signal-wire args are positional wire-captures, NOT in the type.
    Func(Vec<ValueTy>, Vec<ValueTy>),
}
```

- `ValueTy::Func(arg_tys, result_tys)` types only the **value** arguments and
  value results. Signal-wire arguments are positional wire-captures (not typed;
  arity is known to lowering).
- The compiler infers a lambda's signature from its body (HM), checks HOF
  argument positions against expected signatures, checks partial application
  arity/type (`add : Float → Float → Float`; `add 3` → `Float → Float`), and
  rejects arity/type mismatches at compile time.
- **Currying** is closure-based: `add3 = add 3` compiles to a closure that
  captures `3` and applies `add` to `(3, x)` on call.

### 2.4 Signal wires are wire-captures at the call site

A signal argument is **not** a typed value argument. At the **call site**, the
signal wire is bound as a reference to the caller's block register (per tick,
zero-copy — consistent with rill's zero-copy rule). The fragment body reads it
as a block register and runs block-track steps over it.

- A function with a signal wire can be passed to a HOF **as a value** — the
  wire remains unbound until an actual call supplies it. Boundary cases:
  - **Returning a value** — capturing local value variables has no effect on
    the returned value; capturing a signal wire works as usual (bound per call).
  - **Top-level assignment** — a top-level lambda captures same-level
    variables as usual.

### 2.5 Recursion is forbidden

A function that (transitively) calls itself — directly or through a closure —
is a compile-time error, like recursive data types. This preserves:
- the acyclic strict contract (no cycles → RC is sound);
- a **statically-bounded RT call depth** = the length of the longest call path
  in the (acyclic) call graph, pre-allocated (no heap growth on the RT path).

## 3. FragmentIr

```rust
/// A compiled function body: a fragment of the value/block track.
struct FragmentIr {
    /// Value-track instructions for the body.
    value_instrs: Vec<ValueInstr>,
    /// Block-track steps (for signal-wire args) — empty for pure-value bodies.
    steps: Vec<Step>,
    /// Number of value registers (args + temps).
    num_value_regs: usize,
    /// Number of block registers (signal args + temps).
    num_block_regs: usize,
    /// Value register(s) holding the result.
    output_value_regs: Vec<usize>,
    /// Block register(s) holding signal results.
    output_block_regs: Vec<usize>,
    /// Arity: number of value args and signal args.
    sig: FuncSig,
}

/// In `Ir`:
pub fragments: Vec<FragmentIr>,
```

- `FragmentIr` is **not** a standalone program — it is a fragment of the
  surrounding execution context: it shares the interpreter's arena, cells, and
  block-register store, and runs within the same tick.
- Compiled in `lower.rs`: each lambda body / named function body is lowered
  into a fragment by a dedicated pass.

## 4. Dispatch: `ValueCallFunc`

`ValueInstr::ValueCallFunc` becomes a real runtime dispatch:

```rust
run_closure(prog, closure_slot, args):
  1. read Closure(env_ref, fragment_id) from the arena
  2. push a temporary cell-stack frame:
       env fields + value args -> local cells
       signal args -> block-register references (wire captures)
  3. run FragmentIr[fragment_id]:
       exec its value_instrs and block steps
  4. copy result register(s) -> dst
  5. pop the frame (RC release of cell refs)
```

- The temporary cell-stack frame is the existing runtime stack of cells
  (Task 13 of the data stage) — a call pushes an activation frame, pops it on
  return. Depth is statically bounded (acyclicity), pre-allocated.
- `ValueMakeFunc` is replaced: closure creation is a new
  `ValueConstructClosure { dst, env_ref, fragment_id }` instruction (or
  `ValueMakeClosure`) that allocates the `Closure` slot.

## 5. Syntax

```faust
double = fn x -> x * 2.0;           // lambda literal
adder  = fn n -> fn x -> x + n;     // captures parent param n
add2   = adder 2;                   // currying via closure
twice  = fn f x -> f (f x);         // HOF
amp    = fn g x -> x * g;           // g: value, x: signal wire
main   = amp 2.0 _;                 // wire-capture at the call site
apply  = fn f x -> f x;
```

- `fn` is a new keyword. `fn p1 p2 ... -> body` — juxtaposed params (no parens,
  consistent with the language). Body is an expression.
- A lambda in expression position is a value expression producing a
  `ValueTy::Func`.

## 6. Pipeline changes

```
source → lexer (fn keyword) → parser (Lambda expr) → infer
  (infer lambda signature, capture set, Func sig) → reduce (keep lambdas)
  → lower (compile each body to FragmentIr; closure creation; ValueCallFunc
  dispatch) → RillProgram (fragments + pre-allocated call stack)
```

## 7. Out of scope

- Recursion (including mutual), Y-combinator.
- Capturing signal wires into the env at creation (wires bind per call).
- Typeclass method values (methods resolve at compile time; not reified).
- Closures over `main`'s signal inputs beyond wire-capture at call sites.
- Value arithmetic on closures (no comparison/equality of functions).

## 8. Implementation phases

| # | Phase | Scope |
|---|---|---|
| 1 | Signature typing | `ValueTy::Func(Vec, Vec)`, replace `Value::Func(u32)` → `Closure`, `ValueConstructClosure` |
| 2 | FragmentIr + dispatch | `Ir::fragments`, `ValueCallFunc` real dispatch, temp cell-frame, static depth |
| 3 | Lambda literals | lexer `fn`, parser `fn p -> body`, infer signature + capture set, lower to fragment |
| 4 | Env capture | by-value snapshot of free variables incl. parent params; closure creation |
| 5 | Currying + wires | partial application via closures; signal-wire args at call sites |
| 6 | HOF + tests + docs | `map`/`twice`/`apply` combinators; integration tests; rill-lang.md |

Each phase is a commit with tests; phases 1–2 are the foundation, 3–4 the
language surface, 5–6 the first-class completeness.

## 9. Testing

1. Lambda literal creates a `Value::Closure` with the right fragment.
2. Closure captures a parent parameter by-value (snapshot); mutation of the
   original does not affect the closure (COW).
3. `adder n = fn x -> x + n; add2 = adder 2; add2 3` → 5.
4. HOF: `twice f x = f (f x); double = fn x -> x * 2.0; main = twice double 3`
   → 12.
5. Signal-wire arg: `amp = fn g x -> x * g; main = amp 2.0 _` → block × 2.0.
6. Partial application type-checked: `add 3` yields `Float → Float`.
7. Recursion rejected: `f = fn x -> f x` → compile error.
8. Static depth: a deep non-recursive call chain compiles; the call stack is
   pre-allocated to the max path length.
9. Regression: all existing tests stay green; zero clippy warnings.

## 10. Files

| File | Change |
|---|---|
| `rill-lang/src/arena.rs` | `Value::Closure`, remove `Value::Func(u32)` |
| `rill-lang/src/types/ty.rs` | `ValueTy::Func(Vec, Vec)`, `FuncSig` |
| `rill-lang/src/ir.rs` | `ValueConstructClosure`, `ValueCallFunc` (real), `Ir::fragments`, `FragmentIr`, `FuncSig` |
| `rill-lang/src/lexer.rs` | `fn` keyword |
| `rill-lang/src/parser.rs` | `fn p -> body` lambda expression |
| `rill-lang/src/types/infer.rs` | lambda signature + capture set inference |
| `rill-lang/src/lower.rs` | fragment compilation, closure creation, call dispatch |
| `rill-lang/src/backend/interp.rs` | `run_closure`, `ValueCallFunc` dispatch, temp cell-frame |
| `rill-lang/src/program.rs` | pre-allocated call-stack depth |
| `rill-lang/src/reduce.rs` | keep lambdas (no β-reduce of closure bodies) |
| `rill-lang/tests/*` | new tests (§9) |
| `docs/src/guides/rill-lang.md` | language reference updates |

Verification: `cargo test -p rill-lang`, `cargo test --workspace`,
`cargo clippy --workspace`, `cargo fmt`. Zero warnings. No new external
dependencies.