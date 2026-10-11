# rill-lang: IO as State + Streams + Actors (Stage C) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** make `IO` a State computation over an opaque World token, introduce reactive streams (`InputStream`/`OutputStream`) created inside the program, a `runStream` primitive, actor channels (`send`/`receive`/`spawn`), and shared memory (`share`/`Table`). This is the foundation for a Turing-complete rill-lang able to express a `lang_chiptune`-class program's full control logic. See `docs/superpowers/specs/2026-10-11-rill-lang-io-state-streams-design.md`.

**Architecture:** Six tasks. S-1 IO as State (linear opaque token, State-step semantics, `Monad IO` survives). S-2 primitives + runtime tables (channels, resources, shared memory). S-3 `runStream` (backend callback binds handler; blocks the program thread). S-4 actors (`send`/`receive`/`spawn`/`newChannel` over continuations). S-5 shared memory (`share`/`Table`). S-6 tests + full suite green.

**Tech Stack:** Rust, crate `rill-lang`, branch `feature/rill-lang-arrays`. `rill-core-actor` (mailbox) already a dependency. No new external dependencies.

---

## Conventions

- Verify after every task: `cargo test -p rill-lang`; before finishing `cargo clippy --all-features --workspace` + `cargo fmt`.
- Pre-commit hook runs `cargo fmt`.
- Commit style: conventional commits, single-quoted `-m`.
- Stage B landed: `IO a = { unIO: a }` + `instance Monad IO`; `readInput : Int -> IO Buffer`, `writeOutput : Int -> Buffer -> IO ()`; `runST` (rank-2 phantom); `Kleisli m a b` + `instance (Monad m) => Arrow (Kleisli m)`; lazy thunks (`Value::Callable { fragment, env, cell }`) ARE resumable continuations; graph front-end behind `graph` feature.
- The forked-rust dialect: no int→f32 coercion (write `&[2.0]`), core::/std:: preludes, `matches!`/`vec![]`.

---

## Task S-1: IO as State — linear opaque token

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (prelude), `rill-lang/src/types/infer.rs`, `rill-lang/src/lower.rs`
- Test: new `rill-lang/tests/io_state.rs`

**Goal:** `IO a` types as a State computation `World -> (a, World)` with a linear opaque token. Effect ordering under laziness is guaranteed by the token, not by instruction position.

- [ ] **Step 1: Add `World` primitive + redefine `IO a`**

In `ty.rs` `CATEGORY_PRELUDE`:
```haskell
data World;                                  -- primitive opaque token
data IO a = fn w -> Pair a w;                -- State: World -> (a, World)
```
`Monad IO`:
```haskell
instance Monad IO where {
    return x = fn w -> Pair { first: x, second: w };
    bind m f = fn w -> let p = m w in (f p.first) p.second;
};
```
`World` has no constructors, no fields, no pattern-matching arms — only the
runtime creates the initial token and discards the final one. Add a kind check
in the parser/typer: `World` is not constructible (like the runST phantom).

- [ ] **Step 2: `World` non-constructible + linearity checks**

`infer.rs`: reject `World { … }`, `World` used as a data value, or matching on
`World` — mirror the phantom-type treatment in `runST`. A `World`-typed value
cannot be stored in a `List`, `Map`, or `Pair` (it is opaque); using it as a
function argument/result is allowed (it flows through).

- [ ] **Step 3: `runIO` primitive**

`runIO :: (forall w. IO w a) -> a` — like `runST`, but for the World token.
The runtime applies the compiled action to `realWorld` and discards the final
world. `main :: IO ()` becomes `runIO main` (or the runtime does it
implicitly). `runIO`'s forall-w argument reuses the `runST` rank-2 machinery
(generalize `is_runst` handling to a shared helper).

- [ ] **Step 4: IO State lowering**

`lower.rs`: `IO` actions still lower through the value track, but effect
primitives (`readInput`, `writeOutput`) now thread the token: emit instructions
that consume a token register and produce the next one. The tick-end output
drain stays. Add a `ValueWorld`-like marker only if the compiler needs to
thread it; prefer erasure (the token is compile-time) with the *linearity*
enforced by inference.

- [ ] **Verify:** `cargo test -p rill-lang --test io_state` green; `Kleisli`/categories tests still pass.

---

## Task S-2: Primitives + runtime resource tables

**Files:**
- Modify: `rill-lang/src/types/ty.rs`, `rill-lang/src/program.rs`, `rill-lang/src/lower.rs`, `rill-lang/src/backend/interp.rs`
- Test: `tests/io_state.rs` (extend)

**Goal:** runtime resource tables (channels, tapes, tables) with handle-returning
State primitives. Resources live in the runtime; the program owns handles.

- [ ] **Step 1: Runtime resource tables**

`program.rs`: add `Vec`-backed tables (channel registry, table registry) with
budget checks mirroring `alloc_buffers`. Handles are `Value::Int`-based opaque
ids (or a new `Value::Handle(idx, kind)`).

- [ ] **Step 2: Primitives**

```haskell
newChannel :: IO (Channel a)
openInput  :: Int -> IO (InputStream Buffer)
openOutput :: Int -> IO (OutputStream Buffer)
newTape    :: Int -> IO (STRef s Buffer)
```
`Channel a`/`InputStream a`/`OutputStream a` are value types wrapping a handle.
Each primitive is a builtin-sig (like `readInput`) lowered to a runtime-table
alloc + handle-value emission.

- [ ] **Step 3: Stream channel ops**

```haskell
readStream  :: InputStream a  -> IO (Maybe a)
writeStream :: OutputStream a -> a -> IO ()
```
Bound to the backend capture/playback (SPSC queues). `readStream` on an
exhausted stream returns `Nothing`.

- [ ] **Verify:** `cargo test -p rill-lang --test io_state` green.

---

## Task S-3: `runStream`

**Files:**
- Modify: `rill-lang/src/runtime.rs`, `rill-lang/src/program_runner.rs`, `rill-lang/src/lower.rs`, `rill-lang/src/types/infer.rs`
- Test: new `tests/run_stream.rs`

**Goal:** `runStream :: (InputStream Buffer -> OutputStream Buffer -> IO ()) -> IO ()` creates a signal stream inside the program; blocks the program thread while the backend callback (separate thread) drives the handler per block.

- [ ] **Step 1: `runStream` primitive + lowering**

`runStream handler` — `handler` is a lambda. Lower it to a backend-binding
fragment; the runtime registers the per-block step (the `ProgramRunner::apply`
shape) as the driver callback. `main`'s thread blocks until the driver stops.

- [ ] **Step 2: Runtime binding**

`runtime.rs`: `runStream` wires capture → handler → playback like
`Runtime::launch` today, but the handler is the compiled lambda (not `main`).
The program thread blocks (join/sleep loop) until the backend stops.

- [ ] **Step 3: Backward compat**

Indexed `readInput`/`writeOutput` stay as sugar: `writeOutput 0 buf` ≡
`writeStream (openOutput 0) buf`. Old `main :: Buffer -> … -> IO ()` programs
keep working (auto-desugar unchanged).

- [ ] **Verify:** `tests/run_stream.rs` (blocking runStream + mock backend callback driving the handler) green.

---

## Task S-4: Actors — channels, send/receive, spawn

**Files:**
- Modify: `rill-lang/src/program.rs`, `rill-lang/src/lower.rs`, `rill-lang/src/backend/interp.rs`, `rill-lang/src/types/infer.rs`
- Test: new `tests/actors.rs`

**Goal:** actor model over continuations: `send`/`receive`/`spawn`/`newChannel`.

- [ ] **Step 1: Channel runtime**

`program.rs`: mailbox queues (bounded SPSC, `rill-core-actor::Mailbox` reuse).
`send :: Channel a -> a -> IO ()` enqueues; `receive :: Channel a -> IO a`
dequeues, suspending the green thread when empty.

- [ ] **Step 2: Suspension via continuations**

`backend/interp.rs`: `receive` on an empty mailbox **suspends the current
continuation** (its `Value::Callable` cell / thunk) and registers it as a
waiter on the channel; when `send` arrives, the scheduler resumes the waiter.
This is the continuation machinery that already exists.

- [ ] **Step 3: `spawn`**

`spawn :: IO () -> IO ()` forks a green thread: a fresh continuation (the
argument) added to the scheduler's runnable queue. The scheduler multiplexes
over the sequential State evaluation (round-robin; `receive` yields).

- [ ] **Step 4: Selective receive via match**

`receive` returns the message; `match` over it re-waits for unhandled variants
(the mailbox keeps unread messages). Test the `lang_chiptune`-style actor loop.

- [ ] **Verify:** `tests/actors.rs` (ping-pong, selective receive, spawn) green.

---

## Task S-5: Shared memory

**Files:**
- Modify: `rill-lang/src/lower.rs`, `rill-lang/src/program.rs`, `rill-lang/src/backend/interp.rs`
- Test: new `tests/shared_memory.rs`

**Goal:** Erlang-style shared memory: persistent terms and ETS-like tables.

- [ ] **Step 1: `share`/`readShared`**

`share :: a -> IO (Shared a)` — snapshots a value into a RegionHandle-backed
persistent term; `readShared :: Shared a -> IO a` reads it (zero-copy RC).

- [ ] **Step 2: `Table`**

`newTable :: IO (Table k v)`, `insert :: Table k v -> k -> v -> IO ()`,
`lookup :: Table k v -> k -> IO (Maybe v)` — ETS-like concurrent table in the
runtime.

- [ ] **Verify:** `tests/shared_memory.rs` green.

---

## Task S-6: Tests, integration, full suite

- [ ] **Step 1: lang_chiptune-style program**

A test program expressing the STC control thread in rill-lang (actor: awaits
ClockTick messages, sends `SetParameter`), driving the AY-3-8910 + lofi signal
path through `runStream`. Compiles and runs.

- [ ] **Step 2: Re-enable deferred tests**

Re-enable `#[ignore]`d tests that Stage C unblocks (actor/control tests,
tape tests if resources landed).

- [ ] **Step 3: Full verification**

`cargo test --workspace --all-features` + `cargo clippy --all-features
--workspace` + `cargo fmt` — green, 0 warnings.

---

## Non-goals (this plan)

- Scheduler as a full green-thread runtime with timers — the next plan after
  S-4's minimal round-robin.
- Fault tolerance / supervision trees / links (Erlang OTP).
- Zero-copy DMA at the I/O boundary.
- `Array`/`Vector` collection types on `RegionHandle` memory.