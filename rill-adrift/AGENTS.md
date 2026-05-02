# rill-adrift — AGENTS.md

Umbrella crate re-exporting all rill crates for signal processing application development. Owns the domain `rill-adrift.io`.

## Design

- **Always-on core** (no feature gate): `rill-core`, `rill-core-dsp`, `rill-graph`, `rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`, `rill-router`, `rill-patchbay`
- **Feature-gated**: `io`, `lofi`, `telemetry`, `osc`, `sampler` (all in default), `analog` (opt-in)
- **Audio backend passthrough**: `alsa`, `cpal`, `jack`, `pipewire` forward to `rill-io`

## Usage

```rust
use rill_adrift::prelude::*;
use rill_adrift::rill_oscillators::audio::SineOsc;
```

## Commands

```bash
cargo test -p rill-adrift
cargo clippy -p rill-adrift
```

## Known issues

- Feature `analog` enables three crates at once: `rill-core-wdf`, `rill-analog-filters`, `rill-analog-effects`.
- Backend features (`alsa`, `cpal`, etc.) only work when `io` feature is also enabled.
