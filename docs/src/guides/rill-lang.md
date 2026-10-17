# rill-lang: the Signal DSL

`rill-lang` is a small, Faust-style functional streaming language. You write
source text; `rill-lang` compiles it — lexer, parser, Hindley-Milner type
checker, linear IR, β-reduction, and an execution backend — into a runnable
program. There are two execution paths:

- **IO programs** — `main : (String…) -> IO ()`, executed to completion by
  [`ProgramRunner`](#io-programs-streams-and-the-runner).
- **Pure-value programs** — one value-track tick, run by
  `RillProgram::run_value()` and read from `value_outputs()`.

> **Changed in the IO/streams core branch.** `RillProgram` no longer implements
> `Algorithm<T>`/`MultichannelAlgorithm<T>`: the signal-transform executor
> (`process()`, `ProgramEngine`, `readInput`/`writeOutput`, buffer-λ `main`) is
> removed, and so is the old `compile_graph()` graph-execution entry point.
> Programs now express their I/O as streams *inside* the program. The block-arrow
> algebra below is still the language's signal-block layer, but this branch has
> no host that runs a whole program as a transform. Graph compilation is
> **deferred**: what remains sits behind the off-by-default `graph` feature
> (`rill_lang::graph`), does not build at this revision, and is not built by the
> workspace gates.

The current backend is a safe, allocation-free **interpreter**. A Cranelift JIT
backend is planned behind a future `jit` feature; it will consume the same
intermediate representation, so nothing in the language front-end changes when it
arrives.

> This page is the canonical language reference. For the broader idea of
> embedding domain-specific languages in rill, see the [eDSL guide](dsl.md).

## Overview

rill-lang exists so that a program can be **authored — and, in time,
machine-synthesised — at runtime** rather than hand-written in Rust and compiled
ahead of time. A program is compiled on the fly (no `rustc`, no external
toolchain) to a runnable `RillProgram` — an `IO ()` program for the executor, or
a value tick for the retained pure-value path. The compiler is tiny and
self-contained, and the compiled program obeys rill's rules: no heap allocation
after warm-up, no locks, and no syscalls on the processing path.

Five properties define the language:

- **Block-arrow algebra.** A signal-block program is an **arrow** — a block
  transform over channels, `(I₁:Block<Scalar>…Iₙ) → (O₁:Block<Scalar>…Oₘ)` —
  built from geometric combinators (`:` `,` `<:` `:>` `~` `@`), which are arrow
  laws, not imperative statements. There are no runtime variables — only signal
  blocks flowing through wires. `IO` programs add streams and effect sequencing
  on top of the same core.
- **Haskell-style definitions.** Functions and constants use a unified syntax
  (`name args = body`). A **closed** top-level definition — no λ-parameters and
  zero input channels — is a single shared CAF instance, evaluated once; every
  other definition is macro-instantiated per reference site. All binding groups
  (`where`, `let`, top-level) have mutual visibility.
- **β-reduction.** User-defined function calls are fully inlined before lowering;
  references to closed CAFs are lifted once and shared instead. The final IR is
  flat — only Wire, constants, built-ins, and combinators remain.
- **Block-only execution.** The signal-block backend runs every instruction
  whole-buffer (SIMD-friendly); per-sample state (filters, integrators, delay
  lines) lives exclusively inside whole-block `BlockBuiltin` implementations.
- **RT-safe control.** Named parameters and smoothing give control-rate
  automation without recompilation or locks.

## A first program

The canonical form is an `IO` program run to completion by the
`ProgramRunner`:

```rust,no_run
use rill_lang::{compile_runner, run_on_stack, Registry};

let stdout = run_on_stack(16 << 20, || {
    let mut runner = compile_runner::<f32, 256>(
        r#"main = do { write (Just "hi") stdout; return () };"#,
        &Registry::new(),
        44_100.0,
    )
    .unwrap();
    runner.run().unwrap();
    runner.stdout().to_vec()
});
assert_eq!(stdout, ["hi"]);
```

A pure-value program uses `compile()` and runs one value tick:

```rust,no_run
use rill_lang::{arena::Value, compile};

let mut prog = compile::<f32>("main = 1.0 + 2.0;").unwrap();
prog.run_value().unwrap();
assert_eq!(
    prog.arena().get(prog.value_outputs()[0].unwrap()).unwrap(),
    &Value::Float(3.0),
);
```

A program is a list of mutually-recursive definitions, each terminated by `;`.
Exactly one must be named `main` — the entry point.

> **Signal blocks are not executed in this branch.** A `main` such as
> `_ * 0.5` still parses, type-checks, and lowers (the block-arrow algebra below
> is intact), but `RillProgram` no longer implements `Algorithm`/
> `MultichannelAlgorithm` and there is no `process()`: a signal-transform host is
> follow-on work. See [Execution model and performance](#execution-model-and-performance).

### A program is an arrow

A **signal-block** program is an **arrow** `(I₁:Block<Scalar>…Iₙ) → (O₁:Block<Scalar>…Oₘ)` — a
block transform over channels, in Hughes' sense of *Arrows*. It is *not* a
monad: parallel composition (`,`), for instance, cannot be expressed as monadic
bind. The type system has three levels:

| Level | Type | Meaning |
|---|---|---|
| sample | `Scalar` | one element inside a block (`int` / `float` / type variable) |
| channel | `Block<Scalar>` | one signal channel — a fixed-size buffer of samples (`BUF_SIZE`), mutated in place each tick |
| arrow | `ArrowTy` | a block transform `(I₁:Block…Iₙ) → (O₁:Block…Oₘ)` |

A signal-transform host feeds n input blocks through the program and receives m
output blocks per tick. Stateful DSP (oscillators, filters, delay lines)
lives *inside* the arrow and persists between ticks.

## IO programs, streams, and the runner

The executor is `ProgramRunner<T, BUF>`: it runs a program's
`main : (String…) -> IO ()` to completion against a realized `World`. It takes
**no buffers or backend handles from outside** — the program's I/O is the
streams it creates.

```rust,no_run
use rill_lang::{compile_runner, Registry};

let mut runner = compile_runner::<f32, 256>(
    r#"main = do { write (Just "hi") stdout; return () };"#,
    &Registry::new(),
    44_100.0,
)
.unwrap();

runner.set_args(&["launch".to_string()]);   // bind main's String λ-params
runner.set_stdin(&["input line".to_string()]); // injected `read stdin` lines
runner.run().unwrap();

assert_eq!(runner.stdout(), ["hi"]);
assert_eq!(runner.stderr(), Vec::<String>::new());
```

- **`set_args(&[String])`** binds `main`'s String λ-parameters positionally;
  missing arguments default to the empty string. They are *not* control
  parameters.
- **`set_stdin(&[String])`** injects the line-oriented input `read stdin` yields
  (rill has no `ByteString`), rewinding the stream for the next run.
- **`run()`** executes `main`'s `IO` action to completion. The captured console
  is refreshed even on failure, so a caller can inspect what was written before
  the error.
- **`stdout()` / `stderr()`** return the captured lines from **all runs since
  construction** — the console is captured, not echoed, and cumulative.
- **`program()` / `program_mut()`** expose the compiled `RillProgram` for
  parameter control and inspection.

`compile_runner` compiles in **Complete** (lazy) mode unless the source carries
its own `{-# MODE … #-}` pragma: the runner executes a Turing-complete program,
including guarded recursive stream helpers that Strict rejects.

### Streams

Streams are ordinary values — io-streams' `InputStream`/`OutputStream`,
generic over the element type. `Nothing` means EOF on a read and close on a
write.

```
fromList        :: List a -> IO (InputStream a)
toList          :: InputStream a -> IO (List a)
read            :: InputStream a -> IO (Maybe a)
write           :: Maybe a -> OutputStream a -> IO ()
atEnd           :: InputStream a -> IO Bool
unread          :: a -> InputStream a -> IO ()
connect         :: InputStream a -> OutputStream a -> IO ()
makeInputStream :: IO (Maybe a) -> IO (InputStream a)
makeOutputStream:: (Maybe a -> IO ()) -> IO (OutputStream a)

stdout, stderr  :: OutputStream String
stdin           :: InputStream String
```

A complete program — build a memory stream, drain it, render the elements:

```faust
show_all acc x = concat_string acc (show_float x);

main = do {
    xs <- fromList [1.0, 2.0, 3.0];
    ys <- toList xs;
    write (Just (fold show_all "" ys)) stdout;
    write (Just (show_int (length ys))) stderr;
    return ()
};
```

- **Memory streams** — `fromList`/`toList` round-trip, `unread` pushes an
  element back, `connect` pipes an input into an output, and
  `makeInputStream`/`makeOutputStream` wrap program closures as sources/sinks.
- **`toList` drains to EOF** — any input stream, not only memory ones: the
  pushed-back prefix first, then the source action, re-run per element until it
  reports `Nothing`.
- **Console ownership** — `stdin`/`stdout`/`stderr` are runtime-owned:
  `write Nothing` is inert on them. It closes **program-owned** sinks created by
  `makeOutputStream`.
- **Rendering** — the IO surface has no `Show` typeclass: use `show_float`,
  `show_int`, `show_bool`, and `concat_string`.

### ST and IO values

`runST` opens a fresh, isolated state region (`ST s a`; the rank-2 `forall s`
keeps state from escaping), and `newSTRef`/`readSTRef`/`writeSTRef`/
`modifySTRef` operate inside it. `IO a` is the `World`-threading monad the runner
executes; the linear `World` token orders effects by data dependency, so
reordering them is inexpressible.

### Stack depth (documented limit)

The lazy `IO` interpreter nests one `bind` per `do` statement, so running `main`
to completion recurses once per statement — roughly **300 KiB of native stack per
statement in a debug build**. A multi-statement program therefore overflows a
default 2 MiB thread stack (an abort, not a clean error). Run such programs on a
grown-stack thread with `run_on_stack`:

```rust,no_run
use rill_lang::{compile_runner, run_on_stack, Registry};

let stdout = run_on_stack(16 << 20, || {
    let mut runner = compile_runner::<f32, 256>(
        r#"main = do { write (Just "hi") stdout; return () };"#,
        &Registry::new(),
        44_100.0,
    )
    .unwrap();
    runner.run().unwrap();
    runner.stdout().to_vec()
});
assert_eq!(stdout, ["hi"]);
```

`ProgramRunner` is not `Send` (it owns `Rc<RefCell<…>>` state), so the
compile-then-run sequence must happen **inside** the closure. A panic in the
closure is resumed on the calling thread. Trampolining the `bind` chain to remove
the limit is a planned follow-up.

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
  `tape = tape_loop 4096`) is closed, so functions capture it as a free variable;
  the `write_head`/`read_head` builtins resolve it at lowering. The tape is a
  `Buffer` family member created by the `tape_loop` constructor.
- **Recursion.** A self-referential closed definition (`a = a`) is a compile
  error, not a stack overflow.

### `main` with parameters

`main` can declare input parameters — their names become slots in the compiled
`param_map`, addressable by name from the control thread:

```faust
main cutoff res = _ : lowpass cutoff res;
```

Each `main` parameter and each function parameter in a `where` block becomes a
named slot in the compiled program, readable via `params_meta()` /
`param_index()` and writable via `set_param()`. Use the `?name=default` syntax
for late-binding parameter slots (see [Actor parameters](#actor-parameters)
below). The graph-side wiring that exposed these slots on a `rill/lang` node is
not part of this branch.

### Records and config

Built-ins that accept structured configuration use **record literals**
`{ key: val }`:

```faust
main = mixer _1 _2 { buses: 0, master_vol: 0.8 };
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

## Evaluation modes: `strict` / `complete`

A program selects its evaluation contract with a module pragma or an API key.

```rill
{-# MODE complete #-}
from n = cons n (from (n + 1.0));
```

- **`strict`** (default) — the current behavior: eager value track, recursion is
  rejected at compile time, and only finite data exists. Existing programs are
  unaffected.
- **`complete`** — the lazy value track: call-by-need evaluation, Haskell-style
  recursion (all top-level definitions are recursive; mutual recursion and
  `let`/`where` bindings included), and infinite structures.

The mode is selected by a `{-# MODE strict|complete #-}` pragma at the top of
the source (the file's pragma wins over the API key), or via the compile API:

```rust,no_run
use rill_lang::compile_with_mode;
use rill_lang::mode::Mode;
// complete mode: recursion and laziness allowed
let prog = compile_with_mode::<f32>(
    "{-# MODE complete #-}\ncountup n = if n == 3.0 then 3.0 else countup (n + 1.0);\nmain = countup 0.0;",
    Mode::Complete,
);
```

### Laziness and bottom

In `complete` mode the value track is **call-by-need**: function application is
suspended as a lazy thunk, and strict points (arithmetic, comparison, pattern
matching, output) force operands to WHNF. This makes infinite sequences
productive:

```rill
{-# MODE complete #-}
from n = cons n (from (n + 1.0));
headv xs = match (head xs) of { Nothing => 0.0; Just x => x };
main = headv (tail (tail (tail (from 10.0))));
```

`from n` is an infinite lazy list; `head`/`tail` force elements on demand.
(`main` evaluates to `13.0`.)

A computation that is not productive — a self-referential thunk such as
`a = a` — evaluates to the value **bottom** (`⊥`) rather than hanging. Bottom
propagates through strict operations and is observable through the value
outputs.

Recursion in `complete` mode is **not** checked for termination (Turing
completeness): a non-productive recursion surfaces as bottom, never as a hang.

### Self-referential structures (`fibs`)

A self-referential local definition (no λ-parameters) is a **thunk in a shared
cell** in `complete` mode — recursion resolves through the cell, not inline, so
a value can reference itself lazily:

```rill
{-# MODE complete #-}
zipWith f xs ys = cons (f (headv xs) (headv ys)) (zipWith f (tail xs) (tail ys));
headv xs = match (head xs) of { Nothing => 0.0; Just x => x };
fibs = cons 0.0 (cons 1.0 (zipWith (fn a b -> a + b) fibs (tail fibs)));
main = headv (tail (tail (tail (tail fibs))));
```

`fibs` is the infinite Fibonacci sequence: `tail fibs` suspends as a lazy
thunk (application arguments are lazy, not eager), so the self-reference is
productive. (`main` evaluates to `3.0`, the fifth Fibonacci number.)

`-` works both as infix subtraction (`n - 1.0`) and as a negative literal
(`-2.0`, a unary minus applied to the literal): the parser distinguishes them
by adjacency — a `-` directly attached to a number is a negative literal, a
separated `-` is the infix operator.

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
| Mixer/EQ | `mixer`, `eq_parametric`, `dry_wet`, `graphic_eq`, `mono_to_stereo` (block) | `router` |
| Analog | `analog_moog` (block) | `model` |
| Spectral | `spectralgate`, `spectraldelay`, `convolver` (block) | `fft` |
| Complex | `complex`, `conj`, `re`, `im`, `norm`, `arg`, `cmul`, `cadd` | always |
| Sampler | `sampler` (block) | `sampler` |
| Lofi | `lofi`, `ay38910` (block) | `lofi` |
| Tape | `tape_loop`, `write_head`, `read_head` | `sampler` |

### Calling convention

Built-ins use the **unified argument model**: signals are first-class positional
arguments, scalars follow, and configuration is passed as a record:

```faust
main = lowpass _ 1000.0 0.7;           // filter: signal, cutoff, resonance
main = sine 440.0 0.5 0.0;             // oscillator: freq, amp, phase (no signal in)
main = mixer _ ch2 ch3 { buses: 0, master_vol: 1.0 }; // variadic signal args + record
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

The builtin **catalog** lives in the language (`foreign fn` declarations
auto-registered from `rill_lang::types::ty::BUILTIN_FOREIGN_DECLS`). Rust
implementations are registered as **factories** on the
`rill_lang::ffi::ForeignRegistry` via each crate's `register_foreign_*`:

```rust,no_run
use rill_lang::ffi::ForeignRegistry;
use rill_lang::compile_with_ffi;

let mut ffi = ForeignRegistry::<f32>::new();
rill_lang::register::register_foreign_generators(&mut ffi);
rill_lang::register::register_foreign_filters(&mut ffi);
rill_router::register::register_foreign_router(&mut ffi);

let mut prog = compile_with_ffi::<f32>(
    "main = fromList [0.5, 1.0];",
    &ffi,
    48_000.0,
).unwrap();
prog.run_value().unwrap();
```

Built-ins are resolved against the factory registry at compile time; the
program then runs on the pure-value path (or inside an `IO` program compiled
with the same registry via `compile_runner`).

`rill_adrift::lang_builtins::full_registry` builds a factory-only
`rill_lang::builtin::Registry<T>` for the common case: signatures come from the
language catalog, implementations from the FFI factories.

A whole program is not compiled into a graph engine in this branch: there is no
`compile_graph()` and no mailbox-owning `CompiledGraphEngine`. Graph compilation
is **deferred** — it is not buildable at this revision, so it is not part of the
workspace gates.

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

### Parameters on a graph node

The `rill/lang` factory node and its `Node::set_parameter(name, value)` path are
**not available in this branch**: graph-node wiring was part of the removed
graph-execution engine. Parameter slots are still named (the same string you
wrote in the DSL) and are controlled through `RillProgram::param_index()` /
`set_param()`; a host that wants name-addressed control can read the names from
`params_meta()`.

### Control-rate semantics

Parameters and `smooth` are control-rate constructs. The compiled program
stores one scalar per parameter slot; each tick reads the current value once
and re-uses it for the entire block. The control thread (`set_param` /
`set_parameter`) writes a new value, and the read is observed on the next
`run_value()` call — i.e. at the next block boundary. This model is efficient
(the hot path is a simple load + multiply / load + onepole) and safe (no locks,
no atomics).

For more on the automation plumbing, see the [Automaton guide](world-of-automatons.md).

## Actor parameters

rill-lang supports **late-binding parameter slots** with the `?name=default`
syntax — a concise alternative to `param()`:

```faust
main = _ : lowpass ?cutoff=1000.0 ?resonance=0.7;
main = sine ?freq=440.0 0.5 0.0;
```

Each `?name=default` creates a named slot in the compiled program. Slots are
enumerated by `RillProgram::params_meta()` and written by index with
`set_param()`. The actor-mailbox path — a program compiled into a graph engine
whose `handle()` receives `CommandEnum::SetParameter` by name — is **not part of
this branch**; it belonged to the removed graph-execution engine. A host that
wants name-addressed control reads the names from `params_meta()` and sends the
values itself.

### Where-block namespacing

Where-block definitions with parameters use **dot-notation namespacing**:

```faust
main = osc : filt where
    osc  = sine ?freq=440.0 0.5 0.0
    filt = _ : lowpass ?cutoff=1200.0 0.7
-- Parameters: "osc.freq", "filt.cutoff"
```

The name of the enclosing definition is the slot's anchor when inside a `where`
block (e.g. `"osc"` for `osc.freq`); top-level `main` parameters use `"main"`
as their anchor. Full slot names are reported by `params_meta()`.

### `?name` vs `param()`

| Feature | `?name=default` | `param("name", default)` |
|---|---|---|
| Syntax cost | 3 extra chars | 9+ extra chars |
| Intent | Late-binding named slot | Inline parameter slot |
| Works with | `compile_runner()`, `compile_with()` | `compile()`, `compile_with()` |
| Use case | Programs driven from outside | Standalone programs |

Both are RT-safe: one scalar per slot, read once per block, no locks.

## Multi-IO and graph compilation

rill-lang programs can be **multi-channel** — N inputs, M outputs:

```faust
main = mixer _1 _2 _3 { buses: 2, master_vol: 1.0 };  // 3→2 (bus + master)
main = dry_wet _ wet_signals { mix: 0.5 };          // 2→2
```

These lower to multi-channel `MultichannelBlockBuiltin` calls. The executor that
ran a whole program as an N→M transform (`MultichannelAlgorithm` dispatch,
`CompiledGraphEngine`, actor mailbox draining) is removed in this branch, so
multi-IO signal programs are not executable here; the multichannel *built-ins*
themselves remain registered and callable from a future transform host.

### Graph compilation

Graph compilation is **deferred** with the graph follow-on. What still exists in
the source sits behind the off-by-default `graph` feature:

- `rill_lang::graph::GraphBuilder` accumulates nodes and edges into a
  `GraphSpec`; `rill_lang::graph::compile` (intended to turn a spec into a
  `CompiledStream`) does not compile at this revision.
- `rill_lang::graph::compile_spec` reconstructs a spec into rill-lang AST — also
  stale against the current AST.
- A graph with passive tape backends partitions into a recording and a playback
  subgraph joined by a plain-data `TapeSpec`.

`cargo check -p rill-lang --features graph` fails (the module targets the
removed `Par`/`Split`/`Cut` AST), so it is **not built by the workspace gates**;
`rill-analyzer`, its only in-tree consumer, is excluded from the workspace
members for the same reason. Do not depend on any of it from a default build.

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

## Using it in an application

A host application has two paths to runtime:

### The program runner

`rill_lang::compile_runner` gives a `ProgramRunner` you drive directly — bind
arguments, inject stdin, run, and read the captured console (see
[IO programs, streams, and the runner](#io-programs-streams-and-the-runner)).
This is the recommended embedding for a program that owns its I/O.

### The pure-value tick

`rill_lang::compile` / `compile_with` / `compile_with_ffi` give a `RillProgram`
whose `main` is evaluated by `run_value()`, one value tick at a time. This is the
embedding for pure computation (collections, records, closures, typeclasses) with
no I/O.

The `rill/lang` graph factory node and the whole-program graph engine it drove
are **not available in this branch**: `rill-adrift`'s `lang` feature exposes the
language and its built-in registry, not a graph node that owns a
`CompiledGraphEngine`. Embedding a program in a signal graph again is follow-on
work (see the deferred audio plan).

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

The signal-block backend compiles the linear IR into an **execution schedule**
via SCC (strongly-connected component) analysis of the data-dependency graph,
and a signal-transform host feeds n input blocks in and takes m output blocks
out per tick. Every step is a whole-buffer operation:

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
reused across calls. The hot tick path performs no heap allocation, no locks, and
no syscalls, honoring rill's real-time rules.

Feedback (`~`) and delay (`@`) are block-level: feedback uses a double-buffered
block state with a one-tick shadow copy (swapped at tick end), and delay uses a
block-level ring buffer. No instruction runs sample-by-sample, so the whole
engine is SIMD-friendly. A Cranelift JIT backend is still planned and will reuse
the same IR.

### The IO executor

An `IO` program is not scheduled: the interpreter evaluates `main`'s action
chain to completion, one `bind` at a time, threading the `World` token. Effects
are therefore ordered by data dependency (exactly like GHC's `State# RealWorld`),
and the program's cost is its own recursion depth — see
[Stack depth](#stack-depth-documented-limit).

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
| `Bool`, `String` | scalar value types (value track only); literals `true`/`false`, `"text"` |
| `List a`, `Map k v`, `Set a` | builtin containers (**open** — grow up to the pre-allocated arena pool) |
| `Maybe a`, `Pair a b`, `Either a b` | builtin data types: `Maybe a = Just a \| Nothing`, `Pair a b = { first, second }`, `Either a b = Left a \| Right b` |
| `typeclass Show a where { show: a; }` | ad-hoc polymorphism; `instance Show Float where { show f = ...; }` |
| `fn x -> x * 2.0` | first-class function (lambda literal); see below |
| `f = double; main = f 21.0` | named function references (function values) |

Value expressions: record/sum/newtype constructors, field projection `p.x`,
COW field update `p.x := 3.0`, `match` pattern matching, and method calls.
A `data` value output is inspected via `RillProgram::value_outputs()`.

#### Builtin type constructors and kinds

The builtin containers and data types are **type constructors**: they take
type arguments, applied by **juxtaposition** (consistent with the DSL) — e.g.
`List Float` is `List` applied to `Float`. Collections are **open** (no
capacity argument). `Bool`/`String` are leaf value types (kind `*`).

| Constructor | Kind | Meaning |
|---|---|---|
| `List a` | `* → *` | ordered sequence of `a` (open) |
| `Set a` | `* → *` | unordered set of `a` (open) |
| `Map k v` | `* → * → *` | key→value map (open) |
| `Maybe a` | `* → *` | optional `a` |
| `Pair a b` | `* → * → *` | pair of `a` and `b` |
| `Either a b` | `* → * → *` | sum of `a` or `b` |

`List`/`Map`/`Set` are opaque arena containers (see below); `Maybe`/`Pair`/
`Either` are builtin `data` declarations reusing the existing record/sum/match
machinery — they are injected into the type environment at compile time.

### First-class collections

Lists, maps, and sets are first-class arena values with Haskell-style
operations. Collections are **open**: there is no capacity in the type, and
`cons`/`insert` grow freely. Values live in a **page-based arena**
(Alexandrescu-style): a pre-allocated slot pool (embedded free list) plus a
size-classed **payload buffer pool**, so collection ops perform **no heap
allocation** on the processing path in the default (RT) mode. The pool is
bounded by a conservative build-time budget; exceeding it in the default mode
is a detectable no-op (a build-budget bug), and the `growable-arena` feature
(non-RT) grows the pool instead. `map`, `filter`, and `tail` preserve the
shape; `length` needs none.

```faust
xs  = [1.0, 2.0, 3.0];          // List Float
e   = list;                     // empty list
ys  = cons 10.0 e;              // prepends; the list grows
h   = head xs;                  // Maybe Float: Just 1.0 / Nothing
t   = tail xs;                  // List Float
n   = length xs;                // Int
z   = map (fn x -> x * 2.0) xs;           // List Float — function first
s   = fold (fn a b -> a + b) 0.0 xs;      // Float
f   = filter (fn x -> x > 1.0) xs;        // List Float

m  = { "a": 1.0, "b": 2.0 };   // Map String Float
m1 = insert "a" 9.0 m;         // replace-on-duplicate
m2 = empty_map;                // empty Map k v (`map` is the HOF builtin)
v  = lookup "a" m;             // Maybe Float
b  = member "a" m;             // Bool

st = empty_set;                // empty Set a
s1 = insert 1 st;              // Set Int
b2 = member 1 s1;              // Bool
```

| Operation | Signature | Meaning |
|---|---|---|
| `cons x xs` | `a → List a → List a` | prepend `x` (Haskell `x : xs`) |
| `head xs` | `List a → Maybe a` | first element, or `Nothing` for the empty list |
| `tail xs` | `List a → List a` | drop the first element |
| `length xs` | `List a → Int` | element count |
| `map f xs` | `(a → b) → List a → List b` | apply `f` to each element |
| `fold f z xs` | `(b → a → b) → b → List a → b` | left fold — the closure is called `(acc, elem)` |
| `filter p xs` | `(a → Bool) → List a → List a` | keep elements satisfying `p` |
| `list` | `List a` | empty list |
| `empty_map` | `Map k v` | empty map |
| `empty_set` | `Set a` | empty set |
| `insert k v m` / `insert k s` | `k → v → Map k v → Map k v` / `k → Set a → Set a` | insert (replace-on-duplicate); overloaded by arity |
| `lookup k m` | `k → Map k v → Maybe v` | value for key, or `Nothing` |
| `member k c` | `k → Map k v → Bool` / `a → Set a → Bool` | membership test |

List literals `[e1, e2, …]` are `List T n` with `n` = literal length; map
literals `{ "k": v, … }` are `Map String T n`. Bool literals `true`/`false`,
value-track comparisons `==` `!=` `<` `>` `<=` `>=`, and logic `&&` `||`
(`not` is a prefix builtin — `!` stays the wire-cut combinator) are available
in value position only; the signal track is untouched.

Collections are compiled and evaluated through the same `RillProgram` API as
any value-track program:

```rust,no_run
use rill_lang::compile;

// fold (acc, elem) over map (x * 2) of [1.0, 2.0, 3.0] -> 12.0
let mut prog = compile::<f32>(
    "main = fold (fn a b -> a + b) 0.0 (map (fn x -> x * 2.0) [1.0, 2.0, 3.0]);",
).unwrap();
prog.run_value().unwrap();
assert_eq!(
    prog.arena().get(prog.value_outputs()[0].unwrap()).unwrap(),
    &rill_lang::arena::Value::Float(12.0)
);
```

#### Map/Set keys and derived `Eq`/`Ord`

Map keys and set elements can be **any acyclic value type** — scalars, strings,
lists, records, and builtin data types. The compiler derives an `Ord` instance
for every concrete value type (Haskell `deriving`; `Func` is excluded): a
structural order — leaves by value, sums by constructor index then payload,
records by field order, lists lexicographically, maps/sets over their entries.
`Map`/`Set` keep entries sorted by this derived order, so `lookup`/`member`
are O(log n) binary search. A function-typed key has no derived `Ord` instance
and is a **compile error**.

### Higher-kinded types (HKT)

A `data` declaration can take **type parameters**, and a `typeclass` can range
over a type **constructor** (kind `* → *` or `* → * → *`) — higher-kinded
polymorphism with compile-time resolution:

```faust
data Box a = { value: a };          // parameterized user record type

typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
instance Functor List where { fmap g xs = map g xs; }
instance Functor Maybe where {
    fmap g m = match m of { Nothing => Nothing; Just x => Just (g x); };
}

main = length (fmap (fn x -> x * 2.0) [1.0, 2.0, 3.0]);   // Int(3)
```

- **Kinds** — three kinds in v1: `*` (types), `* → *` (unary constructors),
  `* → * → *` (binary constructors). The class variable's **arity** is inferred
  from its use in method signatures (`f a` → 1, `f a b` → 2, bare `a` → 0) and
  checked against the instance: `instance Functor Pair` (Pair has arity 2) is a
  **kind error**.
- **Compile-time inline resolution** — `fmap g xs` unifies the method signature
  with the argument type, instantiates the instance body, β-substitutes the
  arguments, and inlines the result. `fmap` over `List` compiles directly to the
  `map` builtin call — **zero runtime dispatch, no dictionaries**, preserving the
  existing typeclass property.
- Open collections carry no capacity — a kind variable matches the **head
  constructor** (`f a` with `List Float` binds `f := List`, `a := Float`).
- Parameterized user **sums** are not registered as type constructors in v1:
  `data Opt a = Some a | None` works as an ordinary (monomorphic) data type
  (construction + match) but cannot be a typeclass instance.

### Builtin category typeclasses

The category-theory classes `Functor`/`Applicative`/`Monad`/`Monoid`/`Arrow`
are **built in** — declared in a language prelude (`CATEGORY_PRELUDE`) parsed
and registered in `TypeEnv::with_builtins()`, so a program never redeclares
them but may add its own instances. Methods resolve at compile time by
**inline** lowering — zero runtime dispatch, no dictionaries.

```faust
typeclass Functor f     where { fmap:  (a -> b) -> f a -> f b; }
typeclass Applicative f where { pure:  a -> f a; ap: f (a -> b) -> f a -> f b; }
typeclass Monad m       where { return: a -> m a; bind: m a -> (a -> m b) -> m b; }
typeclass Monoid m      where { mempty: m; mappend: m -> m -> m; }
```

- **Auto-derivation** — `instance Monad T` synthesizes `Applicative T`
  (`pure = return`, `ap` via `bind`) and `Functor T` (`fmap` via `bind`);
  `instance Applicative T` synthesizes `Functor T` (`fmap = ap (pure g)`).
  Explicit instances always win, so the prelude's `fmap = map` for `List`
  survives.
- **Result-directed `mempty`** — a nullary method resolves by its **expected
  result type** (`mappend xs mempty` picks the instance by `xs`'s type). A bare
  `mempty` with an unknown expected type is a compile error.
- **`do`-notation** — `do { x <- mx; let y = e; stmt; expr; }` desugars in the
  parser to nested `bind` (`<-` → `bind e (fn x -> rest)`, `let` → `Expr::Let`,
  bare statements → `bind e (fn _ -> rest)`); the final statement is the block's
  result. `a < -b` needs parentheses (`a < (-b)`).

#### `Arrow` and `Kleisli`

The prelude also declares **`Kleisli`**, the free category over a monad (an
arrow `a -> m b` inside the container `m`), and **`Arrow`**, the category of
container morphisms:

```faust
data Kleisli m a b = { unKleisli: a -> m b };

typeclass Arrow a where {
    arr:     (b -> c) -> a b c;
    first:   a b c -> a (Pair b d) (Pair c d);
    compose: a b c -> a c d -> a b d;
    second:  a b c -> a (Pair d b) (Pair d c) =
        k (compose (compose (arr (fn p -> Pair { first: p.second, second: p.first })) (first k)) (arr (fn p -> Pair { first: p.second, second: p.first })));
    both:    a b c -> a d e -> a (Pair b d) (Pair c e) =
        f g (compose (first f) (second g));
    fan:     a b c -> a b d -> a b (Pair c d) =
        f g (compose (arr (fn x -> Pair { first: x, second: x })) (both f g));
}

instance (Monad m) => Arrow (Kleisli m) where {
    arr f        = Kleisli (fn x -> return (f x));
    first k      = Kleisli (fn p -> bind (k.unKleisli p.first) (fn z -> return (Pair { first: z, second: p.second })));
    compose k1 k2 = Kleisli (fn x -> bind (k1.unKleisli x) (fn y -> k2.unKleisli y));
}
```

- **Constraint-qualified instances** — `instance (Monad m) => Arrow (Kleisli
  m)` is the first instance with a **constraint list** before the class name
  and a **partial-application head** (`Kleisli m`). The leading head argument
  `m` is bound at the **call site** from the concrete container's leading type
  arguments — `Kleisli Maybe Float Float` binds `m := Maybe` — and the
  `Monad m` constraint is then discharged by ordinary instance lookup
  (`instance Monad Maybe`). The instance body may therefore call `return` /
  `bind` on the monad directly. A constraint that has no instance is a compile
  error; there is no kind-polymorphic fallback.
- **Default methods** — `second`/`both`/`fan` have **default bodies** in the
  typeclass declaration, written `= param (body)` — a parameter (here `k`)
  followed by a **parenthesized** expression (`second: … = k (compose …)`).
  The default body may itself call other typeclass methods (including other
  defaults — `both` calls `first` and `second`; `fan` calls `both`), which is
  **not** recursion: only a body that re-enters its own method is rejected.
  Resolution precedence is **instance body > class default > compile error**:
  `instance (Monad m) => Arrow (Kleisli m)` supplies `arr`/`first`/`compose`,
  and `second`/`both`/`fan` fall back to the defaults built from them.
- **`arr`/`first`/`compose` are end-to-end runnable** — the canonical program
  composes two arrows and applies the result:

  ```faust
  apply k x = let u = k.unKleisli in u x;
  main = match (apply (compose (arr (fn x -> x + 1.0)) (arr (fn y -> y * 2.0))) 3.0) of {
      Just v => v; Nothing => 0.0;
  };
  // -> Just 8.0, i.e. (3 + 1) * 2
  ```

- **Known limitation: `second`/`both`/`fan` compile but do not yet run.** The
  default bodies are valid and lower to IR, but their **runtime execution is
  deferred**: the arena-capacity heuristic undercounts the deep closure chains
  these defaults build, so a program that calls them panics at build time with
  `value buffer pool exhausted at build time`. This is a known follow-up (the
  correct IR is verified; it needs a larger budget estimate). Do not present
  `second`/`both`/`fan` as runnable.

#### Channel tuples, tuple types, and projections

The `,` combinator doubles as a **channel tuple** that unifies on the track of
its operands: `signal,signal` stays the block-diagram parallel composition;
`value,value` builds a `Pair { first, second }`; a **mixed** `value , signal`
is a compile error (one channel cannot live on both tracks):

```faust
main = (1.0, 2.0);        // value,value -> Pair { first: 1.0, second: 2.0 }
main = (1.0, 2.0).first;  // -> Float(1.0)
```

In **type position**, `(b, d)` is sugar for `Pair b d` — the Arrow signatures
above are `a (Pair b d) (Pair c d)` ≡ `a (b, d) (c, d)`.

A **field projection is a first-class function value**: `(b.f) 3.0` applies
the projected closure, and the bare `b.f 3.0` form works too (this is the
shape the Kleisli instance bodies write — `k.unKleisli p.first`).

A **single-field record** can be constructed newtype-style by passing the
field value directly: `Kleisli (fn x -> …)` is exactly
`Kleisli { unKleisli: fn x -> … }`. The prelude instance bodies use this
shorthand (`arr f = Kleisli (fn x -> return (f x));`).

Reserved names added by the prelude: the Arrow methods `arr`, `first`,
`compose`, `second`, `both`, `fan`, and the data type `Kleisli` (alongside the
existing `fmap`, `pure`, `ap`, `return`, `bind`, `mempty`, `mappend`,
`concat_map`, `append_list`, `concat_string`).

Builtin instances: `Functor`/`Monad` for `List` (`fmap = map`, `bind =
concat_map`), `Maybe`, `Either a`; `Monoid` for `List` (`append_list`), `String`
(`concat_string`), `Float`, `Int`. New value-track IR ops back the category
instances: `ConcatMap`, `AppendList`, `ConcatString` — all buffer-pool-backed,
no per-tick heap allocation.

### Memory model: page arena + RC + COW

Value data lives in a **page-based arena** in the style of Alexandrescu's
"Affordable Allocator", owned by the `RillProgram` and pre-allocated at build
time (no heap growth on the RT path). Value slots form an **embedded free list**
(a freed slot stores the next free index in place of its payload), and collection/
record payload buffers come from a **size-classed buffer pool**, so collection
ops allocate nothing per tick in the default (RT) mode. Slots are managed by
non-atomic reference counting (single-threaded DAG) with **copy-on-write**:
mutating a field of a shared value copies it first. Local variables (including
`main`'s λ-parameters) are **runtime-stack cells** — persistent arena slots that
`SetParameter` writes into directly.

Acyclicity is guaranteed at compile time: a `data`/`newtype` type that
(transitively) references itself is rejected. The arena slot and buffer budgets
are computed from the value instructions and the static subtree sizes of value
outputs, so a well-formed program never exhausts them in the default mode; the
`growable-arena` feature (non-RT) grows the pool instead.

### First-class functions and closures

Functions are first-class values: a **lambda literal** `fn p1 p2 -> body` is a
function value, stored as a `Value::Closure` (a captured environment record
plus a compiled body fragment). Closures capture their free variables **by
value** — an environment snapshot at creation time — so
`adder = fn n -> fn x -> x + n` binds `n` into the closure returned for `x`:

```faust
adder = fn n -> fn x -> x + n;   // a function returning a closure
add2  = adder 2.0;               // partial application: a closure over `x`
main  = add2 3.0;                // -> Float(5.0)
```

- **Currying** — applying fewer arguments than the signature produces a
  closure over the remaining parameters (`add3 = add 3.0; main = add3 4.0` →
  `Float(7.0)`). Named λ-parameter definitions curry too
  (`add5 = add2 5.0`).
- **Higher-order functions** — a lambda parameter can itself be a function:
  `twice = fn f x -> f (f x); double = fn x -> x * 2.0; main = twice double 3.0`
  → `Float(12.0)`. Function values flow through records and projections, so a
  combinator can map over a fixed-shape container:
  `pair_map = fn f p -> Pair { x: f p.x, y: f p.y }`.
- **Signal-wire arguments** — a trailing `_` wire binds to the callee's LAST
  parameter as a signal input: `amp = fn g x -> x * g; main = amp 2.0 _`
  scales the input block by `2.0` (positional wire-capture at the call site).
- **Recursion is forbidden** — the acyclic contract requires every dispatch
  chain to terminate: a definition that (transitively) calls itself is rejected
  at compile time with a `recursive function call` error. Because calls cannot
  recur, the runtime call depth is a static bound: the interpreter
  pre-allocates one value-register frame per compiled fragment, so `ValueCallFunc`
  dispatch performs **no heap allocation** on the RT path.

The type of a function value is structural — `ValueTy::Func(arg_tys, ret_tys)` —
over its value parameters and results. Signal-wire parameters are positional
wire-captures at the call site and are not part of the type. A lambda parameter
used as a callee (`f (f x)`) is inferred as a `Func`, and a record parameter is
inferred as its `Data` type, so lowering resolves field projections and dispatch
arities at compile time.

Deferred: runtime typeclass dispatch (methods resolve at compile time),
value-state persistence beyond per-tick scratch, and `strict`/`complete`
compiler modes (the acyclicity and capacity checks above are the foundation of
the `strict` contract).

### Known v1 limitations

- **Single value output** — `main` exposes one value channel; `main = p, p`
  (multi-value fan-out) and mixed signal+value outputs are rejected at
  lowering.
- **No signal parameters in the `Func` type** — a lambda's signal-wire
  parameters are positional wire-captures, not part of the function value type
  (the trailing `_` wire must exactly complete the value arity).
- **Higher-order parameters are monomorphic** — a lambda parameter's `Func`
  signature is fixed at its first call site; v1 does not generalize a function
  parameter over multiple call shapes.
- **β-substitution gaps** — `substitute` does not descend into `let` /
  `ActorParam`, and reduction does not run inside `match` / field
  project/update / record bodies; a user-def call in those positions errors at
  lowering.

Deferred:

- **User-written `Eq`/`Ord` instances** — instances are compiler-derived for
  all data types (except `Func`); a custom `instance Ord` is not yet
  supported.
- **`Hashable` / hash maps** — `Map`/`Set` are binary-search structures over
  the derived total order; no hash-based containers.
- **Length-indexed lists** — the type carries capacity, not length, so the
  runtime capacity check cannot yet be eliminated statically.
- **String operations beyond equality** — `String` supports construction,
  map keys, and `==`/`!=`; no `concat`, formatting, or `show`.
- **List `concat`/`append`/`reverse`; Set `union`/`intersection`** — the
  current collection operations are the subset in the table above.
- **Signal-track `Bool`** — comparisons and logic are value-track only;
  `Scalar` (signal elements) is untouched.

## Status

The language ships the IO/streams core: `main : (String…) -> IO ()` executed to
completion by `ProgramRunner`, a Haskell-faithful `ST` with isolated state,
io-streams-style memory and stdio streams (`fromList`/`toList`/`read`/`write`/
`atEnd`/`unread`/`connect`/`makeInputStream`/`makeOutputStream`), and a retained
pure-value tick. The signal layer keeps its block-arrow model (`Scalar` /
`Block` / `ArrowTy`) with block-diagram combinators as arrow laws, feedback and
delay, Hindley-Milner types, Haskell-style definitions with β-reduction, CAF
free variables with binding-level laziness, `let`/`where` binding groups, a
27-built-in registry (DSP, effects, oscillators, mixer/EQ, analog, spectral,
complex, lofi), RT-safe named parameters (`param()` and `?name`), and records
for built-in configuration. The value track supports first-class data
(`data`/`type`/`newtype`/`typeclass`, arena+RC+COW memory, runtime-stack cells,
element-wise value arithmetic, first-class functions with closures and runtime
dispatch, currying, higher-order combinators), **higher-kinded types**, and
**first-class Haskell-style collections** (`List`/`Map`/`Set`, `Maybe`/`Pair`/
`Either`, `Bool`/`String`, derived `Eq`/`Ord`, list/map literals).

> **Removed in this branch:** `Algorithm`/`MultichannelAlgorithm` impls for
> `RillProgram` (no `process()`), `ProgramEngine`, `readInput`/`writeOutput`,
> buffer-λ `main`, `compile_graph()`/`CompiledGraphEngine`, and the graph-node
> mailbox. Graph compilation exists only behind the off-by-default `graph`
> feature.

Deferred to follow-on work:

- the **Cranelift `jit`** backend (the linear IR is the shared lowering target);
- **trampolining the `bind` chain** to remove the documented stack-depth limit;
- a **signal-transform host over streams** (the deferred audio plan: per-block
  processing of `InputStream`/`OutputStream float` inside a program);
- **graph compilation repair** and re-embedding a program in a signal graph;
- **signal-rate** (per-sample) modulation of imported built-in parameters
  (current parameter modulation is control-rate/per-block);
- composed expressions as built-in arguments;
- runtime typeclass dispatch, cross-node value ports, and the
  `strict`/`complete` compiler-mode contract;
- user-written `Eq`/`Ord` instances and hash-based containers (see
  [Known v1 limitations](#known-v1-limitations)).

[`RillLangDef`]: https://docs.rs/rill-lang
