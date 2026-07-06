# rill-lang: the Signal DSL

`rill-lang` is a small, Faust-style functional streaming language for describing
the internal math of a signal-graph node. You write a block diagram as source
text; `rill-lang` compiles it — lexer, parser, Hindley-Milner type checker,
linear IR, interpreter — into a value implementing
[`rill_core::Algorithm`](../architecture/core.md), ready to run in the graph.

The current backend is a safe, allocation-free **interpreter** that evaluates
the program one sample at a time. A Cranelift JIT backend is planned behind a
future `jit` feature; it will consume the same intermediate representation, so
nothing in the language front-end changes when it arrives.

> This page is the *language* reference. For the broader idea of embedding
> domain-specific languages in rill, see the [eDSL guide](dsl.md).

## A first program

```rust,no_run
use rill_lang::compile;
use rill_core::traits::Algorithm;

let mut prog = compile::<f32>("process = _ * 0.5;").unwrap();
let mut out = [0.0f32; 4];
prog.process(Some(&[1.0, 2.0, 4.0, 8.0]), &mut out).unwrap();
assert_eq!(out, [0.5, 1.0, 2.0, 4.0]);
```

A program is a sequence of definitions, each terminated by `;`. Exactly one must
be named `process` — it is the entry point. Because it maps onto the single
input / single output of `Algorithm::process`, `process` must reduce to an arity
of **(0 or 1) → 1**.

## Definitions and functions

```faust
gain(x) = x * 0.5;     // a function of one sub-diagram argument
process  = gain(_);     // apply it to the input wire
```

Function definitions are let-generalized: a polymorphic definition can be
instantiated at different scalar types at each use site. Inside an application's
argument list, commas separate arguments; elsewhere `,` is the parallel
combinator (wrap a parallel pair in parentheses to pass it as one argument:
`f((a , b), c)`).

## Primitives

| Syntax | Meaning | Arity (in → out) |
|---|---|---|
| `_` | identity wire | 1 → 1 |
| `!` | cut (discards its input) | 1 → 0 |
| `42` | integer literal | 0 → 1 |
| `1.5` | float literal | 0 → 1 |
| `+` `-` `*` `/` `%` | binary arithmetic block | 2 → 1 |
| `sin` `cos` `tan` `sqrt` `exp` `ln` `tanh` `abs` | math builtins | 1 → 1 |
| `min` `max` | selection | 2 → 1 |

Arithmetic also appears in infix position: `_ * 0.5` and `_ + 1` build the same
blocks as `*` and `+` used as primitives.

## Combinators

The block-diagram algebra composes diagrams. For `A : (aᵢ, aₒ)` and
`B : (bᵢ, bₒ)`:

| Form | Name | Requirement | Resulting arity |
|---|---|---|---|
| `A : B` | sequential | `aₒ = bᵢ` | `(aᵢ, bₒ)` |
| `A , B` | parallel | — | `(aᵢ + bᵢ, aₒ + bₒ)` |
| `A <: B` | split (fan-out) | `bᵢ` is a multiple of `aₒ` | `(aᵢ, bₒ)` |
| `A :> B` | merge (fan-in, sums) | `aₒ` is a multiple of `bᵢ` | `(aᵢ, bₒ)` |
| `A ~ B` | feedback | `bᵢ ≤ aₒ` and `bₒ ≤ aᵢ` | `(aᵢ − bₒ, aₒ)` |
| `A @ n` | integer delay | `A` is `_ → 1`, `n` a constant int | same as `A` |

Feedback (`~`) routes `B`'s outputs back into `A`'s leading inputs through a
one-sample delay — this is how stateful filters and recursive structures are
built. The delay operator `@` requires a compile-time constant integer length
(constant-folded from integer literals and arithmetic on them); variable delays
are not part of the MVP.

### Operator precedence

Loosest to tightest binding, all left-associative:

```text
~   <   :   <   :>   <   <:   <   ,   <   + -   <   * / %   <   @   <   unary -   <   atom
```

So `+ ~ _` parses as `(+) ~ (_)`, and `_ * 2 , _` as `(_ * 2) , _`.

### Idioms

```faust
process = + ~ _;             // integrator:        y[n] = x[n] + y[n-1]
process = + ~ (_ * 0.5);     // leaky integrator:  y[n] = x[n] + 0.5·y[n-1]
process = _ @ 1;             // one-sample delay
process = _ <: (_ , _) :> +; // fan out, then sum  = 2·x
process = abs(_);            // full-wave rectifier
```

## Type checking

`rill-lang` runs a Hindley-Milner inference pass before code generation:

- **Scalar types** — `int`, `float` (the runtime `T`), and type variables — are
  unified with an occurs check. Overloaded operators default to the runtime
  scalar when otherwise unconstrained, so arithmetic is monomorphized.
- **Arities** are synthesized bottom-up as concrete numbers and checked against
  the combinator table above.
- **Named functions** are let-generalized and instantiated per use site.

A type or arity mismatch is reported as an error carrying the offending source
span, and compilation stops there — an ill-typed diagram never reaches the
interpreter.

```rust,no_run
use rill_lang::compile;
// top-level parallel pair is (2 → 2): not a valid `process`
assert!(compile::<f32>("process = _ , _;").is_err());
```

## Built-in functions

rill-lang programs can call stateful DSP/model built-ins from
`rill-core-dsp`/`rill-core-model` via an extensible FFI registry. Built-ins are
**not** compiled into the interpreter core — bindings live in the umbrella crate
`rill-adrift`, keeping `rill-lang` dependent only on `rill-core`.

### Calling convention

A built-in is called like a function with constant-parameter arguments; the
signal wire connects from the left via the sequential combinator `:`:

```faust
process = _ : lowpass(1000.0, 0.7);
```

Parameters are **compile-time constants** (float or integer literals, optionally
with arithmetic). They are folded to `f64` during lowering and passed to the
built-in constructor. A built-in has arity `(signal_ins → signal_outs)`; in this
release `signal_outs` is always 1.

### Sample built-ins vs block built-ins

| Kind | Names | Behaviour | Inside `~` |
|---|---|---|---|
| **Sample** | `onepole`, `moog` | Per-sample state; the built-in's `process_sample` runs inside the sample-level recurrence loop. | Allowed |
| **Block** | `lowpass`, `highpass`, `analog_moog` | Opaque whole-buffer step; the built-in implements `Algorithm<T>` and processes all samples at once. | Compile error |

Sample built-ins are composed from the feedback combinator just like hand-rolled
recurrences:

```faust
process = + ~ moog(500.0, 0.5);   // feedback-legal per-sample filter
```

Block built-ins cannot appear inside `~` — the compiler rejects them with an
error (`block built-in cannot be used inside a feedback loop`).

### Using built-ins from Rust

```rust,no_run
use rill_lang::compile_with;
use rill_adrift::lang_builtins::full_registry;

let reg = full_registry::<f32>();
let mut prog = compile_with::<f32>("process = _ : onepole(200.0, 0.7);", &reg, 48_000.0).unwrap();
let mut out = [0.0f32; 4];
prog.process(Some(&[1.0, 2.0, 4.0, 8.0]), &mut out).unwrap();
```

The `full_registry()` includes all DSP built-ins. `analog_moog` requires
`rill-adrift`'s `analog` feature. The default `compile()` function has no
built-ins — it uses an empty registry and is unchanged.

## Serialization

With the `serde` feature enabled, a program round-trips through
[`RillLangDef`], whose canonical form is simply the **source string** (a compiled
IR would rot across versions; source stays stable and editable):

```rust,no_run
use rill_lang::{RillLangDef, compile_def};

let def = RillLangDef::new("gain", "process = _ * 0.5;");
let mut prog = compile_def::<f32>(&def).unwrap();
```

## Using it in a graph

The umbrella crate `rill-adrift` exposes `rill-lang` behind its `lang` feature
and registers a `rill/lang` node type. A serialized graph can then embed a
rill-lang block by giving it a `source` parameter:

```json
{
  "id": 0,
  "type_name": "rill/lang",
  "name": "MyBlock",
  "parameters": { "source": "process = _ * 0.5;" }
}
```

Setting the node's `source` parameter at runtime recompiles the program and
hot-swaps it — the seed of the runtime code-synthesis loop described in the
project's architecture notes. Note that compilation allocates, so a `source`
swap applied through the graph's `SetParameter` path runs inside the I/O
callback; treat it as a control-time operation to be performed when the graph is
not under hard real-time load, not as an every-block action.

## Execution model and performance

The interpreter compiles the linear IR into an **execution schedule** via SCC
(strongly-connected component) analysis of the data-dependency graph. Each step
in the schedule is classified as either a whole-buffer block op or a per-sample
recurrent region:

- **Feedforward regions** — all combinational instructions (arithmetic, math
  builtins, fan-out/fan-in) — are `Step::Block` and run **whole-buffer** through
  the `rill_core::math::vector` SIMD eDSL (`ScalarVector4`). The block path
  computes directly in `T` (the runtime scalar, e.g. `f32`), letting LLVM
  auto-vectorize the hot loop.
- **Recurrent regions** — anything containing `~` (feedback) or `@` (delay)
  operators that introduce a cross-sample dependency — are `Step::Sample` and
  run as a tight per-sample loop over the instructions in their original IR
  order. Only the recurrence itself goes sample-by-sample; all upstream
  feedforward math stays block-wise.

The whole-buffer register store is a flat `Vec<Vec<T>>` grown once to the block
length and reused across calls. The hot `process()` path performs no heap
allocation, no locks, and no syscalls, honoring rill's real-time rules.

A fully-combinational program (e.g. `_ * 0.5`) compiles to all block steps. A
pure feedback program (e.g. `+ ~ _`) degenerates to a single per-sample region.
Mixed programs (e.g. `(_ * 0.5) : (+ ~ _)`) schedule the feedforward block
steps first, then the recurrent sample region.

The per-sample interpreter is retained as `RillProgram::process_reference` — a
numerical oracle used by tests to validate the hybrid path. A Cranelift JIT
backend is still planned and will reuse the same IR.

## Status

MVP. Deferred to follow-on work: the Cranelift `jit` backend, foreign references
to existing rill DSP primitives (biquads, oscillators), and a SIMD-aware IR.

[`RillLangDef`]: https://docs.rs/rill-lang
