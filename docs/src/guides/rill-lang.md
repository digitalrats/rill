# rill-lang: the Signal DSL

`rill-lang` is a small, Faust-style functional streaming language for describing
the internal math of a signal-graph node. You write a block diagram as source
text; `rill-lang` compiles it — lexer, parser, Hindley-Milner type checker,
linear IR, β-reduction, and an execution scheduler — into a value implementing
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

Four properties define the language:

- **Block-diagram algebra (Faust-style).** Programs are compositions of signal
  processors via geometric combinators (`:` `,` `<:` `:>` `~`), not imperative
  statements. There are no runtime variables — only signals flowing through
  blocks.
- **Haskell-style definitions.** Functions and constants use a unified syntax
  (`name args = body`), distinguished only by parameter count. All binding groups
  (`where`, `let`, top-level) have mutual visibility.
- **β-reduction.** User-defined function calls are fully inlined before lowering.
  The final IR is flat — only Wire, constants, built-ins, and combinators remain.
- **Hybrid block/sample execution.** The compiler analyses the data-dependency
  graph and runs feedforward regions **whole-buffer** (SIMD) while only true
  recurrences (feedback, delay) run sample-by-sample.
- **RT-safe control.** Named parameters and smoothing give control-rate
  automation without recompilation or locks.

## A first program

```rust,no_run
use rill_lang::compile;
use rill_core::traits::Algorithm;

let mut prog = compile::<f32>("main = _ * 0.5;").unwrap();
let mut out = [0.0f32; 4];
prog.process(Some(&[1.0, 2.0, 4.0, 8.0]), &mut out).unwrap();
assert_eq!(out, [0.5, 1.0, 2.0, 4.0]);
```

A program is a list of mutually-recursive definitions, each terminated by `;`.
Exactly one must be named `main` — the entry point. `main` must reduce to a
signal block of arity **(0 or 1) → 1**.

## Definitions and functions

rill-lang uses a unified syntax for constants and functions — both are
definitions of the form `name params = body`. The only difference is the
parameter count: 0 params = constant, 1+ params = function:

```faust
gain x = _ * x;    // function of one argument (x is a constant parameter)
main   = gain 0.5; // apply gain to 0.5, producing a (1→1) signal block
```

Function parameters are **Haskell-style λ-parameters**: space-separated
identifiers after the function name, no parentheses. When a function is called
with arguments, the arguments are substituted into the body via β-reduction —
the result is an inlined expression with no runtime function dispatch:

```faust
// Source:
sq x = _ * x;
main = sq 0.5;

// After β-reduction (compile time):
// main = _ * 0.5
```

All binding groups — top-level, `where` blocks, and `let` bodies — are
**mutually recursive**: every name in the group is visible to every body,
regardless of definition order.

### Application syntax

Built-ins and user-defined functions support two calling conventions:

**Parenthesized:** `name(arg1, arg2, ...)` — comma-separated arguments:

```faust
main = _ : lowpass(1000.0, 0.7);
```

**Juxtaposed (Haskell-style):** `name arg1 arg2 ...` — space-separated atoms,
each argument must be an atom (identifier, literal, `_`, `!`, `(expr)`, `-expr`):

```faust
main = _ : lowpass 1000.0 0.7;
```

Both forms are equivalent. Juxtaposition only works when each argument is an
atom — for complex expressions use parenthesized form.

### `where` blocks and layout

Definitions can be attached to any function or constant using the `where`
keyword. Two syntaxes are supported:

**Explicit braces** — definitions inside `{ ... }`, each terminated by `;`:

```faust
main = osc : filt where {
    osc  = sine 440.0 0.5 0.0;
    filt = _ : lowpass 1200.0 0.7;
}
```

**Layout-based (Haskell-style indentation)** — after `where`, each indented line
is a definition. The block starts at the column of the first definition and ends
when indentation drops below that column or at EOF:

```faust
main = osc : filt where
    osc  = sine 440.0 0.5 0.0
    filt = _ : lowpass 1200.0 0.7
```

The semicolon after each definition is **optional** in layout mode — the parser
accepts both `def = expr` and `def = expr;`. The block terminates when the next
line has indent less than the layout column, or when the file ends.

Where-block definitions are **scoped to the function** they're attached to.
They are not visible to other top-level definitions or to the caller.

### `let` expressions

`let` introduces a mutually-recursive binding group scoped to a single
expression. Available in both brace and layout form, like `where`:

```faust
main = let g x = _ * x in g 0.5

main = let { g x = _ * x; } in g 0.5
```

`let` can appear anywhere an expression is expected — inside combinators,
built-in arguments, or nested inside other `let` blocks.

### Multiple definitions at the top level

A program can have any number of top-level definitions:

```faust
gain = _ * 0.5;
main  = gain;
```

Exactly one must be named `main`. All top-level definitions are mutually
recursive and visible to each other.

### `main` with parameters

`main` can declare input parameters — their names become slots in the compiled
`param_map`, addressable by name from the control thread:

```faust
main cutoff res = _ : lowpass cutoff res;
```

When compiled via `compile_graph()`, each `main` parameter and each function
parameter in the `where` block becomes a named parameter in the resulting
graph node. Where-block function parameters are namespaced with dot notation:
`osc.freq`, `filt.cutoff`.

```faust
main = osc : filt : _ where
    osc  freq   = sine freq 0.5 0.0
    filt cutoff = _ : lowpass cutoff 0.7
-- Exposes parameters: "osc.freq", "filt.cutoff"
```

## Primitives

| Syntax | Meaning | Arity (in → out) |
|---|---|---|
| `_` | identity wire | 1 → 1 |
| `!` | cut (discards its input) | 1 → 0 |
| `42` | integer literal | 0 → 1 |
| `1.5` | float literal | 0 → 1 |
| `3i`, `2.5i` | imaginary literal | 0 → 2 |
| `+` `-` `*` `/` `%` | binary arithmetic block | 2 → 1 |
| `sin` `cos` `tan` `sqrt` `exp` `ln` `tanh` `abs` | math builtins | 1 → 1 |
| `min` `max` | selection | 2 → 1 |

Arithmetic also appears in infix position: `_ * 0.5` and `_ + 1` build the same
blocks as `*` and `+` used as primitives.

Complex number literals use the suffix `i`: `3i`, `2.5i`. The parser also
recognises `1.0 + 2.0i` as syntactic sugar for `complex(1.0, 2.0)`.

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
main = + ~ _;             // integrator:        y[n] = x[n] + y[n-1]
main = + ~ (_ * 0.5);     // leaky integrator:  y[n] = x[n] + 0.5·y[n-1]
main = _ @ 1;             // one-sample delay
main = _ <: (_ , _) :> +; // fan out, then sum  = 2·x
main = abs(_);            // full-wave rectifier
```

## Type checking

`rill-lang` runs a Hindley-Milner inference pass before code generation:

- **Scalar types** — `int`, `float` (the runtime `T`), and type variables — are
  unified with an occurs check. Overloaded operators default to the runtime
  scalar when otherwise unconstrained, so arithmetic is monomorphized.
- **Arities** are synthesized bottom-up as concrete numbers and checked against
  the combinator table above.
- **Named functions** are let-generalized and instantiated per use site.
- **λ-parameters** are counted separately from signal ports. A function `f x = _ * x`
  has one λ-parameter (`x`) and one signal port (from `_`). Calling `f 0.5`
  consumes the λ-parameter, leaving the signal port open.

A type or arity mismatch is reported as an error carrying the offending source
span, and compilation stops there — an ill-typed diagram never reaches the
interpreter.

```rust,no_run
use rill_lang::compile;
// top-level parallel pair is (2 → 2): not a valid `main`
assert!(compile::<f32>("main = _ , _;").is_err());
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
main = _ : lowpass 1000.0 0.7;
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
main = + ~ moog 500.0 0.5;   // feedback-legal per-sample filter
```

Block built-ins cannot appear inside `~` — the compiler rejects them with an
error (`block built-in cannot be used inside a feedback loop`).

### Using built-ins from Rust

```rust,no_run
use rill_lang::compile_with;
use rill_adrift::lang_builtins::full_registry;

let reg = full_registry::<f32>();
let mut prog = compile_with::<f32>(
    "main = _ : onepole 200.0 0.7;",
    &reg,
    48_000.0,
).unwrap();
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
main = _ * param("gain", 0.5);
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
main = _ : lowpass param("cutoff", 1000.0, 20.0, 20000.0) 0.7;
```

### `smooth(x, ms)` — zipper-free smoothing

When a parameter changes abruptly at a block boundary, the step creates an
audible "zipper" click. `smooth(x, ms)` is a native one-pole low-pass (one per
call site) that slides its input value toward its output with the specified time
constant:

```faust
main = _ * smooth(param("gain", 0.5, 0.0, 1.0), 10.0);
```

Here `gain` is ramped with a 10 ms time constant — the output sample moves
smoothly even when the control thread snaps the parameter from 0 to 1.
`smooth` bakes the sample rate at compile time; if the sample rate changes,
the program must be recompiled for the time constant to match.

### Setting parameters from Rust

The `RillProgram` API exposes parameter slots by index:

```rust,no_run
use rill_lang::compile;

let mut prog = compile::<f32>("main = _ * param(\"gain\", 0.5);").unwrap();

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

## Scoping

| Binding form | Visibility | Mutual recursion |
|---|---|---|
| **Top-level defs** | All top-level definitions in the program | Yes |
| **`where` block** | Only within the function it's attached to | Yes, within the block |
| **`let` expression** | Only within the `in` body | Yes, within the block |

`let` bindings shadow outer names. `where` block names shadow top-level names.
Nested `let`/`where` blocks shadow outer blocks.

## Serialization

With the `serde` feature enabled, a program round-trips through
[`RillLangDef`], whose canonical form is simply the **source string** (a compiled
IR would rot across versions; source stays stable and editable):

```rust,no_run
use rill_lang::{RillLangDef, compile_def};

let def = RillLangDef::new("gain", "main = _ * 0.5;");
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
  "parameters": { "source": "main = _ * 0.5;" }
}
```

Setting the node's `source` parameter at runtime recompiles the program and
hot-swaps it — the seed of the runtime code-synthesis loop described in the
project's architecture notes. Note that compilation allocates, so a `source`
swap applied through the graph's `SetParameter` path runs inside the I/O
callback; treat it as a control-time operation to be performed when the graph is
not under hard real-time load, not as an every-block action.

## Execution model and performance

The compiler pipeline: **lex → parse → HM type inference → β-reduction →
lowering → scheduling**.

### β-reduction

After type inference, all user-defined function calls are eliminated by
substituting argument values directly into the function body. This happens at
compile time, producing a flat expression containing only Wire, constants,
built-ins, and combinators:

```faust
// Before reduction:
gain x = _ * x;
main = gain 0.5;

// After reduction (the IR seen by the back-end):
main = _ * 0.5
```

`let`-bound and `where`-bound definitions are also inlined. The reduction is
recursive: chained definitions (`h = g 0.5; g = f 0.25`) collapse to a
single expression.

### Scheduling

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
| `_ : lowpass 1000.0 0.7` (block Biquad) | ~275 ns |
| `_ : lowpass param("cutoff", 1000.0) 0.7` (dynamic) | ~306 ns |
| `_ : onepole 1200.0 0.5` (sample) | ~3.5 µs |
| `_ : moog 800.0 0.6` (sample) | ~4.0 µs |
| DSL-wrapped biquad vs. raw `Biquad` | ~264 ns vs. ~234 ns (~13% overhead) |

Wrapping a `rill-core-dsp` filter in the DSL costs about 13% over calling the raw
`Algorithm` — the price of the schedule dispatch and the register store. Driving
a filter parameter with `param(...)` adds only the per-block coefficient update.

## Status

The language is feature-complete for signal authoring: block-diagram
combinators, feedback and delay, Hindley-Milner types, Haskell-style definitions
with β-reduction, `let` and `where` binding groups with mutual visibility,
hybrid block/sample execution, a DSP/model built-in registry, and RT-safe named
parameters with smoothing.

Deferred to follow-on work:

- the **Cranelift `jit`** backend (the linear IR is the shared lowering target);
- **whole-graph-as-one-program** lowering (fusing a multi-node graph into one
  schedule);
- **signal-rate** (per-sample) modulation of imported built-in parameters
  (current parameter modulation is control-rate/per-block);
- composed expressions as built-in arguments and multi-output built-ins.

[`RillLangDef`]: https://docs.rs/rill-lang
