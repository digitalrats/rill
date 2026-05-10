# Rill — AGENTS.md

## Workspace layout

Cargo workspace — 17 active crates:

| Crate | Status |
|---|---|
| `rill-core` | Active — base traits, math, buffers, queues, time, macros, executor, interpolation |
| `rill-core-dsp` | Active — DSP algorithm trait, filters, generators, delay, vector ops, sample player |
| `rill-graph` | Active — signal graph (DAG) with topological sort |
| `rill-oscillators` | Active — oscillators, LFO, envelopes, wavetable oscillator node |
| `rill-digital-filters` | Active — Biquad, SVF, comb, MoogLadder filters |
| `rill-digital-effects` | Active — Delay, Distortion, Limiter |
| `rill-router` | Active — EQ + mixer + routing |
| `rill-patchbay` | Active — automation (LFO, envelopes, sensors, servos) |
| `rill-lofi` | Active — lo-fi emulation |
| `rill-io` | Active — audio I/O backends (PortAudio, ALSA, PipeWire, JACK) |
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
- **`rill-io`** — audio I/O backends only (`IoBackend` trait + PortAudio/ALSA/PipeWire/JACK). No engine, no processors. `rill-graph::Port::propagate` drives the graph in the I/O callback.
- **`rill-osc`** — standalone crate (no internal workspace deps)

  Crates depending on both `rill-core` + `rill-core-dsp`:
  `rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`, `rill-router`
- **`rill-core-wdf`** — WDF core, depends on `rill-core`
- **`rill-analog-filters`** — depends on `rill-core` + `rill-core-wdf`
- **`rill-analog-effects`** — depends on `rill-core` + `rill-core-wdf`
- **`rill-sampler`** — graph nodes for sample playback and time-series reading; depends on `rill-core` + `rill-core-dsp`
- **`rill-adrift`** — umbrella, re-exports all workspace crates; feature-gates `io`, `lofi`, `telemetry`, `osc`, `analog`, `sampler`

## Commands

```bash
cargo test --workspace           # all tests
cargo test -p <crate>            # single crate
cargo clippy --workspace         # lint
cargo fmt                        # format (max_width=100, tab_spaces=4)

# publish order (leaf to root):
./scripts/publish.sh              # all 17 crates to crates.io
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
- **Versioning:** crates version synchronously (all at 0.4.0). Use `./scripts/publish.sh` to publish — it respects dependency order and handles crates.io rate-limiting.
- **Formatting & Quality:** 
    - Follow `max_width=100`, `tab_spaces=4`. 
    - Always run `cargo clippy --workspace` and fix all warnings before proposing a solution.

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
no backend uses `thread::sleep` in the audio path.)*

**Testing:** any new RT path code must be verified with `cargo test --release`
under `pw‑loopback` or similar virtual device to detect xruns.

## Feature flags (non-default)

- `rill-core-dsp`: `simd`, `f64`, `fast_math`, `unstable`
- `rill-digital-effects`: `modulation` (enables `rill-oscillators`)
- `rill-core`: `serde`, `stats`
- `rill-core-wdf`: `simd`
- `rill-io`: `portaudio` (default), `alsa`, `pipewire`, `jack`, `all-backends`
- `rill-sampler`: `wav` (default, enables `hound`)
- `rill-adrift`: `io`, `lofi`, `telemetry`, `osc`, `sampler` (default), `analog` (opt-in); `alsa`, `portaudio`, `jack`, `pipewire` (backends, forward to `rill-io`)

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

### Audio I/O thread (where the process callback runs)

The `AudioIo::start()` callback fires on the backend's own thread. The nature
of this thread depends on the backend:

- **PipeWire / JACK / PortAudio** — the callback fires on the audio device's real‑time
  thread (SCHED_FIFO). This is **hard RT**.
- **ALSA** — the callback fires on a dedicated polling thread managed
  by the backend. This is **soft RT**; `thread::sleep()` is **not** an
  acceptable wait primitive (see "Real-time safety" above).

In all cases the callback runs:

1. Drain `MpscQueue<ParameterCommand>` (parameter changes from control thread)
2. `Source::generate()` / `Processor::process()` / `Sink::consume()`
3. `Port::propagate()` — recursive DAG traversal through direct port pointers.

All `rill-core::buffer` types (`DelayLine`, `TapeLoop`, `PipeBuffer`,
`RingBuffer`, `FanOutBuffer`, `FanInBuffer`) are used **exclusively** inside
this path. No atomics, no locks — the graph is a single-threaded static DAG.

### Control thread (soft RT) — green threads

`rill-patchbay` runs four kinds of green threads (tokio tasks) for automation:

| Type | Spawn | Source | Output |
|---|---|---|---|
| **LFO / Envelope** | `tokio::spawn` | `tokio::time::interval` | `mpsc::Sender<f64>` → PortCombiner |
| **PortCombiner** | `tokio::spawn` | `mpsc::Receiver<f64>` + `mpsc::UnboundedReceiver<UiCommand>` | `MpscQueue<ParameterCommand>` |
| **Sequencer** | `tokio::task::spawn_blocking` | `crossbeam_channel::Receiver<Telemetry>` (CLOCK_TICK) | `MpscQueue<ParameterCommand>` |
| **Sync Servos** | (no thread, called inline) | `control.update(dt)` | `MpscQueue<ParameterCommand>` |

**LFO/Envelope Automaton** — spawned via `tokio::spawn`. On each `tokio::time::interval`
tick it calls `automaton.step(time, action, state)` and sends the resulting value
through an `mpsc::Sender<f64>` to its paired PortCombiner. Each automaton has its
own cancel channel (`watch::Receiver<bool>`).

**PortCombiner** — spawned via `tokio::spawn`. Sits between the automaton and the
audio thread. Uses `tokio::select!` to listen on three channels:
- `mpsc::Receiver<f64>` — values from the automaton
- `mpsc::UnboundedReceiver<UiCommand>` — UI/MIDI/OSC events from `handle_event()`
- `watch::Receiver<bool>` — cancellation signal

Applies `ControlStrategy` (Absolute / Modulation) and `ConflictStrategy`
(TouchOverride / BasePlusModulation / LastWriteWins) to resolve conflicts
between automaton and UI. Output: `MpscQueue<ParameterCommand>`.

**Sequencer** — spawned via `tokio::task::spawn_blocking` (uses blocking
`crossbeam_channel::Receiver::recv()`). Listens for `CLOCK_TICK` telemetry from
the audio thread through a `crossbeam_channel::Receiver<Telemetry>`. Each tick
contains `(sample_pos, sample_rate, tempo, beat_pos, is_new_beat, is_new_bar)`.
`SnapshotSequencer::tick_ext()` decides whether to advance to the next step and
returns `Vec<ParameterCommand>` which are pushed to `MpscQueue`. Controlled via
`SequencerHandle` (start/stop/reset/set_pattern) over a crossbeam channel.

### Communication channels

```
                               tokio / crossbeam                    MpscQueue
Automaton ──── mpsc<f64> ───→ PortCombiner ──── ParameterCommand ──→ Audio
UI/MIDI/OSC ── mpsc<UiCmd> ─→ PortCombiner ──── ParameterCommand ──→ Audio
Sequencer ◀─── crossbeam<Telemetry> (CLOCK_TICK) ◀──── Audio thread
Sequencer ──── ParameterCommand ──────────────────────────────────────→ Audio
```

All control → audio paths converge on `rill_core::queues::MpscQueue<ParameterCommand>`
— a lock-free SPSC queue designed for safe cross-thread communication without
blocking the real-time audio thread.

### Sharded cancellation

Each PortCombiner + automaton pair has an isolated `watch::Sender<bool>` /
`watch::Receiver<bool>` pair. `stop_all()` sends `true` on each sender, which
causes both the automaton loop and the combiner loop to exit. This per-port
cancellation domain means stopping one LFO doesn't affect others.

The sequencer is stopped via `JoinHandle::abort()` (or naturally when the
telemetry channel closes on audio thread shutdown).

### Rule of thumb

If data crosses threads, use `rill_core::queues::MpscQueue<ParameterCommand>`.
Everything else is single-threaded within the signal graph running inside the
I/O callback (see "Audio I/O thread" above). No external engine loop —
`Port::propagate` replaces `Port::propagate::process_block()`.

## Known pitfalls

- Root `examples/` were **stale** and have been removed. Use per-crate `examples/` for canonical usage.
- README prose about "Мир автоматов" (patchbay) describes an active subsystem, but code examples may be aspirational.
- No CI workflows or pre-commit hooks exist.
- Integration tests live in per-crate `tests/` directories, not a dedicated `rill-tests` crate.
- `rill-adrift` is the recommended entry point for external apps. Use `rill-adrift::rill_core` etc. to access individual crates through it.
- **Two-thread architecture**: the audio I/O thread (see "Audio I/O thread"
  above) runs `AudioInput`'s callback which drives the entire signal graph.
  The control thread (soft RT) runs `rill-patchbay::Manager`.
  Communication via `MpscQueue<ParameterCommand>`. Source/Sink nodes own
  I/O buffers — no external engine loop, `Port::propagate` replaces
  `Port::propagate::process_block`.

## Licensing

- **All workspace crates** — Apache 2.0 (see `LICENSE.md`).
- **Examples** (`examples/` in each crate) — MIT (see `LICENSE-MIT`).
- Do not add new licenses without explicit discussion.
