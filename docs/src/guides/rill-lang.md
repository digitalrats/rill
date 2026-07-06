# rill-lang: the Signal DSL

`rill-lang` is a small, Faust-style functional streaming language for describing
the internal math of a signal-graph node. You write a block diagram as source
text; `rill-lang` compiles it — lexer, parser, Hindley-Milner type checker,
linear IR, and an execution scheduler — into a value implementing
[`rill_core::Algorithm`](../architecture/core.md), ready to run in the graph.

The current backend is a safe, allocation-free **interpreter**. A Cranelift JIT
backend is planned behind a future `jit` feature; it will consume the same
intermediate representation, so nothing in the language front-end changes when it
arrives.

> This page is the canonical language reference. For the broader idea of
> embedding domain-specific languages in rill, see the [eDSL guide](dsl.md).

## Overview

rill-lang exists so that a node's DSP can be **authored — and, in time,
machine-synthesised — at runtime** rather than hand-written in Rust and compiled
ahead of time. A program is compiled on the fly (no `rustc`, no external
toolchain) to a `rill_core::Algorithm<T>` that plugs straight into the signal
graph. The compiler is tiny and self-contained, and the compiled program obeys
rill's real-time rules: no heap allocation, no locks, and no syscalls on the hot
path.

Three properties define the language:

- **Block-diagram algebra (Faust-style).** Programs are compositions of signal
  processors via geometric combinators (`:` `,` `<:` `:>` `~`), not imperative
  statements. There are no runtime variables — only signals flowing through
  blocks.
- **Hybrid block/sample execution.** The compiler analyses the data-dependency
  graph and runs feedforward regions **whole-buffer** (SIMD) while only true
  recurrences (feedback, delay) run sample-by-sample. See
  [Execution model](#execution-model-and-performance).
- **RT-safe control.** Named parameters and smoothing give control-rate
  automation without recompilation or locks.

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
with arithmetic) or a [`param(...)`](#parameters) reference for dynamic control.
Constant parameters are folded to `f64` during lowering and passed to the
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

## Parameters

rill-lang programs can expose **named control-rate parameters** — mutable slots
that stay constant for one signal block and change only between blocks (at
control rate). Parameters are RT-safe because the compiled program bakes them
into a flat array indexed by integer handle; no allocation, no locking, and no
variable lookup occurs on the hot path.

### `param(name, default[, min, max])`

```faust
process = _ * param("gain", 0.5);
```

`param("gain", 0.5)` creates a named control-rate slot that evaluates to `0.5`
initially. At runtime the value can be modified from the control thread, and the
new value takes effect at the next block boundary. The optional `min` and `max`
arguments constrain the range (`0.0` ≤ `param("gain", 0.5, 0.0, 1.0)` ≤ `1.0`);
the runtime clamps writes to this range.

Reusing the same name refers to the **same slot** — every `param("gain", …)` in a
program shares one value. All uses of a name must declare an **identical** default
and range; a conflicting redeclaration is a compile error (this prevents a name
from silently meaning two different things).

Parameters have arity `0 → 1` — they are zero-input signal sources — so they
can appear anywhere a float literal would: in arithmetic expressions and also as
a built-in argument, which lets you dynamically drive filter cutoffs, resonance,
and mixer gains:

```faust
process = _ : lowpass(param("cutoff", 1000.0, 20.0, 20000.0), 0.7);
```

### `smooth(x, ms)` — zipper-free smoothing

When a parameter changes abruptly at a block boundary, the step creates an
audible "zipper" click. `smooth(x, ms)` is a native one-pole low-pass (one per
call site) that slides its input value toward its output with the specified time
constant:

```faust
process = _ * smooth(param("gain", 0.5, 0.0, 1.0), 10.0);
```

Here `gain` is ramped with a 10 ms time constant — the output sample moves
smoothly even when the control thread snaps the parameter from 0 to 1.
`smooth` bakes the sample rate at compile time; if the sample rate changes,
the program must be recompiled for the time constant to match.

### Setting parameters from Rust

The `RillProgram` API exposes parameter slots by index:

```rust,no_run
use rill_lang::compile;

let mut prog = compile::<f32>("process = _ * param(\"gain\", 0.5);").unwrap();

let idx = prog.param_index("gain").unwrap();
prog.set_param(idx, 0.8);
```

### Setting parameters on a `rill/lang` graph node

When the program runs inside a `rill/lang` factory node (via `rill-adrift`'s
`lang` feature), parameters are also accessible by **name** from the control
side — the node's `NodeMetadata` advertises them, and you can write a value
with `Node::set_parameter(name, value)`. Because the parameter name is
stable (the same string you wrote in the DSL), servos, LFOs, envelopes, and
MIDI mappings can target it directly (target by `NodeId` + parameter name).

```rust,no_run
// conceptual: a servo targets the "cutoff" parameter of node 0
node_ref.set_parameter("cutoff", 2000.0);
```

### Control-rate semantics

Parameters and `smooth` are control-rate constructs. The compiled program
stores one scalar per parameter slot; `process()` reads the current value once
per call and re-uses it for the entire block. The control thread (`set_param` /
`set_parameter`) writes a new value, and the read is observed at the next
`process()` call — i.e. at the next block boundary. This model is efficient
(the hot path is a simple load + multiply / load + onepole) and safe (no locks,
no atomics).

For more on the automation plumbing, see the [Automaton guide](world-of-automatons.md).

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

## Benchmarks

`rill-lang` ships [criterion](https://github.com/bheisler/criterion.rs)
benchmarks:

```bash
cargo bench -p rill-lang   --bench lang_bench
cargo bench -p rill-adrift --features lang --bench lang_dsp_bench
```

The figures below are representative (256-sample blocks, `f32`, one core).
**Absolute times are machine- and build-dependent — read the ratios, not the
nanoseconds.**

### Compilation (full pipeline: lex → parse → HM → lower → schedule)

| Program | Compile |
|---|---|
| `_ * 0.5` | ~1.4 µs |
| `_ * 0.5 : abs : (_ * 2.0)` | ~2.1 µs |
| `+ ~ (_ * 0.5)` | ~1.9 µs |
| mixed fan-out + feedback | ~3.8 µs |

Compilation is microseconds — cheap enough to recompile a node's source on the
control thread when its `source` parameter changes.

### Runtime — one 256-sample block

| Program | Time | Kind |
|---|---|---|
| `_ * 0.5` | ~63 ns | feedforward (block) |
| `_ * 0.5 : abs : (_ * 2.0)` | ~111 ns | feedforward (block) |
| `_ <: (_ , _ * 0.5) :> +` | ~88 ns | feedforward (block) |
| `_ * param("g", 0.5)` | ~61 ns | feedforward (block) |
| `_ @ 4` | ~1.6 µs | recurrent (sample) |
| `+ ~ (_ * 0.5)` | ~3.4 µs | recurrent (sample) |
| `_ * smooth(param("g", 0.5), 10.0)` | ~4.5 µs | recurrent (sample) |

### Hybrid vs. the per-sample reference — the block-processing win

Comparing `process` (the hybrid block/sample executor) with
`process_reference` (the per-sample oracle) on the same program:

| Program | Hybrid | Reference | Speedup |
|---|---|---|---|
| feedforward chain | ~165 ns | ~5.5 µs | **~33×** |
| feedback | ~3.4 µs | ~4.2 µs | ~1.2× |

Feedforward programs run whole-buffer through the SIMD eDSL and are an order of
magnitude faster than sample-by-sample evaluation. Feedback programs are near
parity, because the recurrence forces both paths to go sample-by-sample — which
is exactly why the scheduler isolates recurrences and blocks everything else.

### Built-ins (via `rill-adrift`)

| Program | Time |
|---|---|
| `_ : lowpass(1000.0, 0.7)` (block Biquad) | ~275 ns |
| `_ : lowpass(param("cutoff", 1000.0), 0.7)` (dynamic) | ~306 ns |
| `_ : onepole(1200.0, 0.5)` (sample) | ~3.5 µs |
| `_ : moog(800.0, 0.6)` (sample) | ~4.0 µs |
| DSL-wrapped biquad vs. raw `Biquad` | ~264 ns vs. ~234 ns (~13% overhead) |

Wrapping a `rill-core-dsp` filter in the DSL costs about 13% over calling the raw
`Algorithm` — the price of the schedule dispatch and the register store. Driving
a filter parameter with `param(...)` adds only the per-block coefficient update.

## Status

The language is feature-complete for signal authoring: block-diagram
combinators, feedback and delay, Hindley-Milner types, hybrid block/sample
execution, a DSP/model built-in registry, and RT-safe named parameters with
smoothing.

Deferred to follow-on work:

- the **Cranelift `jit`** backend (the linear IR is the shared lowering target);
- **whole-graph-as-one-program** lowering (fusing a multi-node graph into one
  schedule);
- **audio-rate** (per-sample) modulation of imported built-in parameters
  (current parameter modulation is control-rate/per-block);
- composed expressions as built-in arguments and multi-output built-ins.

[`RillLangDef`]: https://docs.rs/rill-lang
