# rill-lang — Faust-style Signal DSL (interpreter backend)

> **Status:** Design — awaiting user review, then implementation plan.
> **Date:** 2026-07-06
> **Spec source:** `docs/src/guides/rill-lang.md` §3 (the DSL/JIT core).
> **Scope:** This document covers **§3 only** — the `rill-lang` crate. The
> transactional learning loop (§4), runtime (§2, already largely implemented),
> and hardware profiles (§5) are out of scope for this effort.

## Motivation

`rill-lang` is a high-level, domain-specific, functional streaming language for
describing the internal mathematical structure of graph node algorithms. It lets
node math be authored (and, later, machine-synthesised) as compact block-diagram
source rather than hand-written Rust, then compiled at runtime into a value that
plugs into the existing `rill_core::Algorithm<T>` contract.

The spec (§3.3) ultimately mandates a **Cranelift JIT** backend. This effort
deliberately targets an **interpreter first**: it gets the lexer, parser,
Hindley-Milner type system, and runtime integration correct and lets the language
design stabilise before the heavier, `unsafe`, dependency-laden JIT is added.

### Why interpreter-first

- **Goal:** next-generation AI for edge (non-anthropomorphic, symbolic-reactive).
  Even at the interpreter's realistic performance (≈5–15× slower than the future
  JIT), throughput remains competitive with existing edge solutions dominated by
  Python + large neural networks, given the same hardware constraints.
- **Dependency discipline:** Cranelift is a large dependency tree and requires
  `unsafe` (mmap page + fn-ptr transmute). Per `AGENTS.md`, both need explicit
  sign-off. Deferring it keeps the MVP dependency surface at essentially zero.
- **Shared lowering target:** the interpreter and the future JIT consume the same
  flat linear IR, so adding the JIT later does not touch the front-end.

## Confirmed decisions

| Dimension | Decision |
|---|---|
| Backend | Trait-based. Safe interpreter first; Cranelift behind a future `jit` feature. |
| Front-end | Hand-written lexer + Pratt (operator-precedence) parser. **0 new deps.** |
| Type system | Full Hindley-Milner (unification + let-polymorphism) over scalar types **and** arities. |
| Language surface | Core Faust subset + feedback `~` + integer delay `@` + named defs/abstraction. |
| Top-level arity | `process` must reduce to arity **(0 or 1) → 1** to fit `Algorithm`'s single in/out buffer. |
| Execution | Typed AST → flat linear IR → **sample-by-sample** interpreter; state pre-allocated (RT-safe). |
| Serialization | `serde`-gated `RillLangDef { source }` + `compile_def`; source string is canonical form. |
| Umbrella | `rill-adrift` `lang` feature: re-export + `"rill/lang"` factory node. |

## Compilation pipeline

```
source ──▶ Lexer ──▶ Parser ──▶ AST ──▶ Type Check (HM) ──▶ Typed AST
                                                                │
                                                          Lower to IR
                                                                │
                                                                ▼
                                                     Linear IR (flat op list
                                                     + state slots)
                                                                │
                                           ┌────────────────────┴───────────┐
                                    Backend::build                    (future: jit.rs)
                                           │
                                           ▼
                                  RillProgram<T>  ──impl──▶  Algorithm<T>  (rill-core)
```

Each stage is a separate module with a clean interface, understandable and
testable in isolation.

## Execution model & performance

Feedback (`~`) introduces a 1-sample dependency, so the interpreter evaluates the
IR **sample-by-sample** in a tight loop over the block, threading state slots
between samples. This is a **bytecode-VM-class interpreter (~5–15× slower than the
future JIT)** — not the optimistic 2× of a whole-buffer block interpreter, but
correct in the presence of feedback and a faithful match to Faust's own
per-sample inner-loop execution model.

**RT safety.** All state — feedback registers and `@` delay lines — is a fixed set
of slots sized and allocated at compile time. The hot `process()` path performs
**zero heap allocation, no locks, no syscalls**, honouring the runtime rules in
`AGENTS.md` (§ Real-time safety). `RillProgram<T>` owns its state and IR buffers.

## Language surface (MVP)

- **Literals** — integer and float.
- **Wire / cut** — `_` (identity wire, arity 1→1), `!` (cut, arity 1→0).
- **Arithmetic** — `+ - * / %`, unary negation.
- **Ordering / selection** — `min`, `max`, comparisons.
- **Math builtins** (from `Transcendental`) — `sin cos tan sqrt exp ln tanh abs`.
- **Combinators** — `:` (sequential), `,` (parallel), `<:` (split / fan-out),
  `:>` (merge / fan-in), `~` (feedback with implicit 1-sample delay).
- **Integer delay** — `@` (needed for feedback to be useful).
- **Definitions & abstraction** — `f(x) = ...;`, `process = ...;`, grouping `(...)`.
- **Top-level constraint** — `process` must have arity **(0 or 1) → 1**. Parallel
  branches (`,`) must merge back (`:>`) before the output.

Explicitly out of scope for the MVP: foreign references to existing rill DSP
primitives (biquad, oscillators), `with { }` scoped environments beyond simple
defs, pattern matching, and multi-output top-level programs.

### Combinator arity algebra

For terms `A : (aᵢ, aₒ)` and `B : (bᵢ, bₒ)`:

| Form | Constraint | Result arity |
|---|---|---|
| `A : B` | `aₒ = bᵢ` | `(aᵢ, bₒ)` |
| `A , B` | — | `(aᵢ + bᵢ, aₒ + bₒ)` |
| `A <: B` | `bᵢ` is a multiple of `aₒ` | `(aᵢ, bₒ)` |
| `A :> B` | `aₒ` is a multiple of `bᵢ` | `(aᵢ, bₒ)` |
| `A ~ B` | `bᵢ ≤ aₒ` and `bₒ ≤ aᵢ` | `(aᵢ − bₒ, aₒ)` |

These constraints are discharged by the HM unifier (arities are type-level
naturals with variables), not by a separate ad-hoc pass.

## Type system (HM, §3.2)

Unification-based Algorithm W over two sorts:

- **Scalar types** — `int`, `float` (= the runtime `T`), and type variables `α`.
- **Arities** — `(ins, outs)` as type-level naturals with variables.

Features:

- **Occurs check** on unification.
- **Let-generalisation** for named definitions, so `f(x) = x + 1` is polymorphic
  and can be instantiated at different types per use site.
- **Ad-hoc operator overloading** (`+`, `*`, `min`, `max`, …) resolved by
  defaulting to the runtime `T` when otherwise unconstrained. Math is
  monomorphised at compile time.
- **Diagnostics** — a mismatch (bad arity, `int` where a signal is required, an
  unadapted port, …) is a compile error carrying a source span. Codegen is
  blocked on any type error (the "sanitiser" of §3.2).

The type checker is a standalone pass; the future JIT reuses it unchanged.

## Module layout

```
rill-lang/
  Cargo.toml
  src/
    lib.rs            # crate docs + re-exports
    prelude.rs        # public prelude (per crate convention)
    error.rs          # CompileError (thiserror) + source spans
    lexer.rs          # hand-written tokeniser (tracks spans)
    ast.rs            # AST node types
    parser.rs         # recursive-descent + Pratt operator-precedence parser
    types/
      mod.rs
      ty.rs           # Type, TypeVar, Arity, scheme
      env.rs          # type environment / contexts
      unify.rs        # unification + occurs check
      infer.rs        # Algorithm W (inference + generalisation)
    ir.rs             # flat linear IR (ops + state slot table)
    lower.rs          # typed AST -> IR
    backend/
      mod.rs          # Backend trait (build IR -> RillProgram)
      interp.rs       # sample-by-sample IR evaluator
      # jit.rs        # (future) Cranelift backend, `jit` feature
    program.rs        # RillProgram<T> : Algorithm<T>
    serde_def.rs      # RillLangDef (serde feature) + compile_def
```

## Dependencies

- **Runtime:** `rill-core` (workspace) for `Transcendental`, `Algorithm`,
  `ProcessResult`; `thiserror` for error types. **No new third-party crates.**
- **Optional `serde` feature:** `serde` (derive) for `RillLangDef`.
- **Deferred `jit` feature (separate approval):** the `cranelift-*` family.
- Register the crate in workspace `members` and `workspace.dependencies` at
  `0.5.0-beta.7` (crates version synchronously).

## Public API

```rust
use rill_core::math::Transcendental;

/// Compile DSL source into a runnable program for a concrete runtime scalar T.
pub fn compile<T: Transcendental>(src: &str) -> Result<RillProgram<T>, CompileError>;

/// RillProgram<T> implements rill_core::Algorithm<T> and owns its state/IR.
pub struct RillProgram<T> { /* ... */ }
```

## Serialization (forward-looking, `serde` feature)

A rill-lang program is fully described by its **source string**. The compiled
AST/IR is an implementation detail that would rot across versions; the source is
stable and human-editable, matching the project's "JSON for manual editing"
philosophy.

```rust
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RillLangDef {
    pub format_version: String, // "rill-lang/1"
    pub source: String,         // the DSL source — canonical serialized form
    pub name: String,
}

pub fn compile_def<T: Transcendental>(def: &RillLangDef)
    -> Result<RillProgram<T>, CompileError>;
```

This is the hook for the future §4 hot-swap / learning loop:
generate source → `compile_def` → swap the node's algorithm.

## `rill-adrift` integration

- Add optional dep `rill-lang` behind a new `lang` cargo feature; `pub use
  rill_lang;` re-export.
- Provide a `NodeFactory` constructor keyed `"rill/lang"` that reads a `source`
  string parameter and compiles it into a graph node. The adapter that wraps
  `RillProgram<T>` as a graph `Processor` lives in `rill-adrift`, keeping
  `rill-lang` free of a `rill-graph` dependency.

## Testing (TDD)

- **Per-stage unit tests:**
  - Lexer — token stream + spans for representative source.
  - Parser — precedence/associativity of `: , <: :> ~ @` and arithmetic; grouping.
  - Type checker — arity algebra, let-polymorphism, overload defaulting, and
    rejection of ill-typed programs with a span.
  - Lowering — AST → IR shape, state-slot allocation for `~` and `@`.
- **Integration tests** — interpreter output vs. hand-computed references, e.g.:
  - `process = _ * 0.5;` halves the input.
  - `process = + ~ _;` is an integrator (accumulator).
  - `process = _ @ 1;` is a one-sample delay.
  - Arity/type errors (`process = _ , _;` at top level; `int`-vs-signal misuse)
    are rejected at compile time.
- **RT-safety check** — assert no allocation in `process()` (review + a
  smoke test under a virtual device per `AGENTS.md`).

## Out of scope / follow-ons

1. **Cranelift JIT** (`jit` feature) — the spec's §3.3 performance target.
2. **§4 transactional learning loop** — snapshot / commit / rollback of hypothesis
   subgraphs; energy-blowup (spike-storm) and OWL-axiom guards.
3. **Foreign DSP block references** — calling existing rill primitives from source.
4. **SIMD-aware IR** — mapping IR ops onto `ScalarVector4` / `AVX-512` / `RVV`.
```

