# rill-lang: Branching and Pattern Matching — Design

> **Status:** Draft — awaiting review.
> **Date:** 2026-09-28
> **Branch:** `feature/rill-lang-logic`
> **Scope:** Add runtime control flow to the rill-lang value track — an `if`
> `then` `else` expression and a generalized `match` with constructor, literal,
> wildcard, variable, and nested patterns plus Haskell-style guards. `match`
> loses its v1 static-only restriction and dispatches on the constructor tag at
> runtime. Program execution is restructured from a flat `Vec<ValueInstr>` into a
> **control-flow graph of value blocks** walked by a trampoline interpreter with
> data-driven successors.

## 1. Problem statement

The value track (per-tick arena values) currently has **no control flow**:

1. **No `if`** — `Bool` exists (literals, `==`/`<`/etc., `&&`/`||`, prefix `not`),
   but there is no way to branch on it. A Boolean condition cannot select between
   two value expressions.
2. **`match` is static-only.** `Expr::Match` (constructor pattern + identifier
   bindings) lowers only the arm whose constructor is statically known; a
   scrutinee that is not statically resolvable is a compile error
   (`"match scrutinee is not statically resolvable in v1"`, `lower.rs:396`).
   Guards, literal patterns, wildcard catch-all arms, and nested destructuring
   do not exist.
3. **The program is a flat list.** `Ir::value_instrs: Vec<ValueInstr>` executes
   in a linear `for` loop (`interp.rs:144`). There is no notion of a branch or a
   join, so per-tick runtime selection (e.g. a `Bool` param written by
   `SetParameter` that changes behavior from block to block) is impossible.

This feature is the first step toward general control flow in the value track,
so the execution structure must survive the eventual move toward
Turing-completeness (loops, general recursion) without a rewrite.

## 2. Semantic model

### 2.1 Expressions, not statements

rill-lang has **no statements** — everything is an expression. `if` and `match`
are pure expressions that produce one value per tick, bindable by `name = ...`,
passable to functions, embeddable in arithmetic/records, or used as `main`:

```faust
x = if flag then 1.0 else 2.0;
y = match m of { Just v => v; Nothing => 0.0; };
main = 0.5 * (if flag then 2.0 else 3.0);
```

Branches are expressions too; there is no imperative form. The language is pure
and recursion-free, so a branch has no side effects other than the discrete
value-state slots (`~`/`@`).

### 2.2 Per-tick evaluation and `SetParameter`

The value track re-evaluates the whole value graph **once per backend block**.
`SetParameter` commands are drained from the actor mailbox at the start of the
tick, written into persistent main cells (`write_main_cell`, `program.rs:461`),
and the value track reads them per tick via `ValueReadMainCell`. Therefore a
runtime `Bool` derived from a parameter switches `if`/`match` behavior from tick
to tick:

```faust
?gate = 1.0;
flag = gate > 0.5;              // runtime Bool, re-evaluated every tick
main = if flag then 1.0 else 0.0;
```

Runtime `Bool` sources in v1: (a) a `Float`/`Int` main λ-parameter compared to a
threshold (primary case), (b) value-state feedback `~`/`@` (a self-referencing
`c = ~ (c + 1.0)` accumulates across ticks). `Bool` parameters themselves are
**out of scope** — `ParamValue` is `Float | Int` and `write_main_cell` always
materializes a `Float` slot; deriving a `Bool` from a param via comparison is
sufficient.

### 2.3 Control flow is data, not the call stack

The value track is **acyclic by construction** today (recursion is forbidden,
`check_recursion`; the DSL has no loops). A per-tick execution is one path
through a DAG. Two execution models were considered:

| Model | Mechanism | Verdict |
|---|---|---|
| **Loopless fn-pointer chain** (old graph engine: `Port::propagate` recursing through `downstream_input_ptrs`) | each step calls the next as a Rust `call`; control lives in the Rust stack | Rejected. Safe Rust has no guaranteed TCO and the crate `#![deny(unsafe_code)]` rules out computed-goto/asm. Fine only in a DAG; **breaks at Turing-completeness** (loops/recursion would overflow the stack). |
| **Block CFG + trampoline** | control lives in the IR as successor **block ids** (data); a trivial `while cur != HALT` loop dereferences | **Chosen.** The "graph of next actions" is preserved; the executor is a data-driven trampoline. Loops later are backward jumps; recursion pushes frames onto the existing pre-allocated call-register store (`max_call_regs`). Survives Turing-completeness without rewriting the executor. |

The existing interpreter already models function calls this way — `run_fragment`
keeps frames in a pre-allocated register store with a watermark, not on the Rust
stack. Block CFG extends the same principle to control flow. A future `jit` maps
block ids → code pointers and the trampoline disappears into generated machine
code.

## 3. Syntax

### 3.1 `if`

```haskell
if cond then a else b        // Haskell-style; `else` mandatory (expression)
```

New lexer keywords `KwIf`, `KwThen`, `KwElse`. `Expr::If { cond, then, els,
span }`. `if` is a prefix expression (parsed like `match`); `else` is not an
atom/continuation, so `parse_expr` stops before it (verify `is_atom_start`).

### 3.2 `match`

```haskell
match x of {
    Circle r => r;                       // constructor pattern + bindings
    0 => "zero"; _ => "other";           // literal + wildcard
    Just (Left x) => x; _ => 0.0;        // nested pattern
    n | n > 0 => 1.0; _ => 0.0;          // guard on a bound variable
}
```

AST change — `Expr::Match { scrutinee, arms: Vec<MatchArm>, span }`:

```rust
pub struct MatchArm {
    pattern: Pattern,
    guards:  Vec<(Expr, Expr)>,   // (guard_cond, body); trailing bare `=>` = otherwise
}
pub enum Pattern {
    Var(String),                  // binds the whole value
    Wild,                         // `_`
    LitInt(i64), LitFloat(f64), LitBool(bool), LitStr(String),
    Ctor(String, Vec<Pattern>),   // recursive (nested)
}
```

**Case convention (new).** The current parser distinguishes constructors from
bindings *positionally* (first ident = ctor, rest = params). Nested patterns
make this ambiguous (`Just (Left x)` — is `Left` a ctor or a binding?), so the
Haskell convention is introduced:

- **Uppercase initial** → constructor (`Circle`, `Just`, `Left`)
- **Lowercase initial** → variable binding (`r`, `x`)
- `_` → wildcard

Existing code and tests are compliant (`Circle r`, `Rect w h`, `Some x`, `Just
g`, `Left x`); confirm during implementation.

Pattern grammar: `_` | literal | `(` subpattern `)` | Ident → `Ctor` (juxtaposed
subpatterns) if uppercase / `Var` if lowercase. `|` reuses the existing
`Tok::Pipe`. In pattern position `_` maps to `Wild`; in expression position it
remains `Wire`.

## 4. Typing

### 4.1 `if`

- `cond` must be `Bool` (value track).
- `then`/`else` unify to a single `ValueTy`; the result is that type.
- Result rate is `Value`. A signal-rate branch is a type error: "if branches
  must be value-track expressions; signal selection is not supported in v1".

### 4.2 `match`

- **Scrutinee**: any value type — sums (user `data` and builtin `Maybe`/
  `Either`) and scalars `Int`/`Float`/`Bool`/`String`. Record patterns are out
  of scope (records use field projection).
- **Pattern vs scrutinee type**:
  - `Var`/`Wild`: any.
  - `Ctor(name, args)`: scrutinee must be a sum; `name` a constructor of that
    sum; `args.len()` must equal the payload arity; each argument pattern is
    type-checked recursively against the payload type (builtin-sum placeholder
    `Var(k)` type params resolve via the scrutinee's type args — existing
    `infer.rs` machinery).
  - `Lit*`: the literal's type must equal the scrutinee type. Runtime equality
    is exact (`f64 ==`); a `NaN` literal pattern never matches (documented).
- **Guards**: each guard must be `Bool` (value track).
- **Arm bodies**: all unify to a single result `ValueTy`; result rate `Value`.
- **Determinism**: ctor indices from declaration order; arms in source order;
  first match wins (`first-match-wins`, Haskell-style). No `HashMap`-order
  hazards.

### 4.3 Exhaustiveness (compile-time totality + runtime fallback)

- **Sum scrutinee**: every constructor must be covered by an *unguarded* arm, or
  an unguarded `Wild`/`Var` arm must exist. Otherwise compile error
  "non-exhaustive match: missing constructor(s) …".
- **Scalar scrutinee**: a `Wild`/`Var` arm is required. Exception: `Bool` is
  total if both `true` and `false` literals are covered (or a wildcard exists).
  No interval analysis for `Int`/`Float`/`String`.
- **Guarded arms do not count toward coverage** (a guard may fail at runtime).
  Under the strict totality rule a constructor covered only by a *guarded* arm
  is **non-exhaustive**: `match Just 5.0 of { Just x | x > 10.0 => 1.0;
  Nothing => 2.0; }` is a **compile error** — the `Just` constructor has no
  unguarded arm (and no wildcard covers it).
- **Runtime fallback (defense-in-depth):** the lowered match always ends in a
  fail block that latches `ProcessError` on a tick where no arm matches. Because
  compile-time totality guarantees a well-formed match has an unguarded
  fallback, this backstop is reachable in practice only when the scrutinee slot
  is uninitialized — an unbound `_` wire scrutinee (`match _ of { … }`) has no
  constructor tag to dispatch on — which yields `ProcessError`. (Covered by
  `match_over_unbound_wire_scrutinee_errors_at_runtime` in
  `tests/match_patterns.rs`.)

## 5. IR

```rust
// ir.rs
pub struct ValueBlock {
    pub instrs: Vec<ValueInstr>,   // straight-line; identical semantics to today
    pub term:   ValueTerm,
}
pub enum ValueTerm {
    Fallthrough(usize),                              // run block id
    Branch     { cond: usize, then: usize, els: usize },   // cond: Bool value reg
    BranchCtor { slot: usize, ctor: u32, then: usize, els: usize }, // scrutinee tag == ctor
    Halt,
}

// Ir:        value_instrs: Vec<ValueInstr>  →  value_blocks: Vec<ValueBlock>, value_entry: usize
// FragmentIr: same shape (blocks local to the fragment)
```

- New `ValueInstr::ValueMove { dst, src }` — ownership transfer (`dst = src;
  src = None`), used at join points so both branches share one result register
  without an extra arena slot (exact capacity accounting preserved).
- `ValueMatch` is retained for payload extraction inside an arm.
- The trampoline loop exits when it reaches a block whose term is `Halt` (no
  sentinel block id needed — `Halt` is a `ValueTerm` variant).
- `BranchCtor` on a `None` slot (uninitialized wire scrutinee) defensively takes
  `els` → eventual `ProcessError`.

## 6. Lowering

Emission switches from `self.value_instrs.push(...)` to
`self.blocks[self.cur_block].instrs.push(...)`; `lower_value` keeps returning a
result register.

**`if`:**

```
cond_reg = lower(cond)
entry:  Branch { cond: cond_reg, then: then_b, els: else_b }
then_b: lower(then) -> r1; ValueMove { dst: out, src: r1 }; Fallthrough(join)
else_b: lower(els)  -> r2; ValueMove { dst: out, src: r2 }; Fallthrough(join)
join:   out
```

**Static fast path:** if the condition is a statically-known `Bool` (literal or
resolvable CAF — same machinery as `static_scrutinee_ctor`), lower only the
taken branch (no `Branch`); the existing exact-capacity behavior is preserved.

**`match`:** a chain of test blocks terminating in `fail_b` (sets
`value_error = ProcessError`; `Halt`):

- `Var`/`Wild` — no test; binding registered in the body scope.
- `Ctor(name, [p1..pk])` — in the test block: `BranchCtor { slot, ctor: idx(name),
  then: bind_b, els: next_test }`; in `bind_b`: `ValueMatch` extracts the
  payload regs, then `lower_pattern` recurses over `p1..pk` against them
  (nested `Just (Left x)` = a `BranchCtor`→`ValueMatch` chain).
- `Lit(v)` — constant + `ValueCompare { Eq }` → `Branch` on the Bool reg.
- Guards — one per guard: `guard_reg` → `Branch { guard_reg, then: body_b, els:
  next_guard_or_arm }`.
- Body: `ValueMove { dst: out, src: r }` → `Fallthrough(join)`.
- Dispatch: for sums, sequential `BranchCtor` tests per arm (no jump table in
  v1; `DispatchCtor` is a future `jit` option). For scalars, the first arm's
  literal test block is the entry.

**Static fast path (existing):** `static_scrutinee_ctor` — when the scrutinee's
constructor is statically known, lower only the matching arm (preserving the
"exactly one `ValueMatch`" unit tests in `lower.rs:3819-3846`). The
"not statically resolvable in v1" compile error is **removed**; such scrutinees
now lower to runtime dispatch.

## 7. Interpreter (trampoline)

`run_value_track` (`interp.rs:130`) becomes:

```rust
let blocks = mem::take(&mut prog.ir.value_blocks);
let mut cur = prog.ir.value_entry;
while cur != HALT {
    let b = &blocks[cur];
    for i in &b.instrs { exec_value_instr(prog, i, &mut drops); }
    cur = match b.term {
        Fallthrough(n) => n,
        Branch { cond, then, els } => if is_true(prog, cond) { then } else { els },
        BranchCtor { slot, ctor, then, els } => if tag(prog, slot) == ctor { then } else { els },
        Halt => HALT,
    };
}
prog.ir.value_blocks = blocks;
```

- `mem::take`/restore, `drops_scratch` mark/drain, `value_error` latch — unchanged.
- `run_fragment` uses the same loop over `frag.value_blocks` with the register
  `base` offset: `remap_value_instr` (unchanged) plus the term's register fields
  (`cond`, `slot`) offset by `base`; fragment-local block ids are untouched.
  Nested fragments = nested trampolines, depth bounded by the existing static
  `max_call_regs` contract → Rust stack stays O(1).
- A non-`Bool`/`None` `Branch` condition is treated as `false` (defensive,
  deterministic; static typing guarantees `Bool`).

## 8. RT safety and capacity

- Blocks are pre-built at compile time; the trampoline is a stack-local loop;
  registers pre-allocated; drops deferred on the pre-sized `drops_scratch`. No
  allocation, locks, or syscalls on the RT path.
- Arena capacity accounts for **all** blocks statically (worst case across both
  branches) — the task 9.1 model. `ValueMove` transfers ownership at joins, so
  no extra slots are pinned by the un-taken branch.
- `max_drops` (`program.rs:391`) sums block instruction counts instead of
  `value_instrs.len()`.

## 9. Tests

End-to-end (`tests/branching.rs`, `tests/match_patterns.rs`):
- `if` static `true`/`false`; result binding; nested in arithmetic.
- `if` with runtime `Bool` from `SetParameter`: `main g = if g > 0.5 then 1.0
  else 2.0;` → default `2.0`; after `set_param(g, 1.0)` the next tick yields
  `1.0` (the user's core scenario).
- `if` from value-state feedback (`c = ~ (c + 1.0)`) switching across ticks
  without parameters.
- `if` + runtime `match` together, driven by `SetParameter`:
  `match (if g > 0.5 then Just 1.0 else Nothing) of { Just x => x; Nothing => 0.0; }`.
- Runtime `match` on non-resolvable scrutinees — the two repurposed tests
  (`tests/collections_list.rs:234`, `lower.rs:3861`) now compile and dispatch:
  `match (head (filter ...)) of { ... }` → `Float(2.0)`; `match _ of { ... }`.

Patterns: literal `Int`/`Float`/`Bool`/`String`; wildcard; `Var` binding whole
value; nested `Just (Left x)`; `Bool` totality without wildcard.

Guards: evaluation order + fallthrough; guard referencing a bound variable;
guarded arm runtime `ProcessError` (guarded pattern matched, guard failed, next
arm's pattern mismatched).

Compile errors: missing ctor, scalar without wildcard, guarded arms without an
unguarded fallback, ctor arity mismatch, unknown ctor, `if` non-Bool cond,
branch type mismatch, signal-rate scrutinee.

Regression + unit: all existing suites stay green (`data_sums`, `hkt_typeclass`,
`bool_values`, `collections_*`, `func_values`, `closures`, `main_cells`,
`currying_wires`, …); block structure / `ValueMove` / `BranchCtor` /
`remap_value_term` unit tests; static fast-path "exactly one `ValueMatch`";
exact arena capacity across both branches; `render.rs` round-trip for the new
AST nodes.

## 10. Out of scope

- Signal-track selection (branching a whole signal block) — a separate
  block-level `select` feature.
- `Bool`/sum parameters via `SetParameter` (params stay `Float | Int`; sums
  become runtime only via functions/state/containers).
- Record patterns (`match` stays on sums and scalars; records use field
  projection).
- `DispatchCtor` jump table (v1 uses sequential `BranchCtor` tests).
- Loops / general recursion (the block CFG is designed to host them later).
- Loopless fn-pointer execution (rejected; see §2.3).

## 11. Impacted files

`ast.rs`, `lexer.rs`, `parser.rs`, `types/infer.rs`, `reduce.rs` (walk new
nodes in beta/CAF inlining), `lower.rs`, `ir.rs`, `backend/interp.rs`,
`render.rs`, `program.rs` (stats), plus new tests and `README.md`/CHANGELOG
documentation.