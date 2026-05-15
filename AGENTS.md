# Rill — AGENTS.md

## Workspace layout

Cargo workspace — 18 active crates:

| Crate | Status |
|---|---|
| `rill-core` | Active — base traits, math, buffers, queues, time, macros, executor, interpolation |
| `rill-core-actor` | Active — actor model (ActorRef, Actor, ActorSystem) for lock-free message passing |
| `rill-core-dsp` | Active — DSP algorithm trait, filters, generators, delay, vector ops, sample player |
| `rill-graph` | Active — signal graph (DAG) with topological sort |
| `rill-oscillators` | Active — oscillators, LFO, envelopes, wavetable oscillator node |
| `rill-digital-filters` | Active — Biquad, SVF, comb, MoogLadder filters |
| `rill-digital-effects` | Active — Delay, Distortion, Limiter |
| `rill-router` | Active — EQ + mixer + routing |
| `rill-patchbay` | Active — automation (LFO, envelopes, sensors, servos) |
| `rill-lofi` | Active — lo-fi emulation |
| `rill-io` | Active — I/O backends (PortAudio, ALSA, PipeWire, JACK) |
| `rill-telemetry` | Active — probes, collectors |
| `rill-core-wdf` | Active — WDF elements, adapters, analysis |
| `rill-analog-filters` | Active — WDF-based analog filters (WdfMoogLadder) |
| `rill-analog-effects` | Active — op-amp, tape deck, preamp models |
| `rill-osc` | Active — OSC server and networking |
| `rill-sampler` | Active — sample playback, time-series reader, WAV loading |
| `rill-adrift` | Active — umbrella crate for signal processing applications |

Dependency tree:
- **`rill-core`** — foundation (depended on by all crates except `rill-osc`)
- **`rill-core-dsp`** — DSP algorithms (depends on `rill-core`)
- **`rill-graph`** — signal graph (DAG), depends on `rill-core` only (no DSP dependency). Contains `Graph`, `GraphBuilder`, `Port::propagate` — no external engine loop.
- **`rill-io`** — I/O backends only (`IoBackend` trait + PortAudio/ALSA/PipeWire/JACK). No engine, no processors. `rill-graph::Port::propagate` drives the graph in the I/O callback.
- **`rill-osc`** — standalone crate (no internal workspace deps)

  Crates depending on both `rill-core` + `rill-core-dsp`:
  `rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`, `rill-router`
- **`rill-core-wdf`** — WDF core, depends on `rill-core`
- **`rill-analog-filters`** — depends on `rill-core` + `rill-core-wdf`
- **`rill-analog-effects`** — depends on `rill-core` + `rill-core-wdf`
- **`rill-sampler`** — graph nodes for sample playback and time-series reading; depends on `rill-core` + `rill-core-dsp`
- **`rill-adrift`** — umbrella, re-exports all workspace crates; feature-gates `io`, `lofi`, `telemetry`, `osc`, `analog`, `sampler`

## History

> **«kama» → «rill» rename (0.3.0).** `kama-*` is the pre‑0.3.0 name of the framework. Every reference to `kama-*` should be read as `rill-*`. Crates `kama-automation` and `kama-control` were merged into `rill-patchbay`.
>
> Crate lineage:
> | Old name | New name |
> |---|---|
> | `kama-core` | `rill-core` |
> | `kama-graph` | `rill-graph` |
> | `kama-oscillators` | `rill-oscillators` |
> | `kama-digital-filters` | `rill-digital-filters` |
> | `kama-digital-effects` | `rill-digital-effects` |
> | `kama-eq` | merged into `rill-router::eq` |
> | `kama-mixer` | merged into `rill-router::mixer` |
> | `kama-automation` | merged into `rill-patchbay` |
> | `kama-control` | merged into `rill-patchbay` |
> | `kama-lofi` | `rill-lofi` |
> | `kama-io` | `rill-io` |

**`drift`** is a downstream live-coding effects server — demo application / proof-of-concept for `rill`. Depends on `rill-adrift`, uses `[patch.crates-io]` for dev mode.

## Commands

```bash
cargo test --workspace           # all tests
cargo test -p <crate>            # single crate
cargo clippy --workspace         # lint
cargo fmt                        # format (max_width=100, tab_spaces=4)

# publish order (leaf to root):
./scripts/publish.sh              # all 18 crates to crates.io
./scripts/publish.sh --check      # dry-run

## crates.io publication rules

- **Order:** leaf → root (see `scripts/publish.sh` for exact sequence).
- **Burst limit:** publish **no more than 5 crates consecutively**, then wait **≥10 minutes**.
- **Error-driven pause:** if crates.io responds with `429 Too Many Requests`,
  wait **≥10 minutes** before the next attempt, even if fewer than 5 have been published.
- **Rate limit cooldown:** between individual publishes, wait **30 seconds** for
  the index to update (leaf crates) or **10 minutes** (dependent crates, to avoid
  index staleness errors).

The `scripts/publish.sh` script implements all of these rules automatically.

# documentation site (mdBook):
mdbook build docs/                # build site to docs/book/
mdbook serve docs/                # dev server at localhost:3000
```

## Code conventions

- **Safety & Unsafe Policy:**
    - `#![deny(unsafe_code)]` set in 7 crates: `rill-core`, `rill-core-dsp`, `rill-graph`, `rill-core-wdf`, `rill-patchbay`, `rill-analog-filters`, `rill-analog-effects`.
    - **Always ask explicit permission before suggesting `unsafe`**, even in crates without the deny.
    - Prefer existing abstractions (buffers, SIMD wrappers) over raw pointer manipulation.
    - Architectural safety over micro-optimizations unless a bottleneck is proven.

- **Documentation language:**
    - All crate-level docs (`README.md`, module doc comments, API docs) must be in **English**.
    - Code comments (inline `//`) should also be in English.
    - The only exception is `docs/src/guides/world-of-automatons.md` — a full-fledged published article intentionally written in Russian as a deliberate stylistic choice.
    - The term **Automaton** is canonical in the codebase (`Automaton` trait, `AutomatonDef`, etc.). Do not use the alternative form "Automata" in code identifiers, documentation, or commit messages. In prose, prefer "automaton" (singular) / "automatons" (plural).
    - Rationale: English is the lingua franca of open-source. One Russian-language article is an exception, not a precedent — do not add more without explicit discussion.

- **Zero-copy data flow:**
    Data copying across node ports is **forbidden** except in these cases:
    1. **Branching (fan-out)** — one source feeds multiple destinations; the router copies.
    2. **Accumulation for delay / feedback** — delay lines, feedback loops.
    3. **External API boundary** — when the external API's buffer layout cannot be
       directly wrapped by a `Port<T, BUF_SIZE>` (e.g. PipeWire's byte-interleaved
       DMA buffers must be deinterleaved into mono port buffers).
    
    Source/Sink nodes **own the I/O buffer** — they must expose it directly as
    a `Port<T, BUF_SIZE>` output/input rather than copying through an intermediary.
    The graph engine routes port buffers by pointer; if data passes through a port,
    it should be read/written in place, not memcpy'd.
- **Dependencies:** 
    - Do not add new external crates to `Cargo.toml` without explicit confirmation.
    - Prefer internal workspace tools over bringing in new third-party dependencies.
- **Module Structure:** 
    - All public APIs must be re-exported via the `crate::prelude` module in each crate.
- **Doc tests:** use `no_run` (not `ignore`) on code blocks that illustrate API usage but are not self-contained runnable examples. `no_run` ensures the example compiles against the current API; `ignore` skips compilation entirely and lets examples rot.
- **Versioning:** crates version synchronously (all at 0.5.0-beta.4). Use `./scripts/publish.sh` to publish — it respects dependency order and handles crates.io rate-limiting.
- **Formatting & Quality:** 
    - Follow `max_width=100`, `tab_spaces=4`. 
    - Always run `cargo clippy --workspace` and fix all warnings before proposing a solution.

## Terminology: «audio» vs «signal» vs «I/O»

Rill is a **universal signal processing platform**, not exclusively audio. The term «audio» must only appear where hardware emitting or consuming sound is genuinely involved.

| Term | Use in | Rationale |
|---|---|---|
| **`audio`** | `rill-io`, `rill-lofi` | These crates deal with genuine audio hardware I/O (PortAudio/ALSA/PipeWire/JACK) and audio device emulation (NES, AY-3-8910). |
| **`signal`** | All other crates | Signal is the generic term for any discrete data stream — processing graph nodes, port connections, sample buffers, generators, filters, effects. |
| **`I/O`** (or `backend`) | When referring to `IoBackend` | `IoBackend` in `rill-core` is an **archetype** — «I do (some kind of) I/O». It applies to any discrete data stream (audio, sensor, telemetry, network), not just audio. The factory that constructs backends is the *I/O backend factory*, not *audio backend factory*. |
| **`signal thread`** (or `RT thread`) | Threading model descriptions | The processing thread runs the generic signal callback, not audio-specific logic. |

**Specific replacements:**

| ❌ Avoid | ✅ Use | Scope |
|---|---|---|
| `audio thread` | `signal thread` / `RT thread` / `I/O callback` | All non-`rill-io`/`rill-lofi` crates |
| `audio data` / `audio signal` / `audio block` | `signal data` / `signal` / `signal block` | All non-`rill-io`/`rill-lofi` crates |
| `audio port` / `audio buffer` / `audio connection` | `signal port` / `signal buffer` / `signal connection` | All non-`rill-io`/`rill-lofi` crates |
| `audio backend` (when meaning `IoBackend`) | `I/O backend` | `rill-core`, `rill-graph`, `rill-adrift` |
| `audio I/O thread` | `I/O callback thread` | Architecture docs |
| `Audio sample rate` (on generic types) | `Sample rate` | `rill-core` traits |
| `is_audio_rate()` | `is_signal_rate()` | `rill_core::PortType` |

**Concrete type names** (`AudioInput`, `AudioOutput`, `AudioConfig` in `rill-io`, `PortAudio`) are **exempt** — they are code identifiers, not prose. Renaming them requires a separate API-breaking change.

**«Hearing» / acoustic sensors** — the `hearing` module name and «acoustic» are domain-level concepts. Doc comments describing signal analysis algorithms should use «signal» (not «audio») for the generic processing path.

## Real-time safety

### Two backend models

The signal graph runs wherever the `AudioIo` process callback fires. The
constraints depend on the backend model:

| Model | Backends | RT guarantee |
|---|---|---|
| **Callback‑driven** | PipeWire, JACK, PortAudio | Hard RT — callback fires on the audio device's real‑time thread. No syscalls, no allocation, no locks. |
| **Poll‑driven** | ALSA | Soft RT — the backend's own thread loops polling the audio device. The thread **must not** use `thread::sleep()` to pace iterations. Use `poll()` / `epoll()` on audio FDs instead. |

### Rules for the RT path (applies to both models)

Any code reached from the process callback — `generate()`, `process()`,
`consume()`, `propagate()`, and everything they call — **must** obey:

| Rule | Rationale |
|---|---|
| **No heap allocation in RT path** | `Vec::new()`, `Box::new()`, `format!()` inside `propagate`/`generate`/`process`/`consume` will cause xruns. All buffers must be stack-allocated or pre-allocated at graph construction. |
| **No locks in RT path** | `Mutex::lock()`, `RwLock::write()` (even parking_lot) may spin. Communication with the control thread uses only `rill_core::queues::MpscQueue` (lock‑free SPSC). |
| **No `thread::sleep()` in RT path** | `thread::sleep()` is a syscall — it blocks the calling thread, introduces timing jitter, and makes deterministic scheduling impossible. Even in poll‑driven backends (ALSA, CPAL) the processing loop must wait on audio FDs (`poll`/`epoll`), not on `sleep`. |
| **No file I/O, no socket I/O in RT path** | Any syscall (open, read, write, send, recv) can block unpredictably. |
| **`downstream_nodes` is pre‑filled** | `Port::downstream_nodes` is populated once by `GraphBuilder::build()` and iterated at runtime without deduplication or allocation. |
| **Fixed‑size stack buffers** | Backend callbacks must use `[f32; MAX_BLOCK_SAMPLES]` (512) instead of `vec![]`. |

**Allowed exceptions:**
- `MpscQueue::pop()` — lock‑free atomic, OK on RT.
- `AtomicU32::fetch_add()` / `AtomicBool::store()` — OK on RT.
- Raw pointer dereference (`*mut`, `*const`) — single‑threaded DAG, guaranteed valid.
- `IoRingBuffer::read()` / `write()` — lock‑free atomic SPSC, OK on RT (used inside backends only, not in graph nodes).

### Known issues

*(All originally identified RT-safety issues have been fixed — ALSA uses
`snd_pcm_wait`, PortAudio drives processing from its stream callback, and
no backend uses `thread::sleep` in the signal path.)*

**Testing:** any new RT path code must be verified with `cargo test --release`
under `pw‑loopback` or similar virtual device to detect xruns.

## Feature flags (non-default)

- `rill-core-dsp`: `simd`, `f64`, `fast_math`
- `rill-digital-effects`: `modulation` (enables `rill-oscillators`)
- `rill-core`: `serde`, `simd` (enables `wide` crate)
- `rill-core-wdf`: (no non-default features)
- `rill-io`: `portaudio` (default), `midir` (default), `alsa`, `pipewire`, `jack`, `all-backends` (includes `midir`)
- `rill-sampler`: `wav` (default, enables `hound`)
- `rill-adrift`: `io`, `lofi`, `telemetry`, `osc`, `sampler`, `serialization`, `portaudio` (default); `analog`, `midi`, `dot` (opt-in); `alsa`, `portaudio`, `jack`, `pipewire` (backends, forward to `rill-io`)

## Branching

[git-flow](https://github.com/petervanderdoes/gitflow-avh) workflow via the `git-flow` CLI plugin.

| Branch pattern | Purpose |
|---|---|
| `main` | Stable releases |
| `develop` | Integration branch |
| `feature/*` | New features (branch off `develop`, merge back) |
| `release/*` | Release candidates |
| `hotfix/*` | Urgent fixes (branch off `main`, merge back to both) |

Conventional commits: `<type>(<scope>): <description>`.
Start a feature branch: `git flow feature start <name>`.

> **Commit messages with backticks:** always use **single quotes** (`'...'`) for
> `git commit -m`, never double quotes. In double quotes the shell interprets
> backticks as command substitution (`\`cmd\`` → runs `cmd`), which silently
> corrupts the message and may execute arbitrary text.

> **Before any work:** if the current branch is `develop`, you **must** create a
> `feature/*` branch first (`git flow feature start <name>`). Directly editing
> `develop` is not allowed.

> **`master`** is write-protected at the Git level — no direct commits, no
> `feature/*` merges. Only `release/*` and `hotfix/*` branches (handled by
> `git flow release finish` / `git flow hotfix finish`) touch `master`.

**Enforcement layers:**

| Layer | What it protects | How |
|---|---|---|
| Convention (`AGENTS.md`) | develop, master | Rule above |
| Pre-commit hook | develop, master | Rejects `git commit` on protected branches. Install once: `ln -s ../../scripts/pre-commit .git/hooks/pre-commit` |
| GitHub branch rules (optional) | develop, master | Require PR + status checks in repo settings |

To create the hook manually:
```bash
cat > .git/hooks/pre-commit << 'HOOK'
#!/usr/bin/env bash
branch=$(git symbolic-ref --short HEAD 2>/dev/null)
if [ "$branch" = "develop" ] || [ "$branch" = "master" ]; then
    echo "ERROR: Direct commits to $branch are not allowed."
    echo "Create a feature/hotfix/release branch first:"
    echo "  git flow feature start <name>"
    exit 1
fi
HOOK
chmod +x .git/hooks/pre-commit
```

## Threading model

### I/O callback — where the signal graph runs

The I/O backend (`IoBackend`) drives processing through an `ActiveNode` (typically
`Input<T,BUF_SIZE>`). The lifecycle is defined by the `IoBackend` trait
(`rill_core/src/io.rs:26`):

1. `set_process_callback()` — registers the graph tick closure
2. `run(running: Arc<AtomicBool>)` — enters the I/O loop; blocks for poll-driven
   backends (ALSA), returns immediately for callback-driven ones (PortAudio/JACK/PipeWire)
3. `stop()` — tears down

The nature of the callback thread depends on the backend:

| Model | Backends | RT guarantee |
|---|---|---|
| **Callback‑driven** | PipeWire, JACK, PortAudio | Hard RT — callback fires on the audio device's real‑time thread (SCHED_FIFO). No syscalls, no allocation, no locks. |
| **Poll‑driven** | ALSA | Soft RT — the backend's own thread loops polling the audio device. Use `poll()`/`epoll()` on audio FDs, never `thread::sleep()`. |

Inside the I/O callback tick (`rill-graph/src/graph.rs:556-588`):

1. `actor.drain()` — applies queued `CommandEnum::SetParameter` commands from the actor mailbox
2. `Source::generate()` / `Processor::process()` / `Sink::consume()` via `process_block()`
3. `Port::propagate()` — recursive DAG traversal through direct port pointers
4. Sends `CommandEnum::ClockTick` to the parent Patchbay actor

All `rill-core::buffer` types (`DelayLine`, `TapeLoop`, `PipeBuffer`,
`RingBuffer`, `FanOutBuffer`, `FanInBuffer`) are used **exclusively** inside
this path. No atomics, no locks — the graph is a single-threaded static DAG.

> **Backward compat:** `AudioIo` (`rill-io/src/audio_io.rs`) is a type alias
> for `dyn IoBackend<f32>`, kept for legacy code. `AudioInput`/`AudioOutput`
> are aliases for `Input`/`Output`. The `ActiveNode` trait is the real driver.

### Control path (soft RT)

Communication between the control thread and the I/O callback uses the
**actor mailbox** — messages are `CommandEnum` variants, sent via `ActorRef<CommandEnum>`
and drained inline inside the callback tick. No separate queue types are needed.

`rill-patchbay` runs control actors:

| Component | Spawn mechanism | Trigger | Output |
|---|---|---|---|
| **Servo\<A: Automaton\>** | `system.spawn_detached_tokio()` | Receives `CommandEnum::ClockTick` via actor mailbox | Sends `CommandEnum::SetParameter` to `graph_ref` |
| **LFO / Envelope (green thread)** | `tokio::spawn` (via `automaton_task.rs`) | `tokio::time::interval` | `mpsc::Sender<f64>` |
| **MIDI / Sensors** | OS thread (`MidiHub`) | `MidiBackend::poll()` | `ActorRef<ControlEvent>` → Patchbay::event_mailbox |

**Servo** (`rill-patchbay/src/engine.rs:463`) — the primary automaton-to-parameter bridge.
On each `CommandEnum::ClockTick` received from the graph:
1. Advances time: `state.time += dt`
2. Calls `automaton.step(&mut internal, &current_value, current_time, &action)` — signature is `(internal, current, time, action)`
3. Applies `ControlStrategy` (Absolute / Modulation) and `ConflictStrategy` (TouchOverride / BasePlusModulation / LastWriteWins) — defined in `rill-patchbay/src/strategy.rs`
4. Sends `CommandEnum::SetParameter` to the graph's `ActorRef<CommandEnum>`
5. The `SetParameter` lands in the graph's actor mailbox; next I/O callback tick, `actor.drain()` applies it

UI events reach the Servo as `CommandEnum::Automaton(AutomatonCommand::UiValue{..})`
or `UiRelease{..}` — no separate `UiCommand` channel exists.

**Automaton task** (`rill-patchbay/src/automaton_task.rs`) — an opt-in green thread
for running an automaton independently of the graph clock, using
`tokio::sync::mpsc::Sender<f64>` and `tokio::sync::watch` for cancellation.
(`PortCombiner` was removed — it duplicated `CommandEnum`'s role.)

### Communication channels

```
I/O callback tick:                     Actor mailbox (CommandEnum):
  actor.drain()  ◄──────────  SetParameter (servo → graph)
  generate() / process() / consume()
  port.propagate()                    Control path:
  ── ClockTick ──→ Servo ──→ automaton.step()
                             ── SetParameter ──→ graph_ref (next tick drain)
```

All control → signal communication uses `ActorRef<CommandEnum>` and the actor
mailbox (lock-free SPSC) — no blocking of the real-time signal thread.

### Cancellation

**Main Servo path:** `ServoState` (`Mutex<ServoState>`) has an `enabled: bool` field,
toggled via `CommandEnum::Automaton(AutomatonCommand::SetEnabled{..})`. When `false`,
the Servo skips processing on ClockTick. The tokio task exits when the actor system
drops — no explicit `watch` cancellation needed.

**Automaton task path** (`automaton_task.rs`): uses `tokio::sync::watch::Receiver<bool>`
for per-task cancellation — sending `true` causes the automaton loop to exit.

Sensors (MIDI, OSC) are stopped via their `Sensor::stop()` method, which
joins the polling thread and releases the backend.

### Rule of thumb

If data crosses threads, send `CommandEnum` variants through `ActorRef<CommandEnum>`.
Everything else is single-threaded within the signal graph running inside the
I/O callback. No external engine loop — `Port::propagate` traverses the DAG
recursively through direct port pointers.

## Known pitfalls

- Root `examples/` were **stale** and have been removed. Use per-crate `examples/` for canonical usage.
- No CI workflows exist.
- Integration tests live in per-crate `tests/` directories, not a dedicated `rill-tests` crate.
- `rill-adrift` is the recommended entry point for external apps. Use `rill-adrift::rill_core` etc. to access individual crates through it.
- **Two-thread architecture**: the I/O callback runs the graph via `ActiveNode::run()`,
  driving `generate()` / `process()` / `consume()` / `propagate()`. The control thread
  (soft RT) runs `rill-patchbay` actors (Servos, Sensors). Communication via
  `ActorRef<CommandEnum>`.
- `automaton_task.rs` originally referenced `PortCombiner` which was removed
  (duplicated `CommandEnum` functionality). The module works standalone
  (proven by tests) and can send values through any `mpsc::Sender<f64>`.

## Licensing

- **All workspace crates** — Apache 2.0 (see `LICENSE.md`).
- **Examples** (`examples/` in each crate) — MIT (see `LICENSE-MIT`).
- Do not add new licenses without explicit discussion.
