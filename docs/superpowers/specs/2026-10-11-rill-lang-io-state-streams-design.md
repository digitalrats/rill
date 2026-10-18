# rill-lang: IO as State over RealWorld + Stream I/O (Stage C)

> **Stage:** C of the rill-lang evaluation-model rework.
> **Prerequisite:** Stage B (full IO monad, FFI partial application, mono-kernel
> model) — `main :: Buffer -> … -> IO ()`, `writeOutput`/`readInput`, the graph
> front-end behind the `graph` feature.
> **Scope:** Make `IO a` a **State-style computation over a RealWorld token**,
> introduce reactive streams (`InputStream`/`OutputStream`) created inside the
> program, and a `runStream` primitive that blocks the main thread while a
> backend callback drives the stream. This is the foundation for a
> Turing-complete rill-lang able to express the full control logic of a
> `lang_chiptune`-class program (including scheduler-driven tasks).

---

## 0. Executive summary

The current `IO a = { unIO: a }` is a **record container**: effects
(`readInput`/`writeOutput`) are compiler-embedded instructions, executed
imperatively tick-by-tick. In `complete` mode the evaluation model is lazy
(call-by-need), so effect ordering is NOT structurally guaranteed — the runtime
leads the program and effects happen where the instructions sit.

Stage C changes the model to match Haskell's: **`IO a` is a function from a
`World` token to a result and a new `World`**:

```
IO a  ≅  World -> (a, World)          -- State monad over the world token
```

This gives, for free:

1. **Effect ordering** — the `World` token flows linearly through every
   `bind`; each operation consumes and returns a unique world.
2. **Global state** — `World` carries the program's global resources
   (buffers, tapes, channels, scheduler state) as its fields. Resources are
   created and returned via `(resource, World')`, not hidden in runtime globals.
3. **Turing completeness** — effects are first-class values (functions of the
   token); they can be stored in lists, passed around, recursed over, composed.
4. **Reactive streams** — `InputStream`/`OutputStream` are channels living in
   the world; `runStream handler` creates a stream, blocks the `main` thread
   (which runs in its own thread, so the block is harmless), and a backend
   callback drives the handler per block.

The block-track tick model is not removed — it becomes one **runtime
convention** (the backend callback = a separate thread) rather than the
language's only execution mode.

---

## 1. Context and current state (audited)

### 1.1 `IO a` today is a container, not a State computation

```haskell
data IO a = { unIO: a };
instance Monad IO where {
    return x = IO { unIO: x };
    bind m f = IO { unIO: (f m.unIO).unIO };
};
```

- `readInput : Int -> IO Buffer`, `writeOutput : Int -> Buffer -> IO ()` are
  builtin-sigs (`ty.rs`); `lower.rs` embeds them as `ValueReadInput` /
  `ValueWriteOutput` instructions.
- The runtime leads the root fragment each tick (`ProgramRunner::apply`), so
  effects execute at their syntactic position — imperatively.
- `main`'s λ-parameters are the input buffers; `writeOutput` drains outputs.

### 1.2 The tick model is a runtime convention, not the only execution mode

`Runtime::launch` (`runtime.rs`) wires capture → program → playback inside a
process callback set on the driver. The callback is effectively **a separate
thread** (the audio demon calls it); the `main` thread is free. This is the
seed of `runStream`: the program runs on its own thread, the backend callback
drives processing on another.

### 1.3 `complete` mode is lazy

In `complete` mode the value track is lazy (thunks, call-by-need). A
record-container `IO` does not guarantee effect ordering under laziness — a
`readInput` thunk can be forced late or not at all, and forcing order is not
the program's syntactic order. A linear `World` token is required for sound
effect semantics in a lazy language.

### 1.4 Readymade infrastructure

- `runST` + phantom `s` (rank-2) — the local-region State pattern already
  exists (`ty.rs` builtin-sig, `infer_apply_impl` special case, `lower.rs`).
  `IO` is the same pattern at global scope.
- `RegionHandle`/`BufferAllocator` (SP-4) — buffer memory; `Value::Buffer(idx)`
  is the value-track buffer handle.
- `IoCapture`/`IoPlayback`/`IoDriver` (`rill-core/src/io.rs`) — the stream
  boundary primitives.
- Collections (`List`, `Map`, `Set`), HOF (`map`/`fold`/`filter`), lazy Cons
  cells — the Turing-complete value layer.
- `ProgramRunner::apply` — pure per-tick transform, the future per-block
  stream step.

### 1.5 Reference program

`rill-adrift/examples/lang_chiptune.rs` — STC file → control thread
(`StcPlayer`, reads ClockTick, sends `SetParameter("regs", Bytes)`) + signal
thread (DSL `ay38910 … : lofi …`). The goal: express BOTH threads inside
rill-lang.

---

## 2. Design decisions

### 2.1 `IO a` = State over a World token

```
data World;                            -- primitive world type (not constructible)
IO a = \w -> Pair a w                -- State monad; w : World
```

- `World` is a **primitive token** (like `State# RealWorld` in GHC): no
  constructors, no pattern matching; only the runtime creates the initial
  `realWorld` and discards the final one.
- `return x = \w -> (x, w)`; `bind m f = \w -> let (a, w') = m w in (f a) w'`.
- `runIO main` — the runtime applies `main` to `realWorld`, discards the
  final world. `main :: IO ()`.
- **Effect = function of the token.** This is State, so it also provides the
  global state carrier: resources live in `World`.

### 2.2 Global resources live in the runtime, ordered by the token

`World` is a **primitive opaque token** (like GHC `State# RealWorld`): no
fields, no constructors, no observation. It only *orders* effects. The actual
global resources (buffers, tapes, channels, tables, scheduler queues) live in
the **runtime**, addressed by handles that flow as ordinary values. An effect
is a State step: it may read/change runtime state, and the token witnesses the
linear order.

This is the Reader/State distinction resolved: **it is State** — each step
potentially changes state — but the state is *in the runtime*, not in the
token. `IO a = World -> (a, World)`; the token is opaque, the state is real.

Resource constructors are State operations returning handles:

```haskell
newTape    :: Int -> IO (STRef s Buffer)      -- alloc tape (runtime table)
openInput  :: Int -> IO (InputStream Buffer)  -- bind a backend capture channel
openOutput :: Int -> IO (OutputStream Buffer)
share      :: a -> IO (Shared a)              -- persistent term (RegionHandle)
newTable   :: IO (Table k v)                  -- ETS-like concurrent table
```

The program owns handles; the runtime owns the memory.

### 2.3 Reactive streams: `InputStream` / `OutputStream`

```haskell
data InputStream a;    -- read  :: InputStream a  -> IO (Maybe a)
data OutputStream a;   -- write :: OutputStream a -> a -> IO ()
```

- Channels live in the World; `read`/`write` are State ops that block on the
  underlying queue (lock-free SPSC at the boundary).
- `readInput`/`writeOutput` keep the `Int`-indexed form as **sugar** over
  `openInput`/`openOutput` + channel ops, OR the stream form becomes primary
  (decided in the plan; both share the same runtime channel).

### 2.4 `runStream` — create a signal stream inside the program

```haskell
runStream :: (InputStream Buffer -> OutputStream Buffer -> IO ()) -> IO ()
runStream handler = do
    inputs  <- openInput 0          -- bind capture
    outputs <- openOutput 0         -- bind playback
    handler inputs outputs          -- blocks until the stream ends
```

- `main` runs in its own thread; `runStream` **blocks** that thread while the
  backend callback (a separate thread) drives `handler` per block. Blocking is
  harmless — the program thread does nothing else while the stream lives.
- The backend callback invokes `handler`'s compiled form per block:
  read inputs → run the DSP → write outputs (the `ProgramRunner::apply` shape).
- `runStream` returns when the backend stops (e.g. Ctrl+C / driver stop).

### 2.5 Turing completeness and control tasks

Once effects are first-class, the control layer of `lang_chiptune`-class
programs is expressible in rill-lang:

```haskell
main = do
    stc  <- openStcPlayer file            -- a task: awaits ClockTick, emits regs
    lfo  <- spawnAutomaton lfo            -- green thread: interval, SetParameter
    audio <- runStream (\ins outs -> do
        regs <- stc.step                    -- await next register block
        writeOutput outs (lofi (ay38910 1750000.0 regs) ...))
    return ()
```

- Scheduler (green threads) is a **runtime mechanism** over the sequential
  State threads — not a type-level token. (RealWorld provides ordering/linearity;
  the scheduler is the executor.)
- The runtime carries scheduler state: runnable queues, timers, channels.

### 2.5b Actors: channels, send/receive, spawn

Actors are expressed in rill-lang through IO over channels (Erlang-style):

```haskell
data Channel a                       -- mailbox / message queue handle
send    :: Channel a -> a -> IO ()      -- non-blocking enqueue
receive :: Channel a -> IO a            -- blocking: suspends the green thread
spawn   :: IO () -> IO ()               -- fork a green thread
newChannel :: IO (Channel a)

actorLoop :: Channel Command -> IO ()
actorLoop mailbox = do
    msg <- receive mailbox              -- block until a message arrives
    handle msg                          -- effects (SetParameter etc.)
    actorLoop mailbox                   -- recurse = keep listening

main = do
    mailbox <- newChannel
    spawn (actorLoop mailbox)
    send mailbox (SetParameter "regs" bytes)
```

Selective receive is ordinary `match` over the received message (Erlang
`receive` with patterns), re-waiting for unhandled variants:

```haskell
msg <- receive mailbox
match msg of
    SetParameter p -> handle p
    ClockTick t    -> step t
    _              -> actorLoop mailbox    -- re-wait
```

### 2.5c Memory: persistent terms and tables

The SP-4 `RegionHandle` (ref-counted view over a shared slab) is the basis for
Erlang-style shared memory:

```haskell
data Shared a                     -- immutable shared term
share :: a -> IO (Shared a)       -- persistent term (RC, zero-copy)
readShared :: Shared a -> IO a

data Table k v                    -- ETS-like concurrent table
newTable :: IO (Table k v)
insert :: Table k v -> k -> v -> IO ()
lookup :: Table k v -> k -> IO (Maybe v)
```

CAF-sharing (`osc = sine 440 0.5 0; main = osc , osc` — one instance) is the
existing persistent-term analogue for closed values; `share` generalizes it.

### 2.6 Required properties of IO

To support everything above, `IO` must have these properties:

| # | Property | Purpose | Existing |
|---|---|---|---|
| 1 | **Monad** (`bind`/`return`, `do`-notation) | compose effects | ✅ |
| 2 | **Linear opaque token** | order effects under laziness; no duplication | — |
| 3 | **State step** (token passes; runtime state may change) | global resources, scheduling | — |
| 4 | **Continuations** (suspend/resume) | blocking `receive`; green threads | ✅ lazy thunks |
| 5 | **Effects as first-class values** | pass/save effects, `spawn` | ✅ lambdas + `Kleisli` |
| 6 | **Resources with lifetimes** | channels/tables/tapes | — (RegionHandle RC) |
| 7 | **Nondeterminism (choice)** | scheduler pick; selective receive | runtime + `match` |

Critical insight: **continuations already exist** (lazy thunks,
`Value::Callable { fragment, env, cell }` — the memo cell IS a resumable
continuation). The scheduler is just a queue manager over them. The genuinely
new work is the linear token (2), State-step semantics (3), and resource
lifetimes (6).

---

## 3. Architecture

```
┌────────────────────────────────────────────────────────────┐
│ Language                                                   │
│  IO a  = \w -> Pair a w   (State over World token)       │
│  World = primitive token (runtime-created)                 │
│  InputStream/OutputStream = channels in the World          │
│  runStream, openInput/openOutput, newTape — IO primitives  │
├────────────────────────────────────────────────────────────┤
│ Compiler                                                   │
│  infer: IO as function type (World -> Pair a World)        │
│  lower: IO ops → State-threading + channel instructions    │
│  runStream → backend-binding fragment                      │
├────────────────────────────────────────────────────────────┤
│ Runtime                                                    │
│  realWorld created; main :: IO () applied, final world discarded│
│  scheduler (green threads) over sequential State threads   │
│  backend callback (separate thread) drives per-block       │
│  channels = lock-free SPSC queues at the boundary          │
└────────────────────────────────────────────────────────────┘
```

### 3.1 Components

1. **`World` + `IO` as State** (`types/ty.rs` prelude, `parser.rs`, `infer.rs`,
   `lower.rs`): `data World`, `IO a = \w -> Pair a w`, `Monad IO` rewritten,
   `runIO`/`runStream` primitives.
2. **Streams** (`lower.rs`, `program.rs`, `interp.rs`): `InputStream` /
   `OutputStream` values; `read`/`write` = channel ops.
3. **`runStream`** (`runtime.rs`, `program_runner.rs`): backend callback binds
   the handler; blocks the program thread.
4. **Resource ops** (`newTape`, `openInput`, `openOutput`, `share`,
   `newTable`): runtime-table allocation, `(handle, World')`.
5. **Channels & actors** (`send`, `receive`, `spawn`, `newChannel`): mailbox
   queues over continuations; `receive` suspends the green thread.
6. **Scheduler** (later): green threads over the State threads (queue manager
   over continuations).

### 3.2 Data flow

`main` (program thread) → `runStream handler` → blocks; backend callback
(audio thread) → per block: read capture → run handler's DSP → write playback.
Channels pass buffers via `Value::Buffer` handles (RegionHandle), zero-copy at
the boundary as today. Actor messages flow through mailbox channels; the
scheduler (when present) multiplexes green threads over the sequential State
evaluation.

---

## 4. Error handling

- **Channel closed**: `read` returns `Nothing` (stream end); `write` to a
  closed channel is a runtime error or silent drop (decided in plan).
- **Resource exhaustion** (runtime table overflow): compile-time budget check
  like today's buffer table.
- **`runStream` double-invocation** on one thread: rejected at runtime.
- **Actor mailbox full**: `send` blocks or drops (SPSC bounded queue policy,
  decided in plan).

---

## 5. Backward compatibility, RT safety, budget

- **Existing programs**: `main :: Buffer -> … -> IO ()` keeps working — the
  auto-desugar and indexed `writeOutput`/`readInput` remain as sugar over the
  stream form. The tick model remains a runtime convention.
- **RT safety**: the backend callback stays lock-free; channels are SPSC
  queues; no allocation in the RT path. The program thread (blocked in
  `runStream`) never touches the RT path.
- **Lazy soundness**: the linear `World` token guarantees effect ordering
  under call-by-need, replacing the imperative-instruction guarantee.

---

## 6. Testing

- **New**: `IO` State semantics (order of effects under laziness, linear
  token), resource allocation (tape/channel/table), `InputStream`/`OutputStream`
  round-trip, `runStream` blocking + backend callback driving, actor mailbox
  (`send`/`receive`/`spawn`), shared memory (`share`/`Table`),
  `lang_chiptune`-style program compiling and running.
- **Regression**: all Stage B tests (FFI partial application, mono kernel,
  graph front-end, IO monad) keep passing.

---

## 7. Verification

- `cargo test --workspace`, `cargo clippy --all-features --workspace`,
  `cargo fmt` — green, 0 warnings.
- `lang_chiptune` equivalent compiles and runs with the control thread
  expressed in rill-lang.

---

## 8. File map

| Area | Files |
|---|---|
| `World`/`IO` State | `types/ty.rs`, `parser.rs`, `types/infer.rs`, `lower.rs` |
| Streams | `lower.rs`, `program.rs`, `backend/interp.rs` |
| `runStream` | `runtime.rs`, `program_runner.rs` |
| Resource ops | `lower.rs`, `program.rs` |
| Channels/actors | `lower.rs`, `program.rs`, `backend/interp.rs` |
| Shared memory | `lower.rs`, `program.rs` (RegionHandle) |
| Scheduler (later) | `runtime.rs`, new `scheduler.rs` |
| Tests | `tests/io_state.rs`, `tests/streams.rs`, `tests/run_stream.rs`, `tests/actors.rs` |

---

## 9. Non-goals (deferred)

- Scheduler / green threads / timer tasks (the `lang_chiptune` control layer)
  — the next stage after `runStream`/channels.
- Fault tolerance / supervision trees / links (Erlang OTP) — requires
  cross-process error handling.
- Zero-copy DMA at the I/O boundary.
- `Array`/`Vector` collection types on `RegionHandle` memory.