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

Setting the node's `source` parameter at runtime recompiles the program on the
control thread and hot-swaps it — the seed of the runtime code-synthesis loop
described in the project's architecture notes.

## Execution model and performance

Because feedback has a one-sample dependency, the interpreter evaluates the IR
sample by sample, threading a fixed set of pre-allocated state slots (feedback
registers and delay lines). The hot `process()` path performs no heap
allocation, no locks, and no syscalls, honoring rill's real-time rules. Expect
the interpreter to be several times slower than the planned JIT; it exists to
get the language correct and to iterate quickly on design.

## Status

MVP. Deferred to follow-on work: the Cranelift `jit` backend, foreign references
to existing rill DSP primitives (biquads, oscillators), and a SIMD-aware IR.

[`RillLangDef`]: https://docs.rs/rill-lang
