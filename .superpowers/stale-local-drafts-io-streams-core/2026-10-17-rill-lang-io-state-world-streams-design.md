# rill-lang: IO as Reader over State + World, Streams as Values (Stage C refinement)

> **Stage:** C refinement — builds on and supersedes
> `2026-10-11-rill-lang-io-state-streams-design.md` (Stage C).
> **Prerequisites:** Stage A (lazy value track), Stage B (ST-style `Buffer`,
> `runST` phantom `s`, IO monad, mono-kernel), the existing `ProgramEngine`,
> `RegionHandle`/`BufferPool` memory layer.
> **Scope:** Finalize the I/O architecture of rill-lang: **no signal-path vs
> data-path dichotomy anywhere**; the program is `(arg₁…argₙ) -> IO` with scalar
> args; `IO` is a reader over `State + World` (memory-manager and backend
> references flow through `State`); `readInput`/`writeOutput` are replaced by
> **operations over stream values** (`InputStream`/`OutputStream`) with a
> backend-independent contract exchanging `List Buffer` chunks; streams are
> **passive or active** in both directions; the **active backend drives only the
> hot signal loop**; RT-safety is a **type-level guarantee**; console and file
> I/O are ordinary stream backends.
>
> The graph feature's reconstruction (`GraphSpec → Program`) is **deferred**
> until this I/O architecture lands, because its target program form depends on
> it (see §5).

## North-star requirement

**A `lang_chiptune`-class program must be expressible entirely in rill-lang.**
That means the program itself **creates backends, threads, and actors inside
the language** — not behind it:

- **Backends** — audio capture/playback, tape, file, console — created inside
  the program via `mkStream`-class constructors, behind the one stream
  contract (§4.2).
- **Threads** — a program spawns its own work loops (control/scheduler-style
  green threads) via `spawn` (§4.7).
- **Actors** — asynchronous entities with mailboxes (Erlang-style send/
  receive/spawn loops) created inside the program (§4.7).

The flagship control layer (STC player stepping from a file, an automaton
emitting `SetParameter`, receiving `ClockTick`) is therefore *in-language*:
it is ordinary rill-lang actors, threads, and streams. The runtime provides
the scheduler/channel mechanics; the program composes them.

**The transition this design enacts:** `ProgramRunner` stops being a *signal
processor* (a fixed `&[&[f32]] → &mut [&mut [f32]]` transform) and becomes the
**executor of a fully Turing-complete program, tied to no specific domain**.
Signal processing is just one domain the program can express (functions over
`Buffer` values via the DSP FFI); the runner's identity is *executing the
program*, not transforming signals. Backward compatibility with the old forms
is deliberately **not required** — existing programs migrate to the stream
form.

---

## 0. Executive summary

The recurring failure of the graph-reconstruction work traced to a single
root: the representation kept reverting to a **signal path vs data path**
split (a Faust-era inheritance). This design eliminates that split **everywhere**,
including at the I/O boundary, by making every entity a value:

- **Program = `(arg₁…argₙ) -> IO`** — the arguments are **scalars**, not
  buffers. External usage is "pass scalars, get an action".
- **`IO` is a reader over `State + World`**, following GHC's model
  (`IO a ≅ State# RealWorld -> (# State# RealWorld, a #)`): a linear opaque
  `World` token orders effects; `State` carries the references that matter
  (memory manager, backend registry/factory).
- **Streams are values.** `InputStream`/`OutputStream` expose a
  **backend-independent contract** exchanging chunks of `List Buffer`
  (multi-channel frames) inside the existing `ST`-style effect model. The
  global `readInput`/`writeOutput` primitives are replaced by **operations
  over these stream values**; backends are created **inside the program**
  via a `mkStream`-class constructor.
- **Passive and active, both directions.** A stream is passive (the program
  drives it) or active (it drives the program). Which one it is is decided by
  the **threading context: on which thread the backend's callback was
  created**. Every running program has ≥1 active backend and ≥1 passive
  backend; the passive is symmetric to the active and is serviced within the
  active's tick.
- **The active backend drives only the hot signal-processing loop**, not the
  whole program. The warm setup (creating streams, wiring) runs once. The hot
  loop is a **distinct, RT-safe-by-construction type** (ST-like, phantom `s`,
  rank-2 confined) that cannot leak warm effects and cannot allocate (rill's
  pre-allocated `BufferPool`/`RegionHandle` is exactly the mechanism that
  makes a GHC-RTS-style hard-RT hazard avoidable).
- **Console and file I/O are ordinary stream backends** — trivially expressible
  behind the same contract.

---

## 1. Intent and why

### 1.1 The dichotomy is the bug

The graph reconstruction kept failing because both the subagent and the
representation kept splitting the world into a "signal path" and a "data
path". In the unified model there is no such split: a signal is a value
(`FixedBuffer a`), a stream is a value, composition is function application.
The I/O boundary was the last place the dichotomy survived — `readInput`/
`writeOutput` as global, special-cased instructions. This design closes that
last gap.

### 1.2 Hard-RT is solved by memory management, not by a second language

GHC's RTS makes hard realtime on the Haskell heap notoriously hard (allocation,
GC). rill's memory layer — the size-classed `BufferPool`, pre-allocated
`FixedBuffer` blocks, `RegionHandle` zero-copy sharing — was designed exactly
to avoid that. The design consequence: the **hot loop must be allocation-free
by construction**, and that must be a type-level guarantee, not a runtime
convention.

### 1.3 GHC provides validated shapes

- `IO a = State# RealWorld -> (# State# RealWorld, a #)` — the reader-over-
  State+World idea, with a linear opaque token ordering effects.
- `ST s a` with `runST :: (forall s. ST s a) -> a` — the canonical way to
  confine effects to a region (the analog of the RT-hot-loop boundary).
- `io-streams` — `InputStream a` (pull: `IO (Maybe a)`) and `OutputStream a`
  (push: `Maybe a -> IO ()`) as backend-independent values; `Nothing` = EOF /
  close; a push side expressed as a `Generator` over an `OutputStream`.
- Pull-based composition (`conduit`/`pipes`) gives backpressure for free.

We take the shapes, not the dependencies.

---

## 2. Goals, non-goals, anti-goals

### Goals

1. **`(arg₁…argₙ) -> IO` scalar program form** — no buffer λ-parameters on
   `main`; simpler external usage.
2. **Streams replace global `readInput`/`writeOutput`** — I/O is operations
   over `InputStream`/`OutputStream` values.
3. **Backend-independent stream contract** over `List Buffer` chunks, usable
   in the existing `ST`-style effect model.
4. **Passive and active streams, both directions**, unified by one contract
   with backpressure (active = the producer may push, but honors the consumer's
   requested quantum `request(n)`; passive = the consumer always pulls).
5. **`IO` as a reader over `State + World`**: a linear `World` token orders
   effects; `State` carries memory-manager and backend references; backends
   are created **inside the program** (`mkStream`).
6. **The active backend drives only the hot signal loop**; the passive side is
   symmetric and serviced within the active's tick.
7. **RT-safety at the type level**: the hot loop is a distinct ST-like type
   that cannot leak warm effects and cannot allocate.
8. **Console (`stdin`/`stdout`) and file I/O as ordinary stream backends.**
9. Preserve Turing completeness (effects are first-class values).
10. **`lang_chiptune` fully in rill-lang**: backends, threads, and actors are
    created inside the program (see North-star requirement and §4.7).

### Non-goals (this stage)

- OTP-style fault tolerance / supervision trees / links (cross-process error
  handling) — deferred.
- Modules/import (`Program` feature) — deferred.
- The graph feature's reconstruction and its SADT/R-schema rendering
  (deferred until the I/O model is stable; see §5).

### Anti-goals (a failure even if it technically works)

- Re-introducing a signal-path/data-path separation anywhere in the model.
- Keeping `readInput`/`writeOutput` as the primary I/O.
- Buffer λ-parameters on `main` as the program's I/O contract.
- Any allocation, lock, or syscall on the hot RT path.
- Requiring the caller to know or configure which backend is active at the
  type level of a general stream (activity is a property of the concrete
  stream/backend pairing, expressed at the boundary where the callback lives).
- A `lang_chiptune`-class program that needs anything significant *outside*
  rill-lang to be written (control logic, backend wiring, actor loops).

---

## 3. Constraints (binding)

- **RT rules** (AGENTS.md): no heap allocation, no locks, no `thread::sleep`,
  no file/socket I/O, no `log`/`println`/`format!` on the RT path. The hot
  loop's type must make violating these inexpressible.
- **Memory model**: size-classed `BufferPool` (rill-core `memory/pool.rs`),
  `FixedBuffer` pre-allocation, `RegionHandle`/`Arena` zero-copy sharing.
  Zero-copy across stream boundaries where the current boundary already does
  it (channel handles = `Value::Buffer`).
- **No new third-party dependencies** without explicit confirmation. All
  mechanisms come from the existing `ST`/`runST` phantom pattern, the `Buffer`
  typeclass, `RegionHandle`, and the IO-monad machinery.
- `rill-lang` denies `unsafe_code`, warns on `missing_docs`. Docs in English.
- **No backward compatibility (decision):** the transition breaks the old
  forms deliberately. The buffer-λ-parameter `main`, the global `readInput`/
  `writeOutput` primitives, and their indexed variants are removed; existing
  programs and tests are migrated to the stream form (`main : (arg₁…argₙ) ->
  IO ()` + `runStream`). No sugar layer is kept.

---

## 4. The "how" decided

### 4.1 `IO` as a reader over `State + World`

```
data World;                              -- primitive opaque token (GHC State# RealWorld analog)
IO a  ≅  World -> (a, World)             -- State step over the token (orders effects)
```
plus a **reader environment `State`** threaded alongside, carrying the
references that a program needs without being re-orderable state:
memory manager / buffer pool, and the backend registry (the factory for
`mkStream`, plus which backend is the active clock). Concretely the composite
is a Reader-over-State: `IO a` reads `State` and threads the linear `World`.

- `World` has no constructors and cannot be observed or duplicated — only the
  runtime creates `realWorld` and discards the final one (existing Stage C
  decision).
- **State** holds references (not buffers): the memory manager and the
  backend registry. Resources/streams themselves are created inside the
  program and flow as ordinary values (`(stream, World')`), as in Stage C.

### 4.2 Streams replace `readInput`/`writeOutput`

```
data InputStream  a;    -- pull:  read  :: InputStream a  -> Maybe (List a)   -- Nothing = EOF/closed
data OutputStream a;    -- push:  write :: OutputStream a -> List a -> ()     -- write Nothing closes
```

- **Chunk = `List Buffer`** (a multi-channel frame), matching today's
  `readInput : () -> List Buffer` / `writeOutput : List Buffer -> IO ()`
  contracts — but as **operations over stream values**, not global
  instructions.
- **Backend-independent**: the program sees only `read`/`write`/`close`
  (+ `request` for backpressure); the concrete backend (audio capture/
  playback, tape, file, console) is behind an instance.
- **Expressed with the language's full HOF/HKT machinery** (not special-cased):
  `InputStream`/`OutputStream` are higher-kinded type constructors
  (`* -> *`, kind-checked like `IO a`/`List a`), and the `runStream` handler
  is a first-class higher-order function. The stream types and the 
  domain-agnostic standard surface live in a general **`STD_PRELUDE`** — the
  common standard-library layer, registered alongside the strictly-category-
  theory `CATEGORY_PRELUDE` and the signal-domain `SIGNAL_PRELUDE`. Streams
  never go in `CATEGORY_PRELUDE`. `STD_PRELUDE` is also the home of the later
  threads/channels/actors surface and other general utilities.
- Backends are created **inside the program**:
  ```
  mkStream :: BackendSpec -> IO (InputStream Buffer, OutputStream Buffer)  -- e.g.
  openInput  :: Int -> IO (InputStream Buffer)
  openOutput :: Int -> IO (OutputStream Buffer)
  openFile   :: FilePath -> IO Stream        -- file backend
  consoleIn  / consoleOut                    -- stdin/stdout backends
  ```
  All of these are instances of the same stream contract — this is why
  console and file I/O are trivial additions.

### 4.3 Passive and active — a single contract with backpressure

- A stream is **passive** when the program drives it (pull read / on-demand
  write) from its own execution step.
- A stream is **active** when the backend drives the program: its callback —
  created on the backend's serving thread — pushes chunks in (active input)
  or requests chunks out (active output).
- Both are **one contract**: the consumer declares a requested quantum
  (`request(n)`), the producer honors it. An active stream is exactly a
  passive one where the producer may initiate, but still within the requested
  quantum. This is the Reactive-Streams style, folded into the stream value.
- **Which one a stream is is decided by the threading context: on which
  thread its backend callback was created.** The type of a stream does not
  need to brand it active/passive for ordinary use; the boundary where the
  callback lives (the active backend's tick) is where activity enters.

### 4.4 Active/passive pairing; the hot loop

- Every running program has **≥1 active backend and ≥1 passive backend**. The
  active backend is physically an input or an output stream; the passive is
  **symmetric** to it (complementary direction) and is **serviced within the
  active backend's tick** — this matches today's model where passive tape
  heads / files are serviced inside the signal callback.
- **The active backend drives only the hot signal-processing loop**, not the
  whole program. The **program-facing per-block contract is `runStream
  handler`** (decision):
  ```
  runStream :: (InputStream Buffer -> OutputStream Buffer -> IO ()) -> IO ()
  ```
  The handler is a rill-lang function over the active input/output stream
  values, invoked per block on the RT thread. The pure `hotLoop` below is an
  **internal** of that contract — what the runtime extracts from the handler
  and calls per tick — not a separate public form.
  ```
  hotLoop :: (forall s. List Buffer -> ST s (List Buffer))   -- internal, per-tick, RT-safe by type
  ```
  Warm setup (create streams, wire, `mkStream`, open files/console, `spawn`
  actors) runs once in `IO`, before the RT thread starts.
- **Allocation guarantee (decision):** "no allocation" on the hot path means
  **bounded bump-allocation into the pre-allocated pool counts as not
  allocating** — i.e. the existing RT discipline (no heap alloc, no locks, no
  syscalls) plus the type boundary that confines the hot loop. No separate
  non-allocating interpreter is introduced, and no affine/linear types are
  required for this guarantee.
- The hot-loop type is **ST-like with a phantom `s`** (the existing `runST`
  one-point rank-2 pattern): it cannot perform warm effects (no `IO` escape),
  and the RT rules ban heap allocation, locks, syscalls, and logging in it.
  This is where RT-safety is a **type-level guarantee** (plus the runtime
  convention; see §4.5).

### 4.5 Effect ordering, resources, Turing completeness

- The linear `World` token orders effects under call-by-need (the laziness-
  soundness gap identified in Stage C is closed by the token, exactly as in
  GHC).
- **Linearity guarantee (decision):** for now the `World` token is a
  **runtime convention with strict bind ordering** — `IO`-bind threads the
  token strictly and in order; there is no compiler-enforced linear-type
  subsystem yet. A compiler-checked linearity system is a possible later
  stage, not part of this design.
- Global resources live in the runtime, addressed by handles that flow as
  values (`newTape`, `openInput`, `openOutput`, `share`, `newTable`, …) —
  unchanged from Stage C.
- Effects are first-class values (functions of the token): they can be stored,
  passed, recursed over — the basis of the control layer of
  `lang_chiptune`-class programs.

### 4.6 Console and file I/O

- `stdin`/`stdout` and files are just **more backend instances** behind the
  same `InputStream`/`OutputStream` contract. Console line/raw and file
  read/write share the chunk model (`List Buffer`, or textual chunks where
  applicable). Nothing about the stream model needs to change to admit them.

### 4.7 In-language concurrency: threads and actors

To make a `lang_chiptune`-class program fully in rill-lang, the language
provides threads and actors as **ordinary effects on stream/channel values**
(Erlang-style, building on Stage C §2.5b):

```
data Channel a                 -- mailbox / message-queue handle
newChannel :: IO (Channel a)
send       :: Channel a -> a -> IO ()
receive    :: Channel a -> IO a     -- blocks the green thread until a message
spawn      :: IO () -> IO (Thread)  -- fork a green thread inside the program
```

- An **actor** is a recursive green-thread loop reading its mailbox and
  issuing effects (`receive` → `match` → handle → recurse), exactly as in
  Stage C.
- **Threads** are multiplexed by the runtime **scheduler** over the sequential
  State threads; `receive` suspends the green thread on a queue. The scheduler
  is therefore a **required** runtime mechanism (it is how "threads and actors
  inside the program" execute), no longer a deferred afterthought.
- **Backends as actors, serviced in the tick:** a passive stream is serviced
  **synchronously inside the active backend's tick** — the `read_mic`/`play_wav`
  style needs no queue at all (the operation is a cheap, in-memory buffer
  exchange). A **warm helper thread + SPSC queue is only introduced where a
  backend genuinely blocks** (disk file read, console input, an STC-player
  stepping a file) — it is a per-backend need, not a blanket requirement for
  passive streams. Either way, blocking I/O never touches the RT thread: it
  runs on a warm helper and reaches the hot loop through the lock-free queue
  only when that backend actually needs it.
- The control layer of the reference program (`stc.step`, automaton clocks,
  `SetParameter` for the audio-tone params) is expressed as such in-language
  actors/threads; the runtime's `?name`/parameter mechanism becomes a low-level
  effect the program's own actor issues, not a separate external surface.

---

## 5. Relationship to the deferred graph reconstruction

The graph feature's `reconstruct` (`GraphSpec → Program`) is deferred until
the I/O model lands. Once `main = (scalars) -> IO`, streams are values, and
I/O is operations over streams, the reconstruction becomes a straightforward
value composition (no channels, no signal path): a graph node is a function
over buffer values; sinks/sources become stream operations. That plan is
written after this one; this document is its prerequisite.

---

## 6. Left to the builder (decisions the implementer/plan may settle)

- The exact reactive-streams typeclass signature: which operations the class
  exposes (`read`, `write`, `close`, `request`/backpressure), how the `ST`
  boundary sits in the method types, and whether activity is a constructor
  argument or a separate marker.
- How `State` and `World` are represented and lowered (reuse the Stage C
  `World`/`IO`-as-State machinery vs new representation), and how `State` is
  introduced at launch.
- `mkStream`/backend-registry mechanics: the shape of `BackendSpec`, how the
  registry is populated, how the active backend is designated at launch.
- How the hot-loop type is defined and connected to the active backend's
  callback (concrete type name, `runST`-style escape guard, how per-block
  `List Buffer` chunks are passed).
- Migration of existing programs: sugar (indexed `readInput`/`writeOutput`,
  buffer-λ `main`) retained vs migrated; the plan decides, the new form is
  primary.
- Console/file chunk encoding (textual vs raw `Buffer`).

---

## 7. Reference program (target)

A `lang_chiptune`-class program expressed purely in rill-lang:

```rill
main = do
    tape    <- newTape 44100 * 2
    stc     <- openStcPlayer "song.stc"        -- control-side stream (a backend + actor)
    regsCh  <- newChannel                      -- actor mailbox: ClockTick -> regs
    spawn (actorLoop regsCh stc)               -- in-language control thread
    input   <- openInput 0                     -- active capture
    output  <- openOutput 0                    -- active playback
    runStream (fn ins outs -> do
        regs <- receive regsCh                 -- next register block from the actor
        let fx = lofi 8 44100 0.75 1.0 1 0 1 (ay38910 1750000.0 regs)
        write outs fx
        return ())
    return ()
```

The hot loop (per-block signal processing) is the RT-safe, allocation-free
fragment; backends, threads, and actors — everything else — are ordinary
rill-lang values and effects created inside the program.

---

## 8. Verification outline

- `cargo test --workspace`, `cargo clippy --all-features --workspace`,
  `cargo fmt` — green, zero warnings.
- Stream round-trips: pull and push, passive and active, EOF/close.
- Effect ordering under `complete` (lazy) mode: a test proving the linear
  token orders effects.
- Hot-loop type: a test proving warm effects cannot escape the hot loop
  (compile-time), and that the hot path allocates nothing (existing
  RT-discipline checks).
- Console/file stream tests: stdin/stdout and file backends behind the same
  contract.
- In-language concurrency: `spawn` green threads, `Channel` send/receive,
  an actor loop that drives a stream (the reference program's control layer)
  — all inside rill-lang.
- The `lang_chiptune` reference program compiles and runs with no external
  control wiring (backends, threads, actors created in the program).

---

## 9. Prior art and supersession

- Supersedes the program-form and I/O-primary decisions of
  `2026-10-11-rill-lang-io-state-streams-design.md` where they conflict
  (buffer-λ `main` → scalar-arg `main`; streams primary over
  `readInput`/`writeOutput`; adds passive/active-by-threading, the RT-safe
  hot-loop type, console/file backends, and — per the north-star — **in-language
  backends, threads, and actors with a required scheduler** instead of a
  deferred one).
- Builds on `2026-10-10-rill-lang-io-monad-design.md` (Stage B IO monad) and
  `2026-10-10-rill-lang-value-signal-unification-st-design.md` (the `ST`/
  `runST`/`Buffer` machinery this design's hot loop reuses).
