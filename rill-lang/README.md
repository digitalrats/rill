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
let mut prog = compile::<f32>("main = _ * 0.5").unwrap();

let mut out = [0.0f32; 4];
prog.process(Some(&[1.0, 2.0, 4.0, 8.0]), &mut out).unwrap();
assert_eq!(out, [0.5, 1.0, 2.0, 4.0]);
```

Three entry points:

- **`compile(src)`** — no built-ins, no sample rate. Pure block-diagram math.
- **`compile_with(src, &registry, sample_rate)`** — with a built-in registry for
  stateful DSP (filters, oscillators, effects).
- **`compile_graph(src, &registry, sample_rate)`** — compiles into a
  `RillGraphEngine` with actor mailbox support for `SetParameter` commands.

## The language in one screen

A program is a list of definitions ending in `;`. One must be named `main`.
The entry point can be **SISO** (1→1) or **multi-IO** (N→M) when the `router`
feature is enabled, supporting multi-channel nodes like mixers and EQs.

```faust
gain x = x * 0.5;            // a one-argument function (juxtaposed args — no parens)
main    = gain _;             // apply it to the input wire
```

### Application syntax

Function calls use **bracket-free juxtaposition** — space-separated arguments,
no parentheses:

```faust
main = _ : lowpass 1000.0 0.7;   // juxtaposed
main = lowpass _ 1000.0 0.7;     // signal as first-class argument
```

Parenthesized form `name(arg, ...)` is also supported (for compatibility),
but juxtaposed is canonical.

### Records

Built-ins are configured with **record literals** `{ key: val }`:

```faust
main = mixer _1 _2 { channels: 3, gain: 0.8 };
main = eq_parametric _ { bands: [{ freq: 1000.0, q: 0.7, gain_db: 3.0 }] };
```

Records can be nested — the EQ `bands` field is a list of
`{ freq, q, gain_db, band_type }` records.

### Actor parameters (`?name=default`)

Late-binding parameter slots resolved at runtime via the engine mailbox:

```faust
main = _ : lowpass ?cutoff=1000.0 ?resonance=0.7;
```

The `?name=default` syntax creates a named parameter slot that receives
`SetParameter` commands from the actor system. Names are stable — servos, LFOs,
and MIDI maps target them directly. Within `where` blocks, dot-notation
namespacing applies: `?osc.freq=440.0`.

### Multi-IO

Multi-channel programs implement `MultichannelAlgorithm<T>` when compiled with
the `router` feature:

```faust
main = mixer _1 _2 { channels: 2, buses: 0 };  // 2→2
main = dry_wet _ _effected { mix: 0.5 };        // 2→1 (interleaved)
```

The graph engine dispatches to `MultichannelAlgorithm::process()` for multi-IO
nodes; SISO programs (0 or 1 input → 1 output) use the single-channel fast path.

### Primitives

| Syntax | Meaning | Arity |
|---|---|---|
| `_` | identity wire | 1 → 1 |
| `!` | cut (discard a wire) | 1 → 0 |
| `3`, `3.5` | int / float literal | 0 → 1 |
| `+ - * / %` | arithmetic (as a block) | 2 → 1 |
| `sin cos tan sqrt exp ln tanh abs` | math builtins | 1 → 1 |
| `min max` | selection | 2 → 1 |

**Complex arithmetic** (builtins, always available):

| Syntax | Meaning | Channels |
|---|---|---|
| `complex(re, im)` | complex constant generator | 0 → 2 |
| `conj x` | conjugate: `re + i·im → re − i·im` | 2 → 2 |
| `re x`, `im x` | real / imaginary part | 2 → 1 |
| `norm x` | magnitude: `√(re² + im²)` | 2 → 1 |
| `arg x` | phase: `atan2(im, re)` | 2 → 1 |
| `cmul a b` | complex multiply | 4 → 2 |
| `cadd a b` | complex add | 4 → 2 |

Complex signals are pairs of wires (re + im). Use the parallel combinator `,` to
combine two complex sources into a 4-wire input for `cmul`/`cadd`:

```faust
main = complex 3.0 4.0 , complex 2.0 0.0 : cmul () : re ();  // → 6.0
main = complex 1.0 2.0 , complex 3.0 4.0 : cadd () : norm (); // → ≈7.21
```

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
main = + ~ _;              // integrator:         y[n] = x[n] + y[n-1]
main = + ~ (_ * 0.5);      // leaky integrator:    y[n] = x[n] + 0.5·y[n-1]
main = _ @ 1;              // one-sample delay
main = _ <: (_ , _) :> +;  // fan-out then sum = 2·x
```

## Built-in functions

rill-lang supports calling stateful DSP/model built-ins from
`rill-core-dsp`/`rill-core-model`/`rill-fft` via `compile_with(src, &registry, sample_rate)`.

| Category | Builtins | Feature |
|---|---|---|
| Filters | `onepole`, `moog` (sample), `lowpass`, `highpass`, `biquad` (block) | always |
| Oscillators | `sine`, `saw`, `square`, `triangle`, `noise` (block) | always |
| Effects | `delay`, `distortion`, `limiter` (block) | always |
| Mixer/EQ | `mixer`, `eq_parametric`, `dry_wet`, `graphic_eq` (block) | `router` |
| Analog | `analog_moog`, `cassette_deck`, `tape_bridge` (block) | `analog` |
| Spectral | `spectralgate`, `spectraldelay`, `convolver` (block) | `fft` |
| Complex | `complex`, `conj`, `re`, `im`, `norm`, `arg`, `cmul`, `cadd` | always |
| Lofi | `lofi`, `ay38910` (block) | `lofi` |

Built-ins use **unified argument syntax**: signals are first-class arguments
passed by juxtaposition (e.g. `lowpass _ 1000.0 0.7`). Some built-ins accept
variadic signal inputs — `mixer` takes any number of signals followed by a
record:

```faust
main = mixer _ ch2 ch3 ch4 { channels: 4, buses: 2 };
main = dry_wet _ wet { mix: 0.7 };
main = eq_parametric _ { bands: [{ freq: 500.0, q: 2.0, gain_db: -3.0 }] };
```

Per-sample built-ins (`onepole`, `moog`) are feedback-legal; whole-buffer
built-ins (`lowpass`, `highpass`, etc.) are opaque block steps and cannot
appear inside `~`. Bindings and registries live in `rill-adrift`
(`lang_builtins::full_registry`), with per-crate `register_lang_builtins()`
functions for selective registration.

## Two parameter models

rill-lang supports **two** parameter mechanisms:

### `?name=default` — actor parameters (canonical)

Late-binding slots resolved at runtime via the engine mailbox. When compiled
with `compile_graph()`, each `?name=default` becomes a named parameter
addressable by `SetParameter` commands:

```faust
main = _ : lowpass ?cutoff=1000.0 ?resonance=0.7;
```

Where-block definitions create namespaced parameters with dot notation:

```faust
main = osc : filt where
    osc  = sine ?freq=440.0 0.5 0.0
    filt = _ : lowpass ?cutoff=1200.0 0.7
-- Exposes: "osc.freq", "filt.cutoff"
```

Parameters are addressed by `"anchor.param"` format (e.g. `"osc.freq"`) —
stable names across sessions, targetable by servos, LFOs, and MIDI maps.

### `param("name", default)` — legacy DSL parameter

The older `param()` built-in creates an inline parameter slot, useful when
compiling directly to `RillProgram` without the graph engine:

```faust
main = _ * param("gain", 0.5);
```

Both mechanisms coexist. `?name` is the canonical form for graph nodes;
`param()` is available for standalone `RillProgram` use. The native
`smooth(x, ms)` one-pole provides zipper-free interpolation when parameters
change at block boundaries.

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

let def = RillLangDef::new("gain", "main = _ * 0.5;");
let prog = compile_def::<f32>(&def).unwrap();
```

## Graph integration

The `rill-adrift` umbrella crate exposes `rill-lang` behind its `lang` feature.
Two paths to runtime:

1. **`compile_graph()`** — compiles source into a `RillGraphEngine` with actor
   mailbox support, ready to wire into a graph's processing pipeline.
2. **`rill/lang` factory node** — serialized graph nodes of type `rill/lang`
   embed their source as a `source` parameter:

```json
{ "id": 0, "type_name": "rill/lang", "parameters": { "source": "main = _ * 0.5;" } }
```

Setting the `source` parameter at runtime recompiles and hot-swaps the program.

## Execution model

The interpreter compiles the linear IR into a hybrid schedule via SCC analysis:
feedforward regions run whole-buffer through the `rill_core::math::vector` SIMD
eDSL, while feedback (`~`) and delay (`@`) recurrences run per-sample. The block
path computes in `T` with zero heap allocation on the hot path. A Cranelift JIT
backend is still planned and will reuse the same IR.

## Debug infrastructure (`debug` feature)

When the `debug` Cargo feature is enabled, the IR gains a `ProbePoint` instruction
for signal-level diagnostics. Each graph node compiled via `rill-graph`'s `build_ir()`
automatically gets a probe at its output:

- **`ProbePoint { id, src, dst }`** — pass-through IR instruction that copies a register
  value and simultaneously captures it to a lock-free probe slot
- **`ProbeSlot`** — atomic flags (`enabled`, `break_flag`, `paused_flag`) plus an
  SPSC queue for frame transport to a non-RT collector thread
- **`DebugControl`** — shared atomics (`global_pause`, `global_resume`) for
  pause/resume execution control without syscalls

Probe data flows through `rill-telemetry`'s `CollectorThread` and can be inspected
via `rill-analyzer`. Zero overhead when the feature is disabled.

## Status

MVP. Deferred to follow-on work: the Cranelift `jit` feature, foreign references
to existing rill DSP primitives, and a SIMD-aware IR.

## License

Apache-2.0. See the workspace `LICENSE.md`.

[`rill_core::Algorithm`]: https://docs.rs/rill-core
[`RillLangDef`]: https://docs.rs/rill-lang
[`compile_def`]: https://docs.rs/rill-lang
