# rill-adrift — AGENTS.md

Umbrella crate re-exporting all rill crates for signal processing application development. Owns the domain `rill-adrift.io`.

## Design

- **Always-on core** (no feature gate): `rill-core`, `rill-core-actor`, `rill-core-dsp`, `rill-graph`, `rill-digital-effects`, `rill-router`, `rill-patchbay`, `rill-lang`
- **Feature-gated**: `io`, `lofi`, `telemetry`, `osc`, `sampler`, `fft` (all in default), `analog`
- **I/O backend passthrough**: `alsa`, `portaudio`, `jack`, `pipewire` forward to `rill-io`

## Usage

```rust
use rill_adrift::prelude::*;
use rill_adrift::rill_core_dsp::generators::SineOscillator;
```

## Commands

```bash
cargo test -p rill-adrift
cargo clippy -p rill-adrift
```

## Known issues

- Feature `analog` enables `rill-core-model` (WDF algorithms) + the `rill-lang/model` feature (analog_moog factory). The former `rill-analog-filters`/`rill-analog-effects` crates were folded into `rill-core-model` in SP-3b.
- Backend features (`alsa`, `portaudio`, etc.) only work when `io` feature is also enabled.
