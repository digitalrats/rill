# rill-lang

A Faust-style functional DSL for rill: a Hindley–Milner core with algebraic data
types, typeclasses, first-class closures, an arena-backed value track, and
Haskell-faithful `IO`/`ST` monads. Source is compiled at runtime and executed by
one of two paths:

| Path | Entry point | What runs |
|---|---|---|
| **Program executor** | `compile_runner` → `ProgramRunner` | `main : (String…) -> IO ()`, run to completion against a realized `World` (memory streams + stdio) |
| **Single-run value eval** | `test_eval` / `test_eval_with_mode` | Evaluate one expression to WHNF against a `defs` context; the result is read from `RillProgram::result()` (`copy_buffer(r, dst)` for whole-buffer results) |

A program executes its value entry exactly ONCE — there is no public re-run
surface. The `RillProgram` returned by `test_eval` carries the run's single
result in `main`/`result()`.

The interpreter is safe and allocation-free after warm-up: values live in a
fixed, page-based arena, and a program that exceeds the budget reports a clean
error instead of touching the heap. A Cranelift JIT backend is planned behind a
future `jit` feature; both share the same linear IR, so the front-end is
unaffected when the JIT lands.

> **Removed in the IO/streams core change (this branch).** `rill-lang` no longer
> implements `Algorithm<T>`/`MultichannelAlgorithm<T>` for `RillProgram`; there
> is no `process()`, no `ProgramEngine`, no `readInput`/`writeOutput`, and no
> buffer-λ `main`. The old `compile_graph()` graph-execution entry point is gone
> as well: what remains of graph compilation sits behind the off-by-default
> `graph` feature (`rill_lang::graph`), does not build at this revision (its
> `reconstruct.rs` still targets the removed `Par`/`Split`/`Cut` AST), and is not
> built by the workspace gates. Programs express their I/O as streams inside the
> program, not as buffers handed in from outside. See *Migrating* below.

## Quick start: an IO program

```rust
use rill_lang::{compile_runner, Registry};

let mut runner = compile_runner::<f32, 256>(
    r#"main = do { write (Just "hi") stdout; return () };"#,
    &Registry::new(),
    44_100.0,
)
.unwrap();

runner.run().unwrap();
assert_eq!(runner.stdout(), ["hi"]);
```

`main` takes String launch arguments, bound with `set_args(&[String])`, and
returns `IO ()`. The runner owns the program's `World`: `stdout`/`stderr` are
captured into the runner (not echoed to the process console) and read back with
`stdout()` / `stderr()`, which report the console built up by **all runs since
construction**. `set_stdin(&[String])` injects the line-oriented input that
`read stdin` yields, rewinding the stream for the next run.

A program whose `main` reaches `return ()` runs to completion cleanly, even when
it performs no I/O; a `main` whose final action's result is not `()` is a clean
`ProcessError` (`` `main` must have type `IO ()` ``) rather than a silently
discarded value — `return 0.0` in a `do` block is rejected as well. Because the
`Monad` method is resolved from the expected type, a bare `main = return ()`
does not type-check: fix the monad with a preceding effect (as above) or with a
`bind` whose result type is known.

### Entry points

| Function | Produces | Use |
|---|---|---|
| `compile_runner(src, &registry, sample_rate)` | `ProgramRunner<T, BUF>` | The executor for `main : IO ()` programs; compiles in `Complete` (lazy) mode unless the source has a mode pragma |
| `compile(src)` | `RillProgram<T, 256>` | Compiles a value-`main` program without running it (still accepts value-`main` until the `IO ()` gate; do not run) |
| `compile_with_ffi(src, &ffi, sample_rate)` | `RillProgram<T, 256>` | Compiles against a foreign-function registry without running |
| `test_eval(defs, expr, &ffi, sample_rate)` | `RillProgram<T, 256>` | Evaluate `expr` (strict mode) against the `defs` context and RUN it once; read the result with `result()` / `copy_buffer(r, dst)` |
| `test_eval_with_mode(defs, expr, &ffi, sample_rate, mode)` | `RillProgram<T, 256>` | Same, under an explicit evaluation mode |
| `RillProgram::result()` | `Option<ArenaRef>` | The single run's result ref |
| `RillProgram::copy_buffer(r, dst)` | `bool` | Copy a whole-buffer result (`Value::Buffer`) into `dst`; `false` on non-buffer refs |

### The pure-value path

`test_eval` builds `"{defs}\n__rill_eval = ({expr});"` and compiles it through
the same pipeline as any program — `defs` carries the program's top-level
bindings, `expr` the expression to evaluate. The entry marker is internal
(never `main`), so `main` in `defs` is an ordinary binding. This is the path for
pure computation — collections, records, closures, typeclass methods — without
I/O:

```rust
use rill_lang::{arena::Value, ffi::ForeignRegistry, test_eval};

let ffi = ForeignRegistry::<f32>::new();
let prog = test_eval::<f32, 256>("", "1.0 + 2.0", &ffi, 44_100.0).unwrap();
assert_eq!(
    prog.arena().get(prog.result().unwrap()).unwrap(),
    &Value::Float(3.0),
);
```

Signal-block built-ins (filters, oscillators, effects; see *Built-in functions*)
lower to whole-buffer values: `main = sine …` evaluates to a `Value::Buffer`,
inspected with `copy_buffer(result().unwrap(), &mut out)`. There is no
signal-transform executor and no re-run surface — a program executes its entry
exactly once.

## Streams

Streams are ordinary values: `InputStream a` and `OutputStream a`, generic in the
element type, with the io-streams operation surface. `Nothing` means EOF on a
read and close on a write.

```haskell
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

A complete program — drain a memory stream and render each element:

```haskell
show_all acc x = concat_string acc (show_float x);
main = do {
    xs <- fromList [1.0, 2.0, 3.0];
    ys <- toList xs;
    write (Just (fold show_all "" ys)) stdout;
    return ()
};
```

- **Memory streams** — `fromList`/`toList` round-trip, `unread` pushes an element
  back, `connect` pipes an input into an output, and `makeInputStream`/
  `makeOutputStream` wrap program closures as sources/sinks.
- **`toList` drains** any input stream to EOF: a pushed-back prefix first, then
  the source action, which is re-run per element until it reports `Nothing`.
- **Console** — `stdout`/`stderr`/`stdin` are runtime-owned. `write Nothing`
  closes a **program-owned** sink (`makeOutputStream`); on the console handles it
  is inert — a program cannot close the runtime's console.
- **Rendering** — there is no `Show` typeclass in the IO surface: use
  `show_float`, `show_int`, `show_bool`, and `concat_string`.

## Stack depth

The lazy `IO` interpreter evaluates one nested `bind` per statement in a `do`
block, so running `main` to completion recurses once per statement — roughly
**300 KiB of native stack per statement in a debug build**. A multi-statement
stream program therefore overflows a default 2 MiB thread stack (an abort, not a
clean error). Run such programs on a grown-stack thread:

```rust
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

`ProgramRunner` is **not `Send`** (it owns `Rc<RefCell<…>>` state), so compile
*inside* the closure, not before it. `run_on_stack` resumes a panic on the
calling thread, and a failed spawn panics (a host-resource failure, not a program
error). Trampolining the `bind` chain to remove the limit is a planned follow-up.

## Migrating from the signal-transform API

| Removed | Replacement |
|---|---|
| `compile(...)` then `prog.process(inputs, outputs)` | `compile_runner(...)` then `runner.run()`; describe the work as stream reads/writes inside `main` |
| `impl Algorithm<T> for RillProgram`, `BlockBuiltin` driving `RillProgram` | Built-ins remain callable *inside* a program; there is no transform wrapper around a whole program |
| `readInput` / `writeOutput` and indexed variants | `fromList` / `toList`, `read` / `write`, `connect`, `makeInputStream` / `makeOutputStream` |
| Buffer-λ `main` (`main = _ * 0.5`) | `main : (String…) -> IO ()`; a value `main` remains valid for the pure-value tick |
| `compile_graph()`, `CompiledGraphEngine`, `MultichannelAlgorithm` dispatch | Not available here: the `graph` feature is off by default, the transform executor is gone, and the module does not build at this revision. Graph compilation is **deferred** with the graph follow-on |
| Actor `?name=default` slots wired to a graph's `SetParameter` mailbox | `?name` slots still type-check and lower as dynamic built-in parameters; the graph-side mailbox wiring is not part of this branch |

## The language in one screen

A program is a list of definitions ending in `;`. One must be named `main`.

```haskell
gain x = x * 0.5;            // a one-argument function (juxtaposed args — no parens)
main = 1.0 + gain 1.0;       // a pure value: Float(1.5)
```

### Application syntax

Function calls use **bracket-free juxtaposition** — space-separated arguments,
no parentheses:

```haskell
f g x = g (g x);             // juxtaposed
main = f (\x -> x * 2.0) 3.0;
```

Parenthesized form `name(arg, ...)` is also supported (for compatibility),
but juxtaposed is canonical.

### Records

Records are written with **record literals** `{ key: val }`:

```haskell
data Point = { x: Float, y: Float };
p = Point { x: 2.0, y: 3.0 };
main = p.x;                  // field projection -> Float(2.0)
```

Records can be nested, and a **single-field record** can be constructed
newtype-style by passing the field value directly (`Box 3.0` ≡
`Box { value: 3.0 }`).

### Primitives

| Syntax | Meaning |
|---|---|
| `3`, `3.5` | int / float literal |
| `+ - * / %` | arithmetic |
| `== /= < <= > >=` | comparison |
| `&& \|\| not` | boolean logic |
| `sin cos tan sqrt exp ln tanh abs` | math builtins |
| `min max` | selection |
| `show_float` / `show_int` / `show_bool` | scalar to `String` |
| `concat_string`, `length` | string/list helpers |

**Complex arithmetic** (builtins, always available):

| Syntax | Meaning |
|---|---|
| `complex(re, im)` | complex pair (re, im) |
| `conj x` | conjugate |
| `re x`, `im x` | real / imaginary part |
| `norm x` | magnitude: `√(re² + im²)` |
| `arg x` | phase: `atan2(im, re)` |
| `cmul a b` | complex multiply |
| `cadd a b` | complex add |

Complex values are pairs of values; the parallel combinator `,` builds them and
`.first`/`.second` take them apart:

```haskell
z = (complex 3.0 4.0, complex 2.0 0.0);
main = norm (cmul z.first z.second);   // 6.0
```

### Combinators (block-diagram algebra)

The signal-block algebra is still part of the language: `A : B` (sequential),
`A , B` (parallel), `A <: B` (split), `A :> B` (merge), `A ~ B` (feedback tap,
one-tick delay), and `A @ n` (integer delay). Precedence, loosest → tightest:
`~` < `:` < `:>` < `<:` < `,` < `+ -` < `* / %` < `@` < unary `-` < atoms.
These construct signal blocks; executing them requires a signal-transform host,
which this branch does not provide.

## Built-in functions

rill-lang supports calling stateful DSP/model built-ins from
`rill-core-dsp`/`rill-core-model`/`rill-fft` via a `ForeignRegistry` handed to
`test_eval` (or to the compile-only `compile_with_ffi`).

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

Built-ins use **unified argument syntax**: signals are first-class arguments
passed by juxtaposition (e.g. `lowpass _ 1000.0 0.7`). Some built-ins accept
variadic signal inputs — `mixer` takes any number of signals followed by a
record:

```haskell
main = mixer _ ch2 ch3 ch4 { buses: 4, master_vol: 1.0 };
main = dry_wet _ wet { mix: 0.7 };
main = eq_parametric _ { bands: [{ freq: 500.0, q: 2.0, gain_db: -3.0 }] };
```

All built-ins are whole-buffer `BlockBuiltin`s. Per-sample state (filters,
integrators) lives inside the built-in. Bindings and registries live in
`rill-adrift` (`lang_builtins::full_registry`), with per-crate
`register_lang_builtins()` functions for selective registration. For declaring
Rust-implemented built-ins in the language, see *FFI (foreign functions)* below.

### Dynamic parameters (`?name=default`)

`?name=default` creates a named parameter slot for a built-in's scalar argument:

```haskell
main = lowpass _ ?cutoff=1000.0 0.7;
```

Names are stable and may be namespaced by a `where` block with dot notation
(`?osc.freq=440.0` → `"osc.freq"`). `param("name", default)` is the older inline
form for the same idea; both coexist. Parameter slots are addressable
programmatically through `RillProgram::params_meta()` / `set_param()`; the
graph-side `SetParameter` mailbox that consumed these names is not part of this
branch.

## FFI (foreign functions)

The **FFI layer** is the language-side contract for calling Rust-implemented
built-ins: a built-in algorithm's signature is written **in rill-lang**, and the
Rust implementation is matched by name through a factory registry at compile
time. **The language owns the signature; Rust owns the implementation.**

A foreign built-in is declared with the `foreign fn name : TypeExpr;` syntax.
The signature is a curried arrow (`a -> b -> r`). Signal channels are typed
`FixedBuffer f32`; scalar parameters use the scalar value types `Float` /
`Int` / `Bool` / `String`; a variadic signal tail is `List (FixedBuffer f32)`:

```haskell
foreign fn biquad : FixedBuffer f32 -> Float -> Float -> Float -> FixedBuffer f32;
foreign fn add    : FixedBuffer f32 -> FixedBuffer f32 -> FixedBuffer f32;
foreign fn sum    : FixedBuffer f32 -> List (FixedBuffer f32) -> FixedBuffer f32;
```

Parameters occupy positional argument slots in declaration order — the signal
channel at slot 0, the scalar at slot 1:

```haskell
foreign fn gain : FixedBuffer f32 -> Float -> FixedBuffer f32;
main = gain _ 0.5;
```

The result type is one or more `FixedBuffer` channels (`FixedBuffer f32` → 1
out; `(FixedBuffer f32, FixedBuffer f32)` → 2 outs). A `List (FixedBuffer f32)`
variadic parameter must be **last** — a mid-signature variadic does not resolve
as a foreign built-in.

### The signal-track types

`SIGNAL_PRELUDE` ships the buffer family and the `Buffer` typeclass:

```haskell
typeclass Buffer b where { }
instance Buffer (FixedBuffer a) where { }
instance Buffer (Tape f32) where { }
```

- **`FixedBuffer a`** — the signal-channel type. A `foreign fn` signal parameter
  is strictly a `FixedBuffer[BUF]` at the runtime boundary (i.e. the const-generic
  `FixedBuffer<f32, BUF>`, block-sized, no heap on the RT path).
- **`Tape a`** — a shared-buffer handle (a tape loop). A `Buffer` family member
  like `FixedBuffer`; its runtime representation is an index into the program's
  shared tape cells (see *Tape loops* below).
- **`Buffer`** — a typeclass over buffer types; the home of buffer math in later
  stages.

### The FFI registry

Rust implementations are registered by name on `rill_lang::ffi::ForeignRegistry<T>`:

```rust
use rill_lang::ffi::ForeignRegistry;

let mut ffi = ForeignRegistry::<f32>::new();
ffi.register_block("gain", |params: &[f64], sample_rate: f32| {
    // -> Box<dyn BlockBuiltin<f32>>  (a 1→1 block builtin)
});
ffi.register_multichannel_block("add", |ins: usize, params: &[f64], sample_rate: f32| {
    // -> Box<dyn MultichannelBlockBuiltin<f32>>  (an N→M block builtin)
});
let prog = rill_lang::test_eval::<f32, 256>("", "gain _ 0.5", &ffi, 44100.0).unwrap();
```

`register_block` registers a single-channel (1→1) built-in;
`register_multichannel_block` an N→M one — the runtime dispatches by the
`BuiltinInst` variant. `test_eval` compiles against the registry and runs the
expression once, returning `RillProgram<T, BUF>`. A foreign call whose name is
not registered fails at `RillProgram::build` with `foreign built-in '...' is
not registered`.

### The builtin catalog

The builtin catalog is **part of the language**: every migrated built-in is
declared with `foreign fn` in the auto-registered catalog
(`BUILTIN_FOREIGN_DECLS` in `rill-lang/src/types/ty.rs`), which populates
`TypeEnv::foreign_sigs`. Programs resolve these names without writing a
`foreign fn` per program. The DSP crates (`rill-core-dsp`,
`rill-digital-effects`, `rill-core-model`, `rill-router`) register their Rust
**factories** on `ForeignRegistry` (see `register_foreign_*` in
`rill-lang/src/register.rs` and the crates' `lang` modules).

### Reserved names

- **`foreign`** — a lexer keyword, reserved for `foreign fn` declarations.
- **`FixedBuffer`** — the built-in signal-channel type constructor (arity 1).
- **`Buffer`** — the built-in typeclass over buffer types.
- **`Tape`** — the built-in shared-buffer handle constructor (arity 1).

### Tape loops

A tape loop is created with the `tape_loop` foreign constructor
(`Int -> Tape f32`) and shared by the tape head builtins `write_head` /
`read_head` (registered by `rill-sampler`). NAMED bindings share one buffer:

```haskell
tape = tape_loop 1024;
main = (write_head _ _ tape 0.5 0.3) , read_head tape 0.1;
```

Each inline `tape_loop <capacity>` call allocates a **fresh** tape; to share a
tape between a write head and its read heads, bind it to a name as above. The
legacy `TapeLoop <capacity>` declaration spelling is still accepted.

## Type system

Types are inferred with a Hindley-Milner core: scalar types (`int`, `float`,
type variables) are unified with an occurs check and let-generalized for named
functions; wire arities are synthesized bottom-up and checked against the
combinator algebra. Any mismatch is a compile error with a source span, and code
generation is blocked — so an ill-formed program never reaches the runtime.

## Serialization

With the `serialization` feature, [`RillLangDef`] carries a program as its
**source string** (the canonical, human-editable form) and [`compile_def`] turns
it back into a runnable program:

```rust,no_run
use rill_lang::{RillLangDef, compile_def};

let def = RillLangDef::new("gain", "main = return ();");
let prog = compile_def::<f32>(&def).unwrap();
```

The `serialization` feature no longer implies `graph`: `RillLangDef` and
`compile_def` are the source-level contract; graph serialization (`GraphDef`) is
gated behind `graph` + `serialization` and is not built by the workspace gates.

## Debug infrastructure (`debug` feature)

When the `debug` Cargo feature is enabled, the IR gains a `ProbePoint`
instruction for signal-level diagnostics:

- **`ProbePoint { id, src, dst }`** — pass-through IR instruction that copies a
  register value and simultaneously captures it to a lock-free probe slot
- **`ProbeSlot`** — atomic flags (`enabled`, `break_flag`, `paused_flag`) plus an
  SPSC queue for frame transport to a non-RT collector thread
- **`DebugControl`** — shared atomics (`global_pause`, `global_resume`) for
  pause/resume execution control without syscalls

Probe data flows through `rill-telemetry`'s `CollectorThread` and can be
inspected via `rill-analyzer`. Zero overhead when the feature is disabled.

## First-class data (`data`/`type`/`newtype`/`typeclass`)

Beyond signals, a program can carry **values** — arena-paced, typed, and
processed in the value-track phase:

```haskell
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
| `Bool`, `String`, `()` | `*` | scalar value types (value track only) |
| `List a`, `Set a` | `* → *` | ordered list / unordered set (**open** — grow up to the pool) |
| `Map k v` | `* → * → *` | key→value map (**open**) |
| `Maybe a` | `* → *` | optional `a` (`Just a` / `Nothing`) |
| `Pair a b` | `* → * → *` | pair (`{ first, second }`) |
| `Either a b` | `* → * → *` | sum (`Left a` / `Right b`) |
| `IO a`, `ST s a` | `* → *`, `* → * → *` | state-threading monads (`World` / isolated state) |
| `InputStream a`, `OutputStream a` | `* → *` | io-streams handles |
| `STRef s a` | `* → * → *` | mutable cell inside an `ST` region |

### First-class collections

`List`/`Map`/`Set` are first-class arena containers with Haskell-style ops.
Collections are **open** — there is no capacity in the type, and `cons`/
`insert` grow freely up to the pre-allocated pool budget (exceeding it in the
default RT mode is a detectable no-op, not a per-container error).
`map`/`fold`/`filter` take the function **first**; `cons` **prepends**
(Haskell `x : xs`); empty containers are `list` / `empty_map` / `empty_set`;
`insert` is overloaded by arity (Map 3-arg, Set 2-arg); `not` is a prefix
builtin.

```haskell
xs  = [1.0, 2.0, 3.0];            // List Float
ys  = cons 10.0 (list);           // prepend; the list grows
h   = head xs;                    // Maybe Float: Just 1.0 / Nothing
n   = length xs;                  // Int
z   = map (\x -> x * 2.0) xs;   // function first
s   = fold (\a b -> a + b) 0.0 xs;   // Float
f   = filter (\x -> x > 1.0) xs;     // List Float

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

### Arrays and `STArray` (Data.Array surface)

Immutable arrays are first-class values: `Array i e` indexed by `Int` or
`Pair Int Int` (a matrix is `Array (Pair Int Int) Float`). Use `listArray` for
sequential construction, or `array` with explicit associations. Index with the
infix `!`; note that `!` is also the bang marker in patterns — `f !x` forces
`x` to WHNF, while `a ! i` indexes an array.

The core vocabulary includes `bounds`, `elems`, `indices`, `assocs`, `update`,
`accum`, and `ixmap` for reindexing:

```haskell
b = Pair { first: 0, second: 2 };
a = listArray b [10.0, 20.0, 30.0];
main = a ! 1;                         // Float(20.0)
```

Mutable arrays live in `ST`: `STArray s i e` can be built and mutated inside a
region, then frozen to `Array i e` with `runSTArray`. The region phantom `s`
cannot escape — a use-after-free is a compile-time error.

```haskell
main = let a = runSTArray (\s -> do {
    arr <- newArray (Pair { first: 0, second: 1 }) 0.0;
    writeArray arr 1 3.0;
    return arr;
}) in a ! 1;
```

### Higher-kinded types (HKT)

`data` can take type parameters and `typeclass` can range over a type
**constructor** (kind `* → *` / `* → * → *`). Resolution is compile-time
**inline** — `fmap` over a `List` compiles directly to the `map` builtin, with
zero runtime dispatch:

```haskell
data Box a = { value: a };
typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
instance Functor List where { fmap g xs = map g xs; }
instance Functor Maybe where {
    fmap g m = case m of { Nothing -> Nothing; Just x -> Just (g x); };
}
main = length (fmap (\x -> x * 2.0) [1.0, 2.0, 3.0]);   // Int(3)
```

Kind arity is inferred from method signatures and checked — `instance Functor
Pair` (Pair has arity 2) is a kind error.

### Builtin category typeclasses (`Functor`/`Applicative`/`Monad`/`Monoid`/`Arrow`)

The category-theory classes ship **built in** — declared in a language prelude
registered at compile time, so a program never redeclares them but may add its
own instances. Methods resolve at compile time by **inline** lowering (zero
runtime dispatch), and `instance Monad T` **auto-derives** `Applicative T` and
`Functor T` (explicit instances always win):

```haskell
typeclass Functor f     where { fmap:  (a -> b) -> f a -> f b; }
typeclass Applicative f where { pure:  a -> f a; ap: f (a -> b) -> f a -> f b; }
typeclass Monad m       where { return: a -> m a; bind: m a -> (a -> m b) -> m b; }
typeclass Monoid m      where { mempty: m; mappend: m -> m -> m; }
```

Builtin instances: `Functor`/`Monad` for `List`, `Maybe`, `Either a`, `IO`, `ST
s`; `Monoid` for `List` (`append_list`), `String` (`concat_string`), `Float`,
`Int`. The partial-application forms `IO`, `ST s` are the monads the runner and
`runST` execute.

**Monad/IO interfaces:**

```haskell
return :: a -> m a            // via Monad
bind   :: m a -> (a -> m b) -> m b
pure   :: a -> f a            // via Applicative
```

`runST` has type `runST :: (forall s. ST s a) -> a` — it opens a fresh, isolated
state region for its argument; an `STRef` or `ST s` result that would escape the
region is a compile-time error. Inside the region, `newSTRef`, `readSTRef`,
`writeSTRef` and `modifySTRef` operate on `STRef s a` cells.

**Result-directed dispatch (`mempty`).** A nullary method has no selector
argument, so it resolves by its *expected result type* — `mappend xs mempty`
resolves `mempty` by the type of `xs`:

```haskell
main = length (mappend [1.0, 2.0] mempty);   // List Float, mempty = list -> Int(2)
main = mappend mempty 3.5;                    // Float, mempty = 0.0 -> Float(3.5)
```

Using `mempty` where its result type is unknown is a compile error.

**`do`-notation** desugars to nested `bind` (Haskell `<-`):

```haskell
mx = Just 1.0;
my = Just 2.0;
main = case (do { x <- mx; y <- my; pure (x + y); }) of { Nothing -> 0.0; Just z -> z; };
// == bind mx (\x -> bind my (\y -> pure (x + y))) -> Just 3.0
```

`do { x <- mx; let y = e; stmt; expr; }` supports `<-` binds, `let` bindings,
and bare monadic statements (each desugars to `bind`); the final statement is
the block's result. Note that `a < -b` is a comparison followed by negation —
parenthesize: `a < (-b)`. In an IO program the final statement is `return ()`;
the monad is fixed by the preceding effects, so a bare `main = return ()` needs
the `IO` type to be known from context.

#### `Arrow` and `Kleisli`

The prelude also declares **`Kleisli`**, the free category over a monad (an
arrow `a -> m b` inside the container `m`), and **`Arrow`**, the category of
container morphisms:

```haskell
data Kleisli m a b = { unKleisli: a -> m b };

typeclass Arrow a where {
    arr:     (b -> c) -> a b c;
    first:   a b c -> a (Pair b d) (Pair c d);
    compose: a b c -> a c d -> a b d;
    second:  a b c -> a (Pair d b) (Pair d c) =
        k (compose (compose (arr (\p -> Pair { first: p.second, second: p.first })) (first k)) (arr (\p -> Pair { first: p.second, second: p.first })));
    both:    a b c -> a d e -> a (Pair b d) (Pair c e) =
        f g (compose (first f) (second g));
    fan:     a b c -> a b d -> a b (Pair c d) =
        f g (compose (arr (\x -> Pair { first: x, second: x })) (both f g));
}

instance (Monad m) => Arrow (Kleisli m) where {
    arr f        = Kleisli (\x -> return (f x));
    first k      = Kleisli (\p -> bind (k.unKleisli p.first) (\z -> return (Pair { first: z, second: p.second })));
    compose k1 k2 = Kleisli (\x -> bind (k1.unKleisli x) (\y -> k2.unKleisli y));
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

```haskell
apply k x = let u = k.unKleisli in u x;
main = case (apply (compose (arr (\x -> x + 1.0)) (arr (\y -> y * 2.0))) 3.0) of {
    Just v -> v; Nothing -> 0.0;
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

The `,` combinator doubles as a **channel tuple** — it unifies on the track of
its operands. `value,value` builds a `Pair { first, second }`; a **mixed**
`value , signal` is a compile error (one channel cannot live on both tracks):

```haskell
main = (1.0, 2.0);        // value,value -> Pair { first: 1.0, second: 2.0 }
main = (1.0, 2.0).first;  // -> Float(1.0)
```

In **type position**, the same syntax desugars to the pair type: `(b, d)` is
sugar for `Pair b d` — that is how the Arrow methods above spell their pair
arguments (`a (Pair b d) (Pair c d)` ≡ `a (b, d) (c, d)`).

A **field projection is a first-class function value** — a projected closure
can be applied directly, parenthesized or bare:

```haskell
data Box = { f: Float -> Float };
b = Box { f: \x -> x * 2.0 };
main = (b.f) 3.0;         // parenthesized projection applied: Float(6.0)
main = b.f 3.0;           // bare projection applied too
```

Reserved method/builtin names from the prelude: `fmap`, `pure`, `ap`,
`return`, `bind`, `mempty`, `mappend`, `concat_map`, `append_list`,
`concat_string`, plus the Arrow methods and the Kleisli data type: `arr`,
`first`, `compose`, `second`, `both`, `fan`, `Kleisli`.

## First-class functions and closures

Functions are first-class values: a **lambda literal** `\p -> body` compiles
to a `Value::Callable` (a by-value environment snapshot plus a compiled body
fragment; eager, with no memo cell), and `ValueCallFunc` performs real runtime
dispatch:

```haskell
adder = \n -> \x -> x + n;   // a function returning a closure
add2  = adder 2.0;               // partial application (currying)
main  = add2 3.0;                // -> Float(5.0)

twice  = \f x -> f (f x);      // a higher-order combinator
double = \x -> x * 2.0;
main   = twice double 3.0;       // -> Float(12.0)
```

- Lambda parameters can themselves be functions (HOF), flow through records
  and projections, and are typed structurally (`ValueTy::Func(arg_tys, ret_tys)`).
- **Recursion is forbidden** in the value track: a definition that transitively
  calls itself is rejected at compile time. Because calls cannot recur, the
  runtime call depth is a static bound and the interpreter pre-allocates the
  dispatch register frames — `ValueCallFunc` performs **no heap allocation** on
  the RT path. (Recursive *stream* helpers are the exception: the runner
  compiles in `Complete` mode, which admits guarded recursion.)

## Branching and pattern matching

`if cond then a else b` and `case` are expressions on the value track. In an
`IO` program they may choose between effects as well as values:

```haskell
main = do {
    n <- read stdin;
    if (length n) > 0 then write (Just n) stdout else write (Just "empty") stdout;
    return ()
};
```

`case` supports constructor, literal (`0`, `1.5`, `true`, `"s"`), wildcard
`_`, variable, and nested patterns plus Haskell-style guards. An uppercase
initial is a constructor, a lowercase initial is a binding. Matches must be
exhaustive (every constructor covered, or a `_`/variable arm); a guarded arm
whose guard fails falls through to the next arm, and a residual non-match is a
runtime `ProcessError::Processing`.

## Status

The IO/streams core is in place: `main : (String…) -> IO ()` executed by
`ProgramRunner`, a Haskell-faithful `ST` with isolated state, io-streams-style
memory/stdio streams, and the retained pure-value tick. The value track ships
first-class collections (`List`/`Map`/`Set`, `Maybe`/`Pair`/`Either`,
`Bool`/`String`), higher-kinded types, and `if`/`case`.

Deferred to follow-on work: the Cranelift `jit` feature, trampolining the `bind`
chain (removing the stack-depth limit), signal-transform execution over streams
(the future audio plan), graph compilation repair, migrating the remaining rill
DSP primitives onto the FFI layer (the `foreign fn` mechanism itself is live —
see *FFI*), a SIMD-aware IR, runtime typeclass dispatch, user-written `Eq`/`Ord`
instances and hash-based containers, and `strict`/`complete` compiler modes.

## License

Apache-2.0. See the workspace `LICENSE.md`.

[`rill_core::Algorithm`]: https://docs.rs/rill-core
[`RillLangDef`]: https://docs.rs/rill-lang
[`compile_def`]: https://docs.rs/rill-lang
