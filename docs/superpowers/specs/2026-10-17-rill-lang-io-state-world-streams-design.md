# rill-lang: Real IO (State + RealWorld), Full Haskell ST, io-streams-style Streams (memory + stdio) — Design

> **Stage:** C refinement — re-scoped. Supersedes the passive/active + backend +
> hot-loop framing of the earlier revision of this same document; that framing
> (audio backends, per-block real-time processing, threads/actors) is deferred
> to follow-on plans (§6).
> **Prerequisites:** Stage A (lazy value track), the `Monad`/`IO`/`ST` typeclass
> machinery, the rank-2 `runST` special case in inference, the `STD_PRELUDE`
> registration point.
> **Scope:** The core execution foundation, nothing more:
> 1. **Real IO as State + RealWorld** — `IO a` threads a linear `World` token
>    (`State# RealWorld` analog), ordering effects by data dependency exactly as
>    GHC does; the runtime realizes `realWorld` (stdio handles + the memory-
>    stream table).
> 2. **Full Haskell ST with isolated state** — `ST s a` threads an opaque,
>    isolated state token; `runST` supplies a **fresh** state per region; the
>    rank-2 `forall s` prevents mutable state from escaping; `STRef` provides
>    mutable cells inside a region.
> 3. **io-streams-style streams** — `InputStream a` / `OutputStream a`, generic
>    over the element type `a`, with the io-streams operation surface, backed
>    only by **memory** (`fromList`/`toList`/`makeInputStream`/
>    `makeOutputStream`) and **stdio/stderr** (`stdin`/`stdout`/`stderr` over
>    `String`).
> 4. **Program form and full replacement** — `main : (String…) -> IO ()`,
>    executed to completion by a new `ProgramRunner` against `realWorld`; the old
>    signal-transform execution system (`readInput`/`writeOutput`, `ProgramEngine`,
>    the `Algorithm`-transform `RillProgram`, buffer-λ `main`) is removed and all
>    programs migrate.

## North-star requirement (unchanged, foundation edition)

**A `lang_chiptune`-class program must eventually be expressible entirely in
rill-lang** — backends, threads, and actors created *inside* the language, not
behind it. This plan does not deliver that; it delivers the **execution core**
that program is written on:

- A **real IO monad** (State + RealWorld) that orders effects and can realize
  arbitrary runtime I/O (here: memory and stdio; audio later).
- A **real, sound ST** (full Haskell semantics) that the language and the future
  real-time layers can rely on for isolated mutable state.
- **io-streams-style streams** as ordinary values — the one I/O contract that
  memory, stdio, and (later) audio backends all share.

**The transition this plan enacts:** `ProgramRunner` stops being a *signal
processor* (a fixed `&[&[f32]] → &mut [&mut [f32]]` transform) and becomes the
**executor of a fully Turing-complete program**: it runs `main : (String…) -> IO
()` to completion. Signal processing is just one domain the program can express
later; the runner's identity is *executing the program*, not transforming
signals. Backward compatibility with the old forms is deliberately **not
required** — existing programs migrate to the new form (no transition period).

---

## 0. Executive summary

The design is deliberately small. Three faithful-to-Haskell mechanisms, nothing
more:

- **`IO a = World -> (a, World)`** — the reader-over-State shape GHC uses
  (`IO a ≅ State# RealWorld -> (# State# RealWorld, a #)`). The linear `World`
  token orders effects; because each bind threads the *same* token, reordering
  is inexpressible. The runtime provides `realWorld` (the actual stdio handles
  and memory-stream table).
- **`ST s a = STState -> (a, STState)`** — the full Haskell ST state monad:
  state is threaded through `bind`, `runST` opens a **fresh, isolated** state
  per region, and the rank-2 `forall s` (already special-cased in inference)
  makes mutable state inexpressible outside the region. `STRef s a` gives
  mutable cells inside the region.
- **Streams are io-streams values.** `InputStream a`/`OutputStream a` with
  `read :: InputStream a -> IO (Maybe a)` and `write :: Maybe a -> OutputStream
  a -> IO ()` (`Nothing` = EOF/close), plus the io-streams operation surface.
  The concrete backends in scope are **memory** and **stdio/stderr**; the stream
  element is a plain, generic `a` — there is no audio block, no `List Buffer`,
  no backpressure, no active/passive distinction in this plan.

The program is `main : (String…) -> IO ()`. The `ProgramRunner` executes it to
completion. The old execution system is removed.

---

## 1. Intent and why

### 1.1 A sound foundation before a wide surface

The earlier revision of this design reached for the full audio/real-time surface
(backends, active/passive streams, master-slave `runStream`, per-block hot
loops) before the underlying semantics were real. That made everything
conditional on machinery that did not exist yet. This revision inverts the
order: **the IO/ST semantics become real first**, and the stream layer is built
on them — small, correct, and testable — before any real-time concern is
touched.

### 1.2 GHC provides validated shapes (we take the shapes, not the dependencies)

- `IO a = State# RealWorld -> (# State# RealWorld, a #)` — the reader-over-
  State+World idea, with a linear opaque token ordering effects.
- `ST s a` with `runST :: (forall s. ST s a) -> a` — the canonical way to
  confine mutable state to a region; `STRef` for cells inside the region.
- `io-streams` — `InputStream a` (pull: `IO (Maybe a)`) and `OutputStream a`
  (push: `Maybe a -> IO ()`) as backend-independent values; `Nothing` = EOF /
  close; memory streams via `fromList`/`toList`; stream construction via
  `makeInputStream`/`makeOutputStream`; stdio via `stdin`/`stdout`/`stderr`.

### 1.3 "Real" is an interpreter-semantics change, not just types

Today `ST s a = { unST: a }` and `IO a = { unIO: a }` are pure record wrappers:
`bind` just unwraps and re-wraps a value, no state exists, `runST` is a
one-point rank-2 type check with no runtime isolation. Making them real means:

- the **declarations** become functions over opaque tokens
  (`STState -> Pair STState a`, `World -> Pair World a`);
- the **`Monad` instances** thread the token through `bind`;
- **`runST`** applies its computation to a fresh, isolated state token and
  discards the final one;
- **`STRef`** cells live in that isolated state;
- the **runner** applies `main` to `realWorld` and performs the effects the
  program's IO actions request (memory-stream and stdio operations).

---

## 2. Goals, non-goals, anti-goals

### Goals

1. **Real IO**: `IO a` threads a linear `World` token; effects are ordered by
   token data-dependency; `realWorld` is realized by the runner.
2. **Full Haskell ST**: `ST s a` threads isolated state; `runST` opens a fresh
   region; the rank-2 `forall s` prevents escape; `STRef` provides mutable
   cells (`newSTRef`/`readSTRef`/`writeSTRef`/`modifySTRef`).
3. **io-streams streams, generic over `a`**: `InputStream a`/`OutputStream a`;
   `read`/`write` (`Maybe`), `atEnd`, `unread`, `connect`, `fromList`, `toList`,
   `makeInputStream`, `makeOutputStream`; `stdin`/`stdout`/`stderr`.
4. **Memory and stdio backends only**: no audio, no `List Buffer` element, no
   backpressure, no active/passive distinction in this plan.
5. **`main : (String…) -> IO ()`**: String launch args via `set_args(&[String])`;
   the `ProgramRunner` executes `main`'s IO to completion against `realWorld`.
6. **Full replacement**: `readInput`/`writeOutput`, `ProgramEngine`, the
   signal-transform `RillProgram`, buffer-λ `main`, and the audio-scoped
   `Stream`/`asInput`/`asOutput`/`mkStream`/`BackendSpec`/`runStream` surface are
   removed; all programs migrate.
7. **Correct prelude placement**: ST, STRef, IO, and the concrete data-type
   instances (Functor/Monad/Monoid for `Maybe`/`List`, `Monoid Float`/`String`)
   move from `CATEGORY_PRELUDE` to `STD_PRELUDE` — they are monads and instances,
   not category theory. `CATEGORY_PRELUDE` keeps only the foundational
   category-theory machinery: the typeclass declarations
   (Functor/Applicative/Monad/Monoid/Arrow) and the Kleisli construction.
   `SIGNAL_PRELUDE` is untouched.

### Non-goals (this plan)

- Audio capture/playback, tape, file, console-stream backends.
- Real-time signal processing, per-block hot loops, RT-safety typing, audio
  element types (`List Buffer`), backpressure, active/passive streams.
- The buffer/region memory model (`Buffer` = reference to persistent region),
  backend-owned buffers, the global memory manager for program resources
  (TapeLoop).
- Threads, channels, actors, schedulers.
- OTP-style fault tolerance; modules/import; the graph feature's reconstruction.

### Anti-goals (a failure even if it technically works)

- Keeping `readInput`/`writeOutput` as the primary I/O, or keeping the old
  signal-transform execution system alongside the new one.
- A stream model that is audio-shaped (block elements, backpressure, master-
  slave binding) before memory/stdio io-streams work.
- ST that is only a type-level phantom with no runtime state (the current
  state).
- Re-introducing a signal-path/data-path separation in the I/O model.

---

## 3. Constraints (binding)

- **No backward compatibility (directive).** Buffer-λ `main`, `readInput`/
  `writeOutput` and their indexed variants, and the signal-transform
  `ProgramRunner`/`ProgramEngine` framing are removed. Existing programs and
  tests migrate to `main : (String…) -> IO ()` with stream I/O. No sugar layer
  is kept. No transition period.
- **ProgramRunner is an executor of a Turing-complete program**, domain-
  agnostic; it takes **no input/output buffers or backend handles from
  outside**. Streams (memory, stdio) are the program's I/O, created inside the
  program.
- **Prelude placement:** `CATEGORY_PRELUDE` — the foundational category-theory
  machinery only: the typeclass declarations (Functor/Applicative/Monad/Monoid/
  Arrow) and the Kleisli construction. `STD_PRELUDE` — the common standard-
  library layer: the concrete data types and their instances moved here from
  `CATEGORY_PRELUDE` (**ST + STRef + IO** and the `Maybe`/`List`/
  `Float`/`String` instances — they are monads and instances, not category
  theory), the stream types and operations, and stdio. `SIGNAL_PRELUDE` —
  untouched.
- **No new third-party dependencies** without explicit confirmation. All
  mechanisms come from the existing value track, the `Monad` machinery, the
  rank-2 `runST` special case, and the `STD_PRELUDE` registration point.
- `rill-lang` denies `unsafe_code`, warns on `missing_docs`. Docs in English,
  `max_width=100`.
- **RT rules** (AGENTS.md) apply to the *future* audio hot loop, not to this
  plan — this plan's program runs to completion on the program thread; there is
  no RT path yet.
- **Workspace baseline:** verification runs against `cargo test -p rill-lang
  --features dsp`; **graph compilation is not required** — the `graph` feature
  and its known-broken `reconstruct.rs` baseline are out of scope and may stop
  compiling; `rill-adrift` drops its `graph` feature dependency on `rill-lang`.
  `rill-adrift` **migrates** with this plan (its program strings and the call
  sites that used `ProgramRunner::apply`/`ProgramEngine` are updated to the new
  entry points).

---

## 4. The "how" decided

### 4.1 IO as State + RealWorld

```
data World;                              -- primitive opaque token (GHC State# RealWorld analog)
data IO a = { unIO: World -> Pair World a }   -- State step over the token (orders effects)
```

- `World` has no constructors and cannot be observed or duplicated — only the
  runtime creates `realWorld` and discards the final token (existing Stage C
  decision). Its runtime value is an opaque handle to the runner's mutable
  runtime state (the memory-stream table and the stdio handles); the token is
  threaded for ordering, the handle is stable — exactly GHC's `RealWorld`.
- **Opaque primitive types:** `World` (and `STState`/`STRef s a` in §4.2) are a
  new **opaque primitive** kind: the program can *name* them in signatures but
  cannot construct, project, or observe them — the prelude declares them, only
  runtime builtins create their values. The compiler has no constructor-less
  form today; this plan introduces the facility (mechanism in §6).
- **Effect ordering (decision):** evaluation mode is per-program — **Strict**
  (eager, no thunks, recursion banned, `!` inert) or **Complete** (lazy /
  call-by-need, recursion allowed). Inside Complete the value track is lazy by
  default; a **bang annotation** (`!`) is a *force of eager evaluation up to a
  set boundary*: it emits `ValueForce` at the annotated site, evaluating the
  suspended thunk to its head (WHNF) — not deeper. The linear `World` token
  orders effects by data dependency in both modes — in Complete the
  continuation forces the new token (call-by-need), in Strict evaluation is
  eager so dependency order holds trivially:
  `bind m f = IO { unIO: \w -> let p = m.unIO w in (f p.second).unIO p.first }`
  — the continuation needs the new token (`p.first`), so reordering is
  inexpressible in either case. There is **no compiler-enforced linear-type
  subsystem**; the token threading is the runtime ordering mechanism
  (unchanged from the earlier revision of this design).
- The `Monad IO` instance (in `STD_PRELUDE`) is re-expressed in this form;
  `return x = IO { unIO: \w -> Pair { first: w, second: x } }`.

### 4.2 Full Haskell ST with isolated state and STRef

```
data STState;                                   -- primitive opaque token (GHC State# s analog)
data ST s a = { unST: STState -> Pair STState a }   -- state-threading monad
data STRef s a;                                 -- opaque handle to a cell in the region's state
runST :: (forall s. ST s a) -> a                -- fresh, isolated region; discard final state
newSTRef     :: a -> ST s (STRef s a)
readSTRef    :: STRef s a -> ST s a
writeSTRef   :: STRef s a -> a -> ST s ()
modifySTRef  :: STRef s a -> (a -> a) -> ST s ()
```

- **State threading:** the `Monad (ST s)` instance threads the token:
  `bind m f = ST { unST: \s -> let p = m.unST s in (f p.second).unST p.first }`.
- **Isolation:** `runST` creates a **fresh** `STState` (a new cell table) per
  invocation, runs the computation against it, and discards the final state —
  two `runST` regions never share mutable state.
- **No escape:** the rank-2 `forall s` and the existing inference escape-check
  (result type must not mention `s`) make an `STRef` inexpressible outside its
  `runST` — already implemented for the phantom form, retained for the real one.
- **STRef:** a cell handle (`newSTRef` allocates, `readSTRef`/`writeSTRef`/
  `modifySTRef` operate). Cells live in the region's isolated cell table; a
  handle cannot outlive the region (escape-check). Like `World`, `STRef s a` is
  an opaque primitive type — nameable, never constructible in source (only
  `newSTRef` creates one).

### 4.3 Prelude placement

```
CATEGORY_PRELUDE  -- foundational category theory ONLY: the typeclass
                  -- declarations (Functor/Applicative/Monad/Monoid/Arrow) and
                  -- the Kleisli construction (Kleisli m a b + its Arrow
                  -- instance). No concrete data types and no instances over
                  -- them live here.
STD_PRELUDE       -- common standard library: the concrete data types and their
                  -- instances MOVED here from CATEGORY_PRELUDE — ST + STRef +
                  -- IO (+ Monad instances), the Maybe/List instances
                  -- (Functor/Monad/Monoid; the type constructors are builtin
                  -- machinery, not prelude text), Monoid Float/String;
                  -- InputStream a / OutputStream a + the io-streams operations;
                  -- stdin / stdout / stderr.
SIGNAL_PRELUDE    -- untouched: Buffer typeclass, FixedBuffer, Tape.
```

The `runST` builtin signature and its rank-2 inference special case stay in the
type-system machinery (they are not prelude text).

### 4.4 Streams are io-streams values (memory + stdio)

```
data InputStream  a;    -- opaque handle (created by IO effects)
data OutputStream a;    -- opaque handle (created by IO effects)

read            :: InputStream a -> IO (Maybe a)        -- Nothing = EOF/closed
write           :: Maybe a -> OutputStream a -> IO ()   -- write Nothing closes
atEnd           :: InputStream a -> IO Bool
unread          :: a -> InputStream a -> IO ()
connect         :: InputStream a -> OutputStream a -> IO ()
fromList        :: List a -> IO (InputStream a)         -- memory
toList          :: InputStream a -> IO (List a)         -- memory (consumer)
makeInputStream :: IO (Maybe a) -> IO (InputStream a)   -- memory (source)
makeOutputStream:: (Maybe a -> IO ()) -> IO (OutputStream a)  -- memory (sink)
stdin           :: InputStream String                   -- stdio
stdout          :: OutputStream String                  -- stdio
stderr          :: OutputStream String                  -- stdio
```

- **Faithful to io-streams.** The operation surface and semantics follow
  Haskell's io-streams exactly: `read` pulls and returns `Nothing` at EOF;
  `write Nothing` closes the stream; `atEnd` reports EOF without consuming;
  `unread` pushes a value back; `connect` pipes an input into an output.
- **Generic element.** The element type `a` is a plain generic type. There is
  no audio block, no `List Buffer` element, no `ST (List Buffer)` wrapping, no
  backpressure, no active/passive distinction — all of that is deferred to the
  audio plan.
- **Streams are values, not global instructions.** `read`/`write`/… are
  operations over stream values; streams are created inside the program
  (`fromList`, `makeInputStream`, …) and realized against the runner's
  runtime state. `InputStream`/`OutputStream` are opaque handles — the payload
  is a runtime stream-table index, invisible to the program.
- **Backends in scope:** memory (`fromList`/`toList`/`makeInputStream`/
  `makeOutputStream`) and stdio/stderr (`stdin`/`stdout`/`stderr` over `String`
  — rill has no `ByteString`; line-oriented text). The stdio console is the
  runner's realized `realWorld`; tests inject a capture console.

### 4.5 Program form and the new `ProgramRunner`

- The program is `main : (String…) -> IO ()` — String launch args, no
  buffers, no buffer-λ. Args are bound at launch via `set_args(&[String])`;
  defaults are empty strings. They are **not** control params (`?name`/
  `SetParameter` remains for dynamic builtin params, unchanged).
- The new `ProgramRunner` (in `program_runner.rs`) is the executor:
  ```rust
  impl ProgramRunner<f32, BUF> {
      pub fn set_args(&mut self, args: &[String]);      // bind main's String λ-params
      pub fn run(&mut self) -> Result<(), Error>;       // run main's IO to completion vs realWorld
      pub fn stdout(&self) -> &[String];                // captured stdout (test access)
      pub fn stderr(&self) -> &[String];                // captured stderr (test access)
  }
  ```
  It takes **no input/output buffers or backend handles from outside**. It
  realizes `realWorld` (the memory-stream table + the stdio console), binds the
  String args, applies `main`'s `IO` to `realWorld`, and performs the effects
  the program requests until `main` completes. There is **no per-tick loop, no
  hot loop, no warm/hot split** in this plan — the program runs to completion.
- The `ProgramRunner` owns the runtime state that `World` realizes; `stdin`/
  `stdout`/`stderr` are wired to the runner's console (real stdio by default,
  injectable capture console for tests).
- **Runner evaluation mode:** the runner compiles and runs `main : IO ()` in
  **Complete mode** (`{-# MODE complete #-}` is applied when the source has no
  pragma). Recursive stream loops (a self-referential `makeOutputStream`
  handle) and recursive helpers are rejected by Strict's `check_recursion`, so
  Complete is required — and it is the GHC-faithful lazy model. Bang
  annotations (`!`) remain available inside Complete for per-site eager forcing
  to a boundary (§4.1).

### 4.6 Full replacement: removal and migration

- **Removed:** the `readInput`/`writeOutput` builtins and their lowering; the
  `ProgramEngine` signal wrapper; the signal-transform `Algorithm`/
  `MultichannelAlgorithm` impls of `RillProgram`; the program-fixed signal
  `num_inputs`/`num_outputs`; buffer-λ `main`; and the audio-scoped surface from
  the earlier revision of this design (`Stream a`, `asInput`/`asOutput`,
  `mkStream`, `BackendSpec`, the master-slave `runStream` handler, the stream
  registry / `stream_backend.rs`).
- **Migration rule:** an old signal program's *computation* is preserved; its
  I/O boundary becomes memory-stream operations, because streams are generic.
  A 1→1 processor
  `main = do { writeOutput (map f (readInput)); return () }`
  becomes a program that builds its blocks with `fromList`, processes them, and
  rounds them back with `toList`:
  `main = do { xs <- fromList (map f (fromList [0.5, 1.0])); _ <- toList xs; return () }`
  (concretely: create a memory stream of blocks, read/process/write). Generator
  programs (`writeOutput [sine …]`) become `fromList (map …)`; control params
  (`?name`) stay as-is. The signal ARITY semantics (per-block processing) are
  dropped with the signal-transform engine — the deferred audio plan restores
  block processing over the stream contract.
- **Retained:** the mono-kernel value-track interpreter and its IR; the
  `SIGNAL_PRELUDE` and the DSP/buffer builtins (pure functions still work);
  `?name`/`SetParameter` for builtin dynamic params.

---

## 5. Relationship to deferred work

- **Audio/real-time plan (I/O-2+):** backends created via `mkStream`, `List
  Buffer` block elements, per-tick hot loops, RT-safety typing, passive/active
  streams, the buffer/region memory model and backend-owned buffers, and the
  global memory manager for program resources (TapeLoop). It builds on this
  plan's real IO/ST and stream contract.
- **Threads/actors:** `spawn`/`Channel`/`receive` in-language concurrency
  (deferred; uses real IO and, later, streams).
- **Graph reconstruction:** deferred until the I/O model is stable (unchanged).

---

## 6. Left to the builder (decisions the implementer/plan may settle)

- The exact runtime representation of the `World`/`STState` tokens and of the
  stream-table/cell-table state the runner realizes (handle values, indexes, or
  distinct `Value` variants).
- Whether `InputStream`/`OutputStream` are empty-payload records (`{ }`, the
  `a` phantom) or carry a handle field; only the runtime constructs them.
- How the stdio console is injected for tests (a console source/sink on the
  runner vs a captured-by-default mode).
- `makeInputStream`/`makeOutputStream` closure representation in the value
  track (callable values over the token).
- `modifySTRef` via `readSTRef`+`writeSTRef` or as a fused primitive.
- **Mechanism of "real" IO/ST** (not user-visible): where the mutable runtime
  state the effects act on lives and how effect builtins reach it mid-
  interpretation — extend the program/runner struct vs a new runner-level
  executor owning program + runtime separately.
- **Registration of the polymorphic operation signatures** (`read`, `write`,
  `atEnd`, `unread`, `connect`, `fromList`, `toList`, `makeInputStream`,
  `makeOutputStream`, the `newSTRef` family): extend the rank-2 per-name
  special-case pattern (like `runST`), or generalize `builtin_sigs` to
  forall-quantified signatures, or express the ops as polymorphic `STD_PRELUDE`
  text functions over a small set of monomorphic primitives. The user-facing
  signatures are fixed in §4.2/§4.4; only the mechanism is open.
- **`runST`'s operational shape:** fresh isolated state per region, apply the
  ST closure, discard the final state — via new lowering/IR or an
  interpreter-recognized builtin. The semantics are fixed in §4.2; the mechanism
  is the builder's call. In-region ST is lazy by default with `!`-forced
  boundaries to WHNF, consistent with the runner's Complete mode (§4.1/§4.5).
- **Opaque primitive facility:** the mechanism for `World`/`STState`/`STRef`
  (a new opaque/primitive type kind vs builtin-only constructors) — required
  for the isolation and effect-ordering soundness claims.
- **Run-to-completion memory:** programs are assumed **bounded** for this plan —
  the existing fixed-capacity arena budget is kept, its limit documented, tests
  sized within it. Growing/collecting the arena is deferred to the memory-
  management follow-on.

---

## 7. Reference program (target)

```rill
-- io-streams core: read lines from stdin, transform, echo to stdout
handle = makeOutputStream (\ mb -> case mb of
    Nothing => return ()
    Just x  => do { write (Just x) stdout; handle });

main = do
    xs   <- fromList (map (\x -> x * 2.0) [1.0, 2.0, 3.0])   -- memory source
    ys   <- toList xs                                         -- memory sink
    ref  <- runST (fn s -> do
        r <- newSTRef 0.0
        writeSTRef r 42.0
        readSTRef r)
    write (Just ("sum-ish: " ++ show ys)) stdout              -- stdio out
    return ()
```

(Exact surface — `++`, `show`/string rendering — is settled in the plan; the
shape is the target: memory streams, STRef inside a `runST`, stdio.)

---

## 8. Verification outline

- `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang
  --features dsp`, `cargo fmt` — green, zero warnings.
- **IO/ST semantics:** STRef read/write/modify inside `runST`; two `runST`
  regions do not share state (isolation); an `STRef`/ST result escaping its
  region is a **compile-time** error; IO effects ordered by World threading by
  data dependency — call-by-need in Complete, eager in Strict, with `!` forcing
  to a boundary at annotated sites.
- **Streams (memory):** `fromList`→`read` round-trip incl. `Nothing` at EOF;
  `atEnd`; `unread`; `connect` input→output; `makeInputStream`/`
  makeOutputStream` sources/sinks; `toList` consumer.
- **Streams (stdio):** `stdin`/`stdout`/`stderr` round-trips against an injected
  capture console; `write Nothing` closes.
- **Program form:** `main : (String…) -> IO ()` with `set_args` String binding;
  a program with no I/O (`main = return ()`) runs to completion cleanly.
- **Bounded memory:** a `main` that accumulates beyond the fixed arena budget
  reports a clean capacity error (documented limit), not a crash.
- **Full replacement:** the `-p rill-lang` suite contains zero `readInput`/
  `writeOutput`/buffer-λ `main` program strings after migration; grep confirms;
  `rill-adrift`'s program strings are migrated, leaving only the known
  `graph`/`reconstruct.rs` baseline.

---

## 9. Prior art and supersession

- **Supersedes** the passive/active + backend + hot-loop + master-slave framing
  of the earlier revision of this same document, and the additive Task 1/2
  products built under it (`Stream a`, `asInput`/`asOutput`, `mkStream`,
  `BackendSpec`, `stream_backend.rs`, the master-slave `runStream`), where they
  conflict with the re-scoped core above.
- **Retains from it:** the scalar-arg `main`, `IO` as a reader over
  State+World, streams-as-values, `STD_PRELUDE`, the rank-2 `runST` escape
  check, the "no transition period" replacement directive.
- Builds on `2026-10-10-rill-lang-io-monad-design.md` (Stage B IO monad) and
  `2026-10-10-rill-lang-value-signal-unification-st-design.md` (the `ST`/
  `runST`/`Buffer` machinery this design's ST reuses and makes real).
