# Rill — AGENTS.md

## Workspace layout

Cargo workspace — 7 active crates, several disabled/planned:

| Crate | Status |
|---|---|
| `rill-core` | Active — base traits, math, buffers, queues, time, macros, executor |
| `rill-core-dsp` | Active — DSP algorithm trait, filters, generators, delay, vector ops |
| `rill-graph` | Active — audio graph with topological sort |
| `rill-oscillators` | Active — oscillators, LFO, envelopes |
| `rill-digital-filters` | Active — Biquad, SVF, comb filters |
| `rill-digital-effects` | Active — Delay, Distortion, Limiter |
| `rill-router` | Active — EQ + mixer (version 0.3.0) |
| `rill-patchbay` | Disabled (commented out of workspace) |
| `rill-lofi` | Disabled |
| `rill-io` | Disabled |
| `rill-wdf` / `rill-server` | Planned, not in workspace |

Dependency tree:
- **`rill-core`** — foundation, depended on by all other crates
- **`rill-core-dsp`** — DSP algorithms (depends on `rill-core`)

  Consumer crates that depend on both `rill-core` AND `rill-core-dsp`:
  `rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`, `rill-router`
- **`rill-graph`** — audio graph, depends on `rill-core` only (no DSP dependency)

## Commands

```bash
cargo test --workspace           # all tests
cargo test -p <crate>            # single crate
cargo clippy --workspace         # lint
cargo fmt                        # format (max_width=100, tab_spaces=4)
./scripts/bump-version.sh <ver>  # bump all crates in sync
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
- **Versioning & Workspace:** 
    - Crates must stay in version lockstep. 
    - **Never** manually edit `version` fields in `Cargo.toml`. Use `./scripts/bump-version.sh`.
- **Formatting & Quality:** 
    - Follow `max_width=100`, `tab_spaces=4`. 
    - Always run `cargo clippy --workspace` and fix all warnings before proposing a solution.

## Feature flags (non-default)

- `rill-core-dsp`: `simd` (needs `wide` crate), `f64`, `fast_math`, `unstable`
- `rill-digital-effects`: `modulation` (enables `rill-oscillators`)
- `rill-core`: `serde`, `stats`

## Branching

Git Flow: `main` (stable), `develop` (integration), `feature/*`, `release/*`, `hotfix/*`.
Conventional commits: `<type>(<scope>): <description>`.

## Known pitfalls

- `examples/` are **stale** — they reference removed APIs (`rill_core::dsp`, `rilldelay`). Do not trust as canonical.
- README prose about "Мир автоматов" (patchbay) describes a **disabled** subsystem. Skip that section.
- No CI workflows or pre-commit hooks exist.
- `rill-tests` integration test crate is planned but not yet created.
