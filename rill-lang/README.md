# rill-lang

A Faust-style functional streaming DSL that compiles to a
[`rill_core::Algorithm`]. Programs describe the internal mathematical structure
of a signal-graph node as a compact block diagram; `rill-lang` compiles that
source at runtime into a value you can drop straight into the rill graph.

The first backend is a **safe, allocation-free interpreter**. A Cranelift JIT
backend is planned behind a future `jit` feature; both share the same linear IR,
so the language front-end is unaffected when the JIT lands.

## Example

```rust
use rill_lang::compile;
use rill_core::traits::Algorithm;

// A half-gain: y[n] = x[n] * 0.5
let mut prog = compile::<f32>("process = _ * 0.5;").unwrap();

let mut out = [0.0f32; 4];
prog.process(Some(&[1.0, 2.0, 4.0, 8.0]), &mut out).unwrap();
assert_eq!(out, [0.5, 1.0, 2.0, 4.0]);
```

## The language in one screen

A program is a list of definitions ending in `;`. One must be named `process`,
and it must reduce to arity **(0 or 1) → 1** so it fits the single-input,
single-output `Algorithm::process` contract.

```faust
gain(x) = x * 0.5;          // a one-argument function
process  = gain(_);          // apply it to the input wire
```

### Primitives

| Syntax | Meaning | Arity |
|---|---|---|
| `_` | identity wire | 1 → 1 |
| `!` | cut (discard a wire) | 1 → 0 |
| `3`, `3.5` | int / float literal | 0 → 1 |
| `+ - * / %` | arithmetic (as a block) | 2 → 1 |
| `sin cos tan sqrt exp ln tanh abs` | math builtins | 1 → 1 |
| `min max` | selection | 2 → 1 |

### Combinators (block-diagram algebra)

| Operator | Name | Constraint | Result arity |
|---|---|---|---|
| `A : B` | sequential | `out(A) = in(B)` | `(in A, out B)` |
| `A , B` | parallel | — | `(in A + in B, out A + out B)` |
| `A <: B` | split / fan-out | `in(B)` multiple of `out(A)` | `(in A, out B)` |
| `A :> B` | merge / fan-in (sums) | `out(A)` multiple of `in(B)` | `(in A, out B)` |
| `A ~ B` | feedback (1-sample delay) | `in(B) ≤ out(A)`, `out(B) ≤ in(A)` | `(in A − out B, out A)` |
| `A @ n` | integer delay (`n` const) | `A` is `_ → 1` | same as `A` |

Precedence, loosest → tightest: `~` < `:` < `:>` < `<:` < `,` < `+ -` < `* / %` < `@` < unary `-` < atoms.

### Idioms

```faust
process = + ~ _;            // integrator:     y[n] = x[n] + y[n-1]
process = + ~ (_ * 0.5);    // leaky integrator: y[n] = x[n] + 0.5·y[n-1]
process = _ @ 1;            // one-sample delay
process = _ <: (_ , _) :> +; // fan-out then sum = 2·x
```

## Type system

Types are inferred with a Hindley-Milner core: scalar types (`int`, `float`,
type variables) are unified with an occurs check and let-generalized for named
functions; wire arities are synthesized bottom-up and checked against the
combinator algebra. Any mismatch is a compile error with a source span, and code
generation is blocked — so an ill-formed diagram never reaches the runtime.

## Serialization

With the `serde` feature, [`RillLangDef`] carries a program as its **source
string** (the canonical, human-editable form) and [`compile_def`] turns it back
into a runnable program:

```rust,ignore
use rill_lang::{RillLangDef, compile_def};

let def = RillLangDef::new("gain", "process = _ * 0.5;");
let prog = compile_def::<f32>(&def).unwrap();
```

## Graph integration

The `rill-adrift` umbrella crate exposes `rill-lang` behind its `lang` feature,
including a `rill/lang` factory node that reads a `source` parameter — so a
serialized graph can embed a rill-lang block directly.

## Execution model

The interpreter compiles the linear IR into a hybrid schedule via SCC analysis:
feedforward regions run whole-buffer through the `rill_core::math::vector` SIMD
eDSL, while feedback (`~`) and delay (`@`) recurrences run per-sample. The block
path computes in `T` with zero heap allocation on the hot path. A Cranelift JIT
backend is still planned and will reuse the same IR.

## Built-in functions

rill-lang supports calling stateful DSP/model built-ins from
`rill-core-dsp`/`rill-core-model` via `compile_with(src, &registry, sample_rate)`.
Parameters are compile-time constants (`_ : lowpass(1000.0, 0.7)`), signals flow
via combinators. Per-sample built-ins (`onepole`, `moog`) are feedback-legal;
whole-buffer built-ins (`lowpass`, `highpass`, `analog_moog`) are opaque block
steps and cannot appear inside `~`. Bindings live in `rill-adrift`
(`lang_builtins::full_registry`, `analog_moog` behind the `analog` feature).

## Parameters

rill-lang programs can expose named control-rate parameter slots with
`param("name", default[, min, max])`. Parameters are RT-safe — stored in a flat
array indexed by integer handle — and settable via `RillProgram::set_param` /
`param_index(name)`. On a `rill/lang` graph node they are advertised in
`NodeMetadata` and writable by name via `Node::set_parameter`, so servos, LFOs,
and MIDI mappings can automate them directly. The native `smooth(x, ms)`
one-pole provides zipper-free interpolation when parameters change at block
boundaries.

## Status

MVP. Deferred to follow-on work: the Cranelift `jit` feature, foreign references
to existing rill DSP primitives, and a SIMD-aware IR.

## License

Apache-2.0. See the workspace `LICENSE.md`.

[`rill_core::Algorithm`]: https://docs.rs/rill-core
[`RillLangDef`]: https://docs.rs/rill-lang
[`compile_def`]: https://docs.rs/rill-lang
