# Rill — AGENTS.md

## Workspace layout

Cargo workspace — 17 active crates:

| Crate | Status |
|---|---|
| `rill-core` | Active — base traits, math, buffers, queues, time, macros, executor, interpolation |
| `rill-core-dsp` | Active — DSP algorithm trait, filters, generators, delay, vector ops, sample player |
| `rill-graph` | Active — audio graph with topological sort |
| `rill-oscillators` | Active — oscillators, LFO, envelopes, wavetable oscillator node |
| `rill-digital-filters` | Active — Biquad, SVF, comb, MoogLadder filters |
| `rill-digital-effects` | Active — Delay, Distortion, Limiter |
| `rill-router` | Active — EQ + mixer + routing |
| `rill-patchbay` | Active — automation (LFO, envelopes, sensors, servos) |
| `rill-lofi` | Active — lo-fi emulation |
| `rill-io` | Active — audio I/O backends (ALSA, CPAL, PipeWire, JACK) |
| `rill-telemetry` | Active — probes, collectors |
| `rill-core-wdf` | Active — WDF elements, adapters, analysis |
| `rill-analog-filters` | Active — WDF-based analog filters (WdfMoogLadder) |
| `rill-analog-effects` | Active — op-amp, tape deck, preamp models |
| `rill-osc` | Active — OSC server and networking |
| `rill-sampler` | Active — sample playback, time-series reader, WAV loading |
| `rill-adrift` | Active — umbrella crate for audio applications |

Dependency tree:
- **`rill-core`** — foundation (depended on by all crates except `rill-osc`)
- **`rill-core-dsp`** — DSP algorithms (depends on `rill-core`)
- **`rill-graph`** — audio graph, depends on `rill-core` only (no DSP dependency). Contains `AudioEngine` — real-time safe graph engine with `process_tick()`, `process_block()`, and `spawn()`.
- **`rill-io`** — audio I/O backends only (`AudioBackend` trait + ALSA/CPAL/JACK/PipeWire). No engine, no processors. `rill-graph::AudioEngine` drives the graph in the I/O callback.
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
- **Dependencies:** 
    - Do not add new external crates to `Cargo.toml` without explicit confirmation.
    - Prefer internal workspace tools over bringing in new third-party dependencies.
- **Module Structure:** 
    - All public APIs must be re-exported via the `crate::prelude` module in each crate.
- **Versioning:** crates version synchronously (all at 0.3.0). Use `./scripts/publish.sh` to publish — it respects dependency order and handles crates.io rate-limiting.
- **Formatting & Quality:** 
    - Follow `max_width=100`, `tab_spaces=4`. 
    - Always run `cargo clippy --workspace` and fix all warnings before proposing a solution.

## Feature flags (non-default)

- `rill-core-dsp`: `simd`, `f64`, `fast_math`, `unstable`
- `rill-digital-effects`: `modulation` (enables `rill-oscillators`)
- `rill-core`: `serde`, `stats`
- `rill-core-wdf`: `simd`
- `rill-io`: `cpal` (default), `alsa`, `pipewire`, `jack`, `all-backends`
- `rill-sampler`: `wav` (default, enables `hound`)
- `rill-adrift`: `io`, `lofi`, `telemetry`, `osc`, `sampler` (default), `analog` (opt-in); `alsa`, `cpal`, `jack`, `pipewire` (backends, forward to `rill-io`)

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

## Known pitfalls

- Root `examples/` were **stale** and have been removed. Use per-crate `examples/` for canonical usage.
- README prose about "Мир автоматов" (patchbay) describes an active subsystem, but code examples may be aspirational.
- No CI workflows or pre-commit hooks exist.
- Integration tests live in per-crate `tests/` directories, not a dedicated `rill-tests` crate.
- `rill-adrift` is the recommended entry point for external apps. Use `rill-adrift::rill_core` etc. to access individual crates through it.
- **Two-thread architecture**: `rill-graph::AudioEngine` runs on the audio thread (hard RT), `rill-patchbay::PatchbayManager` runs on the control thread (soft RT). Communication via `CommandQueue`/`TelemetryQueue`. Source/Sink nodes own I/O buffers — the engine only orchestrates.
