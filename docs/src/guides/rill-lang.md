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

Five properties define the language:

- **Block-arrow algebra.** A program is an **arrow** — a block transform over
  channels, `(I₁:Block<Scalar>…Iₙ) → (O₁:Block<Scalar>…Oₘ)` — executed by the
  backend on each hardware tick. Programs are compositions of signal processors
  via geometric combinators (`:` `,` `<:` `:>` `~` `@`), which are arrow laws,
  not imperative statements. There are no runtime variables — only signal blocks
  flowing through wires.
- **Haskell-style definitions.** Functions and constants use a unified syntax
  (`name args = body`). A **closed** top-level definition — no λ-parameters and
  zero input channels — is a single shared CAF instance, evaluated once; every
  other definition is macro-instantiated per reference site. All binding groups
  (`where`, `let`, top-level) have mutual visibility.
- **β-reduction.** User-defined function calls are fully inlined before lowering;
  references to closed CAFs are lifted once and shared instead. The final IR is
  flat — only Wire, constants, built-ins, and combinators remain.
- **Block-only execution.** The engine runs every instruction whole-buffer
  (SIMD-friendly); per-sample state (filters, integrators, delay lines) lives
  exclusively inside whole-block `BlockBuiltin` implementations.
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
Exactly one must be named `main` — the entry point. `main` may be any `n → m`
arrow; the example above is the common 1 → 1 case. `compile()` accepts general
arrows — its SISO `process()` drives the **first** output channel, while
multi-channel programs use the `MultichannelAlgorithm` trait (see
[Multi-IO and graph compilation](#multi-io-and-graph-compilation)).

### A program is an arrow

Every program is an **arrow** `(I₁:Block<Scalar>…Iₙ) → (O₁:Block<Scalar>…Oₘ)` — a
block transform over channels, in Hughes' sense of *Arrows*. It is *not* a
monad: parallel composition (`,`), for instance, cannot be expressed as monadic
bind. The type system has three levels:

| Level | Type | Meaning |
|---|---|---|
| sample | `Scalar` | one element inside a block (`int` / `float` / type variable) |
| channel | `Block<Scalar>` | one signal channel — a fixed-size buffer of samples (`BUF_SIZE`), mutated in place each tick |
| arrow | `ArrowTy` | a block transform `(I₁:Block…Iₙ) → (O₁:Block…Oₘ)` |

On each hardware tick the backend feeds n input blocks through the program and
receives m output blocks. Stateful DSP (oscillators, filters, delay lines)
lives *inside* the arrow and persists between ticks.

## Definitions and functions

rill-lang uses a unified syntax for constants and functions — both are
definitions of the form `name params = body`. The parameter count separates
constants (0 params) from functions (1+ params); whether a definition is
**shared** or **macro-instantiated** depends on its input channels — see
[Free variables (CAF)](#free-variables-caf):

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

rill-lang uses **bracket-free juxtaposition** as its canonical calling
convention — function name followed by space-separated arguments:

```faust
main = _ : lowpass 1000.0 0.7;
main = lowpass _ 1000.0 0.7;     // signal as first-class argument
main = sine 440.0 0.5 0.0;       // oscillator with freq, amp, phase
```

The parenthesized form `name(arg, ...)` is also supported but **juxtaposition
is canonical**. Each argument must be an atom (identifier, literal, `_`, `!`,
`(expr)`, `-expr`); for complex expressions use parentheses around the argument.

### Unified arguments

**Signals are first-class arguments** in the unified calling model. A built-in
doesn't require the signal on the left via `:`, you can pass it inline:

```faust
main = lowpass _ 1000.0 0.7;        // signal as first positional arg
main = mixer _1 _2 _3 { gain: 0.8 }; // variadic signal args
```

Some built-ins accept **variadic** signal inputs (e.g. `mixer` takes any number
of signals). Others specify a fixed signal arity per their signature. Scalar
parameters (floats, ints, records) follow signal args.

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

### Free variables (CAF)

A top-level definition with **no λ-parameters and zero input channels** is
**closed** and becomes a *Constant Applicative Form* (CAF) — Haskell's
shared-instance semantics. The compiler evaluates it **once** and every
reference — including references from inside user-defined functions — sees the
**same** instance:

```faust
osc  = sine 440.0 0.5 0.0;
main = osc, osc;          // both channels feed the same shared oscillator
```

Definitions with input channels (e.g. `gain = _ * 0.5`) are **open**: they keep
macro semantics and are re-instantiated at each reference site, exactly as
β-reduction dictates.

> **Behavior change.** A closed stateful top-level definition referenced two or
> more times used to compile into N independent copies; it is now **1 shared
> instance**:
>
> ```faust
> osc = sine 440 0.5 0;
> main = osc, osc;   // was 2 oscillators, now 1 shared
> ```
>
> To get independent instances, define distinct names or parametrize the
> definition with a λ-argument.

Three consequences of the CAF model:

- **Binding-level laziness.** A top-level definition that is never referenced
  produces no code (dead-code elimination by reference). `where`/`let` bindings
  keep macro semantics — they are re-instantiated per enclosing call, matching
  Haskell.
- **Global buffers.** A buffer resource declared at top level (e.g.
  `tape = TapeLoop 4096`) is closed, so functions capture it as a free variable;
  the existing `write_head`/`read_head` machinery resolves it at lowering. No
  new buffer syntax is needed.
- **Recursion.** A self-referential closed definition (`a = a`) is a compile
  error, not a stack overflow.

### `main` with parameters

`main` can declare input parameters — their names become slots in the compiled
`param_map`, addressable by name from the control thread:

```faust
main cutoff res = _ : lowpass cutoff res;
```

When compiled via `compile_graph()`, each `main` parameter and each function
parameter in the `where` block becomes a named parameter in the resulting
graph node. Use the `?name=default` syntax for late-binding actor parameters
(see [Actor Parameters](#actor-parameters) below).

### Records and config

Built-ins that accept structured configuration use **record literals**
`{ key: val }`:

```faust
main = mixer _1 _2 { channels: 2, buses: 0, master_vol: 0.8 };
main = dry_wet _ wet { mix: 0.5 };
main = eq_parametric _ { bands: [
    { freq: 500.0, q: 2.0, gain_db: -3.0, band_type: 0 },
    { freq: 2000.0, q: 1.0, gain_db: 1.5, band_type: 0 },
]};
```

Records can be nested — the EQ `bands` field contains a list of band
configurations (`{ freq, q, gain_db, band_type }`). Record keys must be
literals; values can be literals, `param()` references, or other records.

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
recognises `1.0 + 2.0i` as syntactic sugar for `complex 1.0 2.0`.

## Combinators

The block-diagram combinators are the **arrow laws** of the category of block
transforms: `:` is composition, `,` is the product, `<:` fan-out, `:>` fan-in
(sum), `~` the 1-block delayed loop, and `@` block-level delay. For
`A : (aᵢ, aₒ)` and `B : (bᵢ, bₒ)`:

| Form | Arrow law | Requirement | Resulting arity |
|---|---|---|---|
| `A : B` | composition (`Seq`) | `aₒ = bᵢ` | `(aᵢ, bₒ)` |
| `A , B` | product (`Par`) | — | `(aᵢ + bᵢ, aₒ + bₒ)` |
| `A <: B` | split / fan-out (`Split`) | `bᵢ` is a multiple of `aₒ` | `(aᵢ, bₒ)` |
| `A :> B` | merge / fan-in sum (`Merge`) | `aₒ` is a multiple of `bᵢ` | `(aᵢ, bₒ)` |
| `A ~ B` | 1-block delayed loop (`Loop`) | `bₒ ≤ aᵢ` | `(aᵢ − bₒ, aₒ)` |
| `A @ n` | block-level delay (`Delay`) | `A` is `_ → 1`, `n` a constant int | same as `A` |

Feedback (`~`) is the 1-block delayed **loop**: `B`'s outputs feed back into
`A`'s trailing inputs through one block (one tick) of delay — a unidirectional
feedback edge. `B` is evaluated independently and does not consume `A`'s output.
Stateful filters and recursive structures are whole-block built-ins (per-sample
state lives inside their `Algorithm` implementation). The delay operator `@` is
the block-level delay: it requires a compile-time constant integer length
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
main = integrator;             // running sum: y[n] = x[n] + y[n-1]
main = leaky_integrator 0.5;   // leaky integrator: y[n] = x[n] + 0.5·y[n-1]
main = + ~ _;                  // short form for `integrator`
main = + ~ (_ * 0.5);          // short form for `leaky_integrator 0.5`
main = _ @ 1;                  // one-sample delay
main = _ <: (_ , _) :> +;      // fan out, then sum  = 2·x
main = abs _;                  // full-wave rectifier
```

## Type checking

`rill-lang` runs a Hindley-Milner inference pass before code generation. Every
term is typed as an **arrow** — an `ArrowTy` over channels, each channel a
`Block<Scalar>`:

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
// a top-level parallel pair is a (2 → 2) arrow: compile() accepts general arrows
assert!(compile::<f32>("main = _ , _;").is_ok());
```

## Built-in functions

rill-lang programs can call stateful DSP/model built-ins from workspace crates
via an extensible FFI registry. Built-ins are **not** compiled into the
interpreter core — bindings live in the individual crates, aggregated by
`rill-adrift` (`lang_builtins::full_registry`), keeping `rill-lang` dependent
only on `rill-core`.

### Built-in registry

| Category | Builtins | Feature |
|---|---|---|
| Filters | `onepole`, `moog`, `lowpass`, `highpass`, `biquad` (block) | always |
| Integrators | `integrator`, `leaky_integrator` (block) | always |
| Oscillators | `sine`, `saw`, `square`, `triangle`, `noise` (block) | always |
| Effects | `delay`, `distortion`, `limiter` (block) | always |
| Mixer/EQ | `mixer`, `eq_parametric`, `dry_wet`, `graphic_eq` (block) | `router` |
| Analog | `analog_moog`, `cassettedeck` (block) | `analog` |
| Spectral | `spectralgate`, `spectraldelay`, `convolver` (block) | `fft` |
| Complex | `complex`, `conj`, `re`, `im`, `norm`, `arg`, `cmul`, `cadd` | always |
| Sampler | `sampler` (block) | `sampler` |
| Lofi | `lofi`, `ay38910` (block) | `lofi` |

### Calling convention

Built-ins use the **unified argument model**: signals are first-class positional
arguments, scalars follow, and configuration is passed as a record:

```faust
main = lowpass _ 1000.0 0.7;           // filter: signal, cutoff, resonance
main = sine 440.0 0.5 0.0;             // oscillator: freq, amp, phase (no signal in)
main = mixer _ ch2 ch3 { channels: 3 }; // variadic signal args + record
```

Parameters are **compile-time constants** (float or integer literals, optionally
with arithmetic) or a `param(...)` reference. Constants are folded to `f64`
during lowering. The signal port count per built-in is defined by its signature
(see individual crate registrations).

### Block built-ins

All built-ins are whole-buffer `BlockBuiltin`s: the built-in implements
`Algorithm<T>` and processes all samples of a block at once. Per-sample state
(filters, oscillators, integrators) lives inside the `Algorithm` implementation;
the engine itself is purely block-level, which keeps every step SIMD-friendly.

```faust
main = _ : moog 500.0 0.5;   // recursive filter (state inside the built-in)
```

Block built-ins cannot appear inside `~` — the compiler rejects them with an
error (`block built-in cannot be used inside a feedback loop`).

### Using built-ins from Rust

The umbrella registry (`rill_adrift::lang_builtins::full_registry`) aggregates
all workspace built-ins. For selective registration, individual crates expose
`register_lang_builtins()` functions:

```rust,no_run
use rill_lang::compile_with;
use rill_lang::builtin::Registry;

let mut reg = Registry::<f32>::new();
rill_core_dsp::lang::register::register_lang_builtins(&mut reg);
rill_lang::register::register_core_builtins(&mut reg);

let mut prog = compile_with::<f32>(
    "main = lowpass _ 1000.0 0.7;",
    &reg,
    48_000.0,
).unwrap();
let mut out = [0.0f32; 4];
prog.process(Some(&[1.0, 2.0, 4.0, 8.0]), &mut out).unwrap();
```

Or to compile directly into a graph engine with actor mailbox support:

```rust,no_run
use rill_lang::compile_graph;
use rill_adrift::lang_builtins::full_registry;

let reg = full_registry::<f32>();
let mut engine = compile_graph::<f32, BUF_SIZE>(
    "main = _ : lowpass ?cutoff=1000.0 ?resonance=0.7;",
    &reg,
    48_000.0,
).unwrap();
// engine.handle() returns ActorRef<CommandEnum> for sending SetParameter
```

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

## Actor parameters

rill-lang supports **late-binding actor parameters** with the `?name=default`
syntax — a concise alternative to `param()` designed for `compile_graph()`:

```faust
main = _ : lowpass ?cutoff=1000.0 ?resonance=0.7;
main = sine ?freq=440.0 0.5 0.0;
```

Each `?name=default` creates a named parameter slot. When compiled via
`compile_graph()`, parameters are addressable by the engine's `handle()`:

```rust,no_run
use rill_lang::compile_graph;
use rill_adrift::lang_builtins::full_registry;
use rill_core::queues::CommandEnum;
use rill_core::traits::ParamValue;

let reg = full_registry::<f32>();
let mut engine = compile_graph::<f32, BUF_SIZE>(
    "main = _ : lowpass ?cutoff=1000.0 ?resonance=0.7;",
    &reg,
    48_000.0,
).unwrap();
engine.handle().send(CommandEnum::SetParameter(
    rill_core::queues::SetParameter {
        anchor: "main".into(),
        parameter: "cutoff".into(),
        value: ParamValue::Float(2000.0),
        port: String::new(),
        source: SignalOrigin::Manual,
        timestamp: 0,
        sample_pos: None,
    }
)).ok();
```

### Where-block namespacing

Where-block definitions with parameters use **dot-notation namespacing**:

```faust
main = osc : filt where
    osc  = sine ?freq=440.0 0.5 0.0
    filt = _ : lowpass ?cutoff=1200.0 0.7
-- Parameters: "osc.freq", "filt.cutoff"
```

The `anchor` field in `SetParameter` is the definition name when inside a
`where` block (e.g. `"osc"` for `osc.freq`). Top-level `main` parameters
use `"main"` as their anchor.

### `?name` vs `param()`

| Feature | `?name=default` | `param("name", default)` |
|---|---|---|
| Syntax cost | 3 extra chars | 9+ extra chars |
| Intent | Late-binding for actor system | Inline parameter slot |
| Works with | `compile_graph()` | `compile()` / `compile_with()` |
| Use case | Graph nodes with external control | Standalone programs |

Both are RT-safe: one scalar per slot, read once per block, no locks.

## Multi-IO and graph compilation

rill-lang programs can be **multi-channel** — N inputs, M outputs. Multi-IO
programs implement `MultichannelAlgorithm<T>` when compiled with the `router`
feature:

```faust
main = mixer _1 _2 _3 { channels: 3, buses: 2 };  // 3→4 (2 master + 2 bus)
main = dry_wet _ wet_signals { mix: 0.5 };          // 2→2
```

### Graph compilation

`compile_graph(src, &registry, sample_rate)` compiles a rill-lang program into
a `CompiledGraphEngine<T, BUF_SIZE>` — a self-contained engine that:

`compile_graph()` and `rill-graph::GraphBuilder::build_ir()` share the same
compilation backend. Both produce a `GraphIr` (multi-node intermediate
representation), which `graph_compiler::compile()` transforms into a
`CompiledGraphEngine<T, BUF_SIZE>`. The difference is the source:
`compile_graph()` takes a single DSL source string; `build_ir()` takes a
programmatically-built topology with typed parameter signatures.

- Runs a flat vector of `NodeClosure`s over a pool of pre-allocated `FixedBuffer`s
- Drains the actor mailbox for `SetParameter` commands each tick
- Dispatches to `MultichannelAlgorithm::process()` for multi-IO programs
- Uses SISO fast path for 0/1 input → 1 output programs

```rust,no_run
use rill_lang::compile_graph;
use rill_core::traits::Algorithm;

let reg = rill_lang::builtin::Registry::<f32>::new();
let mut engine = compile_graph::<f32, BUF_SIZE>(
    "main = _ * 0.5;",
    &reg,
    48_000.0,
).unwrap();
let mut out = [0.0f32; 4];
engine.process(Some(&[1.0, 2.0, 4.0, 8.0]), &mut out).unwrap();
assert_eq!(out, [0.5, 1.0, 2.0, 4.0]);
```

`CompiledGraphEngine` supports bridge/feedback configurations via `GraphNode`
annotations (`is_bridge`, `feedback_read`, `feedback_write`), used for tape delay
and send/return topologies where left-side outputs feed right-side inputs.

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

Two paths to runtime:

### `compile_graph()` — direct engine

`compile_graph()` compiles source directly into a `CompiledGraphEngine<T>` with
actor mailbox support:

```rust,no_run
use rill_lang::compile_graph;
use rill_adrift::lang_builtins::full_registry;

let reg = full_registry::<f32>();
let mut engine = compile_graph::<f32, BUF_SIZE>(
    "main = _ * 0.5;",
    &reg,
    48_000.0,
).unwrap();
```

The engine provides `handle()` → `ActorRef<CommandEnum>` for sending
`SetParameter` commands, and implements `Algorithm<T>` directly.

### `rill/lang` factory node

The umbrella crate `rill-adrift` registers a `rill/lang` node type. A
serialized graph can embed a rill-lang block by giving it a `source` parameter:

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
built-ins, and combinators. References to **closed CAFs** are the one exception —
they are not inlined here, but lifted once at lowering and shared by every
reference site (see [Free variables (CAF)](#free-variables-caf)):

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

On each hardware tick the arrow runs: the backend feeds n input blocks in, the
schedule executes, m output blocks come out. The interpreter compiles the linear
IR into an **execution schedule** via SCC (strongly-connected component) analysis
of the data-dependency graph. Every step is a whole-buffer operation:

- **Block steps** — all instructions (arithmetic, math builtins, fan-out/fan-in,
  block-state read/write, delay read/write) run **whole-buffer** through the
  `rill_core::math::vector` SIMD eDSL (`ScalarVector4`). The block path computes
  directly in `T` (the runtime scalar, e.g. `f32`), letting LLVM auto-vectorize
  the hot loop.
- **Foreign-block steps** — `BlockBuiltin` calls are opaque whole-buffer
  `Algorithm::process` invocations. A built-in's internal per-sample recurrence
  (e.g. a filter's state) is invisible to the engine.

The whole-buffer register store is a `Vec<FixedBuffer<T, BUF>>` — fixed buffers
of the program's `BUF_SIZE`, pre-allocated at construction and mutated in place,
reused across calls. The hot `process()` path performs no heap allocation, no
locks, and no syscalls, honoring rill's real-time rules.

Feedback (`~`) and delay (`@`) are block-level: feedback uses a double-buffered
block state with a one-tick shadow copy (swapped at tick end), and delay uses a
block-level ring buffer. No instruction runs sample-by-sample, so the whole
engine is SIMD-friendly. A Cranelift JIT backend is still planned and will reuse
the same IR.

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

| Program | Time |
|---|---|
| `_ * 0.5` | ~63 ns |
| `_ * 0.5 : abs : (_ * 2.0)` | ~111 ns |
| `_ <: (_ , _ * 0.5) :> +` | ~88 ns |
| `_ * param("g", 0.5)` | ~61 ns |
| `_ @ 4` | ~1.6 µs |
| `+ ~ (_ * 0.5)` | ~3.4 µs |
| `_ * smooth(param("g", 0.5), 10.0)` | ~4.5 µs |

### Built-ins (via `rill-adrift`)

| Program | Time |
|---|---|
| `_ : lowpass 1000.0 0.7` (block Biquad) | ~275 ns |
| `_ : lowpass param("cutoff", 1000.0) 0.7` (dynamic) | ~306 ns |
| `_ : onepole 1200.0 0.5` (block) | ~3.5 µs |
| `_ : moog 800.0 0.6` (block) | ~4.0 µs |
| DSL-wrapped biquad vs. raw `Biquad` | ~264 ns vs. ~234 ns (~13% overhead) |

Wrapping a `rill-core-dsp` filter in the DSL costs about 13% over calling the raw
`Algorithm` — the price of the schedule dispatch and the register store. Driving
a filter parameter with `param(...)` adds only the per-block coefficient update.

## Value channels and first-class data

Alongside block-rate **signal** channels, a program can carry **value**
channels: one arena value per tick. Value channels are typed (`ValueTy`),
flow through the same combinators, and are processed in a per-tick value-track
phase of the interpreter (the signal track stays whole-buffer SIMD).

### Value types

| Construct | Meaning |
|---|---|
| `data Point = { x: Float, y: Float }` | record (product) type; construct `Point { x: 1.0, y: 2.0 }`, project `p.x` |
| `data Shape = Circle Float \| Rect Float Float` | sum type with constructors; match with `match s of { Circle r => ...; Rect w h => ...; }` |
| `type Angles = Float` | type synonym (pure substitution) |
| `newtype Hz = Float` | distinct wrapper; construct `Hz 440.0` (no auto-unwrap in v1) |
| `typeclass Show a where { show: a; }` | ad-hoc polymorphism; `instance Show Float where { show f = ...; }` |
| `f = double; main = f 21.0` | named function references (β-inlined at compile time) |

Value expressions: record/sum/newtype constructors, field projection `p.x`,
COW field update `p.x := 3.0`, `match` pattern matching, and method calls.
A `data` value output is inspected via `RillProgram::value_outputs()`.

### Memory model: arena + RC + COW

Value data lives in a **fixed-capacity arena** owned by the `RillProgram`,
pre-allocated at build time (no heap growth on the RT path). Slots are managed
by non-atomic reference counting (single-threaded DAG) with **copy-on-write**:
mutating a field of a shared value copies it first. Local variables (including
`main`'s λ-parameters) are **runtime-stack cells** — persistent arena slots that
`SetParameter` writes into directly.

Acyclicity is guaranteed at compile time: a `data`/`newtype` type that
(transitively) references itself is rejected. The arena capacity bound is
computed from the value instructions and the static subtree sizes of value
outputs, so a well-formed program never exhausts the arena.

Deferred: closures (only named function references), runtime typeclass
dispatch (methods resolve at compile time), value-state persistence beyond
per-tick scratch, and `strict`/`complete` compiler modes (the acyclicity and
capacity checks above are the foundation of the `strict` contract).

## Status

The language is feature-complete for signal authoring: a block-arrow model
(`Scalar` / `Block` / `ArrowTy`) with block-diagram combinators as arrow laws,
feedback and delay, Hindley-Milner types, Haskell-style definitions with
β-reduction, CAF free variables (shared closed instances) with binding-level
laziness, `let` and `where` binding groups with mutual visibility, block-only
execution, a 27-built-in registry (DSP, effects, oscillators,
mixer/EQ, analog, spectral, complex, lofi), RT-safe named parameters (`param()`
and `?name`), records for built-in configuration, multi-IO via
`MultichannelAlgorithm`, graph compilation (`compile_graph()` →
`CompiledGraphEngine`), and first-class data (`data`/`type`/`newtype`/
`typeclass`, value channels, arena+RC+COW memory, runtime-stack cells).

Deferred to follow-on work:

- the **Cranelift `jit`** backend (the linear IR is the shared lowering target);
- **whole-graph-as-one-program** lowering (fusing a multi-node graph into one
  schedule);
- **signal-rate** (per-sample) modulation of imported built-in parameters
  (current parameter modulation is control-rate/per-block);
- composed expressions as built-in arguments;
- closures, runtime typeclass dispatch, cross-node value ports, and the
  `strict`/`complete` compiler-mode contract.

[`RillLangDef`]: https://docs.rs/rill-lang
