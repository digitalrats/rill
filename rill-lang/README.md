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
  `CompiledGraphEngine` with actor mailbox support for `SetParameter` commands.

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
| `A ~ B` | feedback tap (1-tick delay) | `out(B) ≤ in(A)` | `(in A − out B, out A)` |
| `A @ n` | integer delay (`n` const) | `A` is `_ → 1` | same as `A` |

Precedence, loosest → tightest: `~` < `:` < `:>` < `<:` < `,` < `+ -` < `* / %` < `@` < unary `-` < atoms.

### Idioms

```faust
main = integrator;           // integrator:         y[n] = x[n] + y[n-1]
main = leaky_integrator 0.5; // leaky integrator:  y[n] = x[n] + 0.5·y[n-1]
main = + ~ _;               // short form for `integrator`
main = + ~ (_ * 0.5);       // short form for `leaky_integrator 0.5`
main = _ @ 1;              // one-sample delay
main = _ <: (_ , _) :> +;  // fan-out then sum = 2·x
```

## Built-in functions

rill-lang supports calling stateful DSP/model built-ins from
`rill-core-dsp`/`rill-core-model`/`rill-fft` via `compile_with(src, &registry, sample_rate)`.

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

Built-ins use **unified argument syntax**: signals are first-class arguments
passed by juxtaposition (e.g. `lowpass _ 1000.0 0.7`). Some built-ins accept
variadic signal inputs — `mixer` takes any number of signals followed by a
record:

```faust
main = mixer _ ch2 ch3 ch4 { channels: 4, buses: 2 };
main = dry_wet _ wet { mix: 0.7 };
main = eq_parametric _ { bands: [{ freq: 500.0, q: 2.0, gain_db: -3.0 }] };
```

All built-ins are whole-buffer `BlockBuiltin`s — opaque block steps implementing
`Algorithm<T>`. Per-sample state (filters, integrators) lives inside the built-in,
so the engine stays block-only and SIMD-friendly. Bindings and registries live in
`rill-adrift`
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
Three paths to runtime:

1. **`compile_graph()`** — compiles source into a `CompiledGraphEngine` with actor
   mailbox support, ready to wire into a graph's processing pipeline.
2. **`GraphBuilder::build_ir()`** (from `rill-graph`) — builds a multi-node
   `GraphIr` from a programmatic topology, then calls the same
   `graph_compiler::compile()` to produce a `CompiledGraphEngine`.
3. **`rill/lang` factory node** — serialized graph nodes of type `rill/lang`
   embed their source as a `source` parameter:

```json
{ "id": 0, "type_name": "rill/lang", "parameters": { "source": "main = _ * 0.5;" } }
```

Setting the `source` parameter at runtime recompiles and hot-swaps the program.

## Execution model

The interpreter compiles the linear IR into a block-only schedule via SCC analysis:
every instruction runs whole-buffer through the `rill_core::math::vector` SIMD
eDSL, and `BlockBuiltin`s are opaque whole-buffer `Algorithm` calls. Feedback (`~`)
uses a double-buffered block state with a one-tick shadow copy; delay (`@`) uses a
block-level ring buffer. No instruction runs sample-by-sample. The block path
computes in `T` with zero heap allocation on the hot path. A Cranelift JIT backend
is still planned and will reuse the same IR.

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

## First-class data (`data`/`type`/`newtype`/`typeclass`)

Beyond signals, a program can carry **value channels** — one arena value per
tick, typed and processed in a per-tick value-track phase:

```faust
data Point = { x: Float, y: Float };          // record type
data Shape = Circle Float | Rect Float Float; // sum type
newtype Hz  = Float;                          // distinct wrapper
type Angles = Float;                          // synonym
typeclass Show a where { show: a; }           // ad-hoc polymorphism
instance Show Float where { show f = f; }

p = Point { x: 2.0, y: 3.0 };
main = p.x;                                    // field projection -> Float(2.0)
```

Values live in a **page-based arena** in the style of Alexandrescu's
"Affordable Allocator": a pre-allocated pool of value slots (an embedded free
list — a freed slot stores the next free index in place of its payload) plus a
size-classed **payload buffer pool** for collection/record element buffers, so
collection ops perform **no heap allocation** on the processing path in the
default (RT) mode. Copy-on-write mutation (`p.x := 3.0`), reference counting,
and acyclic-by-construction data types are unchanged; the pool is bounded by a
conservative build-time budget, and a program that exhausts it (default mode)
or grows it (`growable-arena` feature, non-RT) hits a detectable no-op /
runtime error. `typeclass` methods resolve at compile time (no runtime
dispatch).

### Builtin value types and type constructors

The value track ships scalar value types and builtin **type constructors**
applied by juxtaposition (`List Float`):

| Type | Kind | Meaning |
|---|---|---|
| `Bool`, `String` | `*` | scalar value types (value track only) |
| `List a`, `Set a` | `* → *` | ordered list / unordered set (**open** — grow up to the pool) |
| `Map k v` | `* → * → *` | key→value map (**open**) |
| `Maybe a` | `* → *` | optional `a` (`Just a` / `Nothing`) |
| `Pair a b` | `* → * → *` | pair (`{ first, second }`) |
| `Either a b` | `* → * → *` | sum (`Left a` / `Right b`) |

### First-class collections

`List`/`Map`/`Set` are first-class arena containers with Haskell-style ops.
Collections are **open** — there is no capacity in the type, and `cons`/
`insert` grow freely up to the pre-allocated pool budget (exceeding it in the
default RT mode is a detectable no-op, not a per-container error).
`map`/`fold`/`filter` take the function **first**; `cons` **prepends**
(Haskell `x : xs`); empty containers are `list` / `empty_map` / `empty_set`;
`insert` is overloaded by arity (Map 3-arg, Set 2-arg); `not` is a prefix
builtin.

```faust
xs  = [1.0, 2.0, 3.0];            // List Float
ys  = cons 10.0 (list);           // prepend; the list grows
h   = head xs;                    // Maybe Float: Just 1.0 / Nothing
n   = length xs;                  // Int
z   = map (fn x -> x * 2.0) xs;   // function first
s   = fold (fn a b -> a + b) 0.0 xs;   // Float
f   = filter (fn x -> x > 1.0) xs;     // List Float

m  = { "a": 1.0, "b": 2.0 };      // Map String Float
m1 = insert "a" 9.0 m;            // replace-on-duplicate
v  = lookup "a" m;                // Maybe Float
b  = member "a" m;                // Bool
st = insert 1 (empty_set);        // Set Int
```

Map keys and set elements can be **any acyclic value type**: the compiler
derives `Eq`/`Ord` instances for every data type (except `Func`) — a structural
total order, used to keep entries sorted for O(log n) `lookup`/`member`. A
function-typed key is a compile error.

### Higher-kinded types (HKT)

`data` can take type parameters and `typeclass` can range over a type
**constructor** (kind `* → *` / `* → * → *`). Resolution is compile-time
**inline** — `fmap` over a `List` compiles directly to the `map` builtin, with
zero runtime dispatch:

```faust
data Box a = { value: a };
typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
instance Functor List where { fmap g xs = map g xs; }
instance Functor Maybe where {
    fmap g m = match m of { Nothing => Nothing; Just x => Just (g x); };
}
main = length (fmap (fn x -> x * 2.0) [1.0, 2.0, 3.0]);   // Int(3)
```

Kind arity is inferred from method signatures and checked — `instance Functor
Pair` (Pair has arity 2) is a kind error.

### Builtin category typeclasses (`Functor`/`Applicative`/`Monad`/`Monoid`/`Arrow`)

The category-theory classes ship **built in** — declared in a language prelude
registered at compile time, so a program never redeclares them but may add its
own instances. Methods resolve at compile time by **inline** lowering (zero
runtime dispatch), and `instance Monad T` **auto-derives** `Applicative T` and
`Functor T` (explicit instances always win):

```faust
typeclass Functor f     where { fmap:  (a -> b) -> f a -> f b; }
typeclass Applicative f where { pure:  a -> f a; ap: f (a -> b) -> f a -> f b; }
typeclass Monad m       where { return: a -> m a; bind: m a -> (a -> m b) -> m b; }
typeclass Monoid m      where { mempty: m; mappend: m -> m -> m; }
```

Builtin instances: `Functor`/`Monad` for `List`, `Maybe`, `Either a`;
`Monoid` for `List` (`append_list`), `String` (`concat_string`), `Float`, `Int`.

**Result-directed dispatch (`mempty`).** A nullary method has no selector
argument, so it resolves by its *expected result type* — `mappend xs mempty`
resolves `mempty` by the type of `xs`:

```faust
main = length (mappend [1.0, 2.0] mempty);   // List Float, mempty = list -> Int(2)
main = mappend mempty 3.5;                    // Float, mempty = 0.0 -> Float(3.5)
```

Using `mempty` where its result type is unknown is a compile error.

**`do`-notation** desugars to nested `bind` (Haskell `<-`):

```faust
mx = Just 1.0;
my = Just 2.0;
main = match (do { x <- mx; y <- my; pure (x + y); }) of { Nothing => 0.0; Just z => z; };
// == bind mx (fn x -> bind my (fn y -> pure (x + y))) -> Just 3.0
```

`do { x <- mx; let y = e; stmt; expr; }` supports `<-` binds, `let` bindings,
and bare monadic statements (each desugars to `bind`); the final statement is
the block's result. Note that `a < -b` is a comparison followed by negation —
parenthesize: `a < (-b)`.

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

This is the first **constraint-qualified instance** in the prelude. The
instance head `Kleisli m` is a partial application: the leading head argument
`m` is bound at the **call site** from the concrete container's leading type
arguments — `Kleisli Maybe Float Float` binds `m := Maybe` — and the
`Monad m` constraint is then discharged by ordinary instance lookup
(`instance Monad Maybe`), so the bodies may call `return`/`bind` directly.

**`second`/`both`/`fan` are default methods** — the typeclass declares a
default body (with parameters, parenthesized) that any instance may override.
Resolution precedence is **instance body > class default > compile error**:
`instance (Monad m) => Arrow (Kleisli m)` provides `arr`/`first`/`compose`,
and `second`/`both`/`fan` fall back to the class defaults built from them.
The `second` default body is written `= k (compose …)` — `k` is the parameter
and the body is parenthesized.

```faust
apply k x = let u = k.unKleisli in u x;
main = match (apply (compose (arr (fn x -> x + 1.0)) (arr (fn y -> y * 2.0))) 3.0) of {
    Just v => v; Nothing => 0.0;
};
// -> Just 8.0, i.e. (3 + 1) * 2
```

**Status.** `arr`/`first`/`compose` are fully working end-to-end — the example
above compiles, runs, and produces `Just 8.0`. `second`/`both`/`fan` are
declared and **compile** (their default bodies lower), but their **runtime
execution is not yet supported**: the arena-capacity heuristic undercounts the
deep closure chains these defaults build, so a program that actually calls
them panics at build time with `value buffer pool exhausted at build time`.
This is a known, deferred follow-up — do not rely on `second`/`both`/`fan` at
runtime yet.

#### Channel tuples, tuple types, and projections

The `,` combinator now doubles as a **channel tuple** — it unifies on the
track of its operands. `signal,signal` stays the block-diagram parallel
composition; `value,value` builds a `Pair { first, second }`; a **mixed**
`value , signal` is a compile error (one channel cannot live on both tracks):

```faust
main = (1.0, 2.0);        // value,value -> Pair { first: 1.0, second: 2.0 }
main = (1.0, 2.0).first;  // -> Float(1.0)
```

In **type position**, the same syntax desugars to the pair type: `(b, d)` is
sugar for `Pair b d` — that is how the Arrow methods above spell their pair
arguments (`a (Pair b d) (Pair c d)` ≡ `a (b, d) (c, d)`).

A **field projection is a first-class function value** — a projected closure
can be applied directly, parenthesized or bare:

```faust
data Box = { f: Float -> Float };
b = Box { f: fn x -> x * 2.0 };
main = (b.f) 3.0;         // parenthesized projection applied: Float(6.0)
main = b.f 3.0;           // bare projection applied too
```

A **single-field record** can be constructed newtype-style, passing the field
value directly instead of a record literal — `Kleisli (fn x -> …)` is exactly
`Kleisli { unKleisli: fn x -> … }`. The prelude instance bodies above use this
shorthand (`arr f = Kleisli (fn x -> return (f x));`).

Reserved method/builtin names from the prelude: `fmap`, `pure`, `ap`,
`return`, `bind`, `mempty`, `mappend`, `concat_map`, `append_list`,
`concat_string`, plus the Arrow methods and the Kleisli data type: `arr`,
`first`, `compose`, `second`, `both`, `fan`, `Kleisli`.

## First-class functions and closures

Functions are first-class values: a **lambda literal** `fn p -> body` compiles
to a `Value::Closure` (a by-value environment snapshot plus a compiled body
fragment), and `ValueCallFunc` performs real runtime dispatch:

```faust
adder = fn n -> fn x -> x + n;   // a function returning a closure
add2  = adder 2.0;               // partial application (currying)
main  = add2 3.0;                // -> Float(5.0)

twice  = fn f x -> f (f x);      // a higher-order combinator
double = fn x -> x * 2.0;
main   = twice double 3.0;       // -> Float(12.0)

amp = fn g x -> x * g;           // trailing `_` is a signal-wire argument
main = amp 2.0 _;                // input block scaled by 2.0
```

- Lambda parameters can themselves be functions (HOF), flow through records
  and projections, and are typed structurally (`ValueTy::Func(arg_tys, ret_tys)`).
- **Recursion is forbidden**: a definition that transitively calls itself is
  rejected at compile time. Because calls cannot recur, the runtime call depth
  is a static bound and the interpreter pre-allocates the dispatch register
  frames — `ValueCallFunc` performs **no heap allocation** on the RT path.

## Branching and pattern matching

`if cond then a else b` and `match` are pure expressions on the value track,
re-evaluated every tick. A runtime `Bool` — e.g. a main λ-parameter compared to
a threshold and written via `SetParameter` — switches the active branch on
each tick:

```faust
main g = if g > 0.5 then 1.0 else 0.0;
```

`match` supports constructor, literal (`0`, `1.5`, `true`, `"s"`), wildcard
`_`, variable, and nested patterns plus Haskell-style guards. An uppercase
initial is a constructor, a lowercase initial is a binding. Matches must be
exhaustive (every constructor covered, or a `_`/variable arm); a guarded arm
whose guard fails falls through to the next arm, and a residual non-match is a
runtime `ProcessError::Processing`.

## Status

MVP. The value track ships first-class Haskell-style collections
(`List`/`Map`/`Set` with strict type-carried capacities, `Maybe`/`Pair`/
`Either`, `Bool`/`String` value types) and higher-kinded types (parameterized
`data`, kind polymorphism over type constructors, compile-time inline
resolution), with runtime control flow on the value track (`if`/`match`).
Deferred to follow-on work: the Cranelift `jit` feature, foreign
references to existing rill DSP primitives, a SIMD-aware IR, runtime typeclass
dispatch, user-written `Eq`/`Ord` instances and hash-based containers, and
`strict`/`complete` compiler modes.

## License

Apache-2.0. See the workspace `LICENSE.md`.

[`rill_core::Algorithm`]: https://docs.rs/rill-core
[`RillLangDef`]: https://docs.rs/rill-lang
[`compile_def`]: https://docs.rs/rill-lang
