# Rill — AGENTS.md

## Workspace layout

Cargo workspace — 15 active crates:

| Crate | Status |
|---|---|
| `rill-core` | Active — base traits, math, buffers, queues, time, macros, executor |
| `rill-core-dsp` | Active — DSP algorithm trait, filters, generators, delay, vector ops |
| `rill-graph` | Active — audio graph with topological sort |
| `rill-oscillators` | Active — oscillators, LFO, envelopes |
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
| `rill-server` | Active — OSC server and networking |

Dependency tree:
- **`rill-core`** — foundation, depended on by all other crates except `rill-core-wdf`
- **`rill-core-dsp`** — DSP algorithms (depends on `rill-core`)

  Consumer crates that depend on both `rill-core` AND `rill-core-dsp`:
  `rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`, `rill-router`
- **`rill-graph`** — audio graph, depends on `rill-core` only (no DSP dependency)
- **`rill-core-wdf`** — WDF core, standalone (no `rill-core` dependency)
- **`rill-analog-filters`** — analog filters, depends on `rill-core` + `rill-core-wdf`
- **`rill-analog-effects`** — analog effects, depends on `rill-core` + `rill-core-wdf`

## Commands

```bash
cargo test --workspace           # all tests
cargo test -p <crate>            # single crate
cargo clippy --workspace         # lint
cargo fmt                        # format (max_width=100, tab_spaces=4)
```

## Code conventions

- **Safety & Unsafe Policy:**
    - Strictly respect `#![deny(unsafe_code)]` in `rill-core`, `rill-core-dsp`, and `rill-graph`.
    - **Always ask and obtain explicit user permission before suggesting ANY `unsafe` code**, even in crates where it is not denied.
    - Prioritize using existing abstractions from `rill-core` and `rill-core-dsp` (buffers, SIMD wrappers) over raw pointer manipulation or `unsafe` blocks. 
    - Architectural safety always takes precedence over micro-optimizations unless a bottleneck is proven.
- **Dependencies:** 
    - Do not add new external crates to `Cargo.toml` without explicit confirmation.
    - Prefer internal workspace tools over bringing in new third-party dependencies.
- **Module Structure:** 
    - All public APIs must be re-exported via the `crate::prelude` module in each crate.
- **Versioning (independent):** 
    - Each crate versions independently — only bump when it actually changes.
    - Core crates (`rill-core`, `rill-core-dsp`, `rill-core-wdf`) are independent of each other; a consumer crate's version reflects only its own changes, not the core's.
    - When bumping a crate, also update its `version` in `[workspace.dependencies]` in the root `Cargo.toml` so consumers resolve correctly.
    - **Do not use `./scripts/bump-version.sh`** — it is deprecated and kept only as a reference.
- **Formatting & Quality:** 
    - Follow `max_width=100`, `tab_spaces=4`. 
    - Always run `cargo clippy --workspace` and fix all warnings before proposing a solution.

## Feature flags (non-default)

- `rill-core-dsp`: `simd` (needs `wide` crate), `f64`, `fast_math`, `unstable`
- `rill-digital-effects`: `modulation` (enables `rill-oscillators`)
- `rill-core`: `serde`, `stats`
- `rill-core-wdf`: `simd`

## Branching

Git Flow: `main` (stable), `develop` (integration), `feature/*`, `release/*`, `hotfix/*`.
Conventional commits: `<type>(<scope>): <description>`.

## Known pitfalls

- Root `examples/` were **stale** and have been removed. Use per-crate `examples/` for canonical usage.
- README prose about "Мир автоматов" (patchbay) describes an active subsystem, but code examples may be aspirational.
- No CI workflows or pre-commit hooks exist.
- Integration tests live in per-crate `tests/` directories, not a dedicated `rill-tests` crate.
