# Crates

The Rill workspace consists of 17 crates, all versioned synchronously.

| Crate | Version | Description | Docs |
|-------|---------|-------------|------|
| **rill-adrift** | 0.4.0 | Umbrella crate — re-exports all workspace crates | [docs.rs](https://docs.rs/rill-adrift) |
| **rill-core** | 0.4.0 | Core traits, math, buffers, queues, time, macros, interpolation | [docs.rs](https://docs.rs/rill-core) |
| **rill-core-dsp** | 0.4.0 | DSP algorithms, vector ops, filters, generators, sample player | [docs.rs](https://docs.rs/rill-core-dsp) |
| **rill-core-wdf** | 0.4.0 | Wave Digital Filter core — elements, adapters, analysis | [docs.rs](https://docs.rs/rill-core-wdf) |
| **rill-graph** | 0.4.0 | Static DAG signal graph with topological sort | [docs.rs](https://docs.rs/rill-graph) |
| **rill-oscillators** | 0.4.0 | Sine, saw, noise, LFO, envelope, wavetable nodes | [docs.rs](https://docs.rs/rill-oscillators) |
| **rill-digital-filters** | 0.4.0 | Biquad, SVF, Comb, MoogLadder filter nodes | [docs.rs](https://docs.rs/rill-digital-filters) |
| **rill-digital-effects** | 0.4.0 | Delay, Distortion, Limiter nodes | [docs.rs](https://docs.rs/rill-digital-effects) |
| **rill-router** | 0.4.0 | EQ (graphic, parametric) + mixer (channels, sends, master) | [docs.rs](https://docs.rs/rill-router) |
| **rill-patchbay** | 0.4.0 | Automation — LFO, envelopes, sensors, servos, mappings | [docs.rs](https://docs.rs/rill-patchbay) |
| **rill-lofi** | 0.4.0 | Lo-fi emulation — NES, AY-3-8910, Akai S900 | [docs.rs](https://docs.rs/rill-lofi) |
| **rill-io** | 0.4.0 | Audio I/O — ALSA, CPAL, PipeWire, JACK backends | [docs.rs](https://docs.rs/rill-io) |
| **rill-telemetry** | 0.4.0 | Probes, collectors, real-time monitoring | [docs.rs](https://docs.rs/rill-telemetry) |
| **rill-analog-filters** | 0.4.0 | WDF-based analog filters — WdfMoogLadder | [docs.rs](https://docs.rs/rill-analog-filters) |
| **rill-analog-effects** | 0.4.0 | Analog circuit models — op-amp, tape deck, preamps | [docs.rs](https://docs.rs/rill-analog-effects) |
| **rill-osc** | 0.4.0 | OSC server — UDP, encode/decode, pattern dispatch | [docs.rs](https://docs.rs/rill-osc) |
| **rill-sampler** | 0.4.0 | Sample playback + time-series reader + WAV loading | [docs.rs](https://docs.rs/rill-sampler) |

## Feature flags

| Crate | Features |
|-------|----------|
| `rill-core` | `serde`, `simd` |
| `rill-core-dsp` | `simd`, `f64`, `fast_math`, `unstable` |
| `rill-core-wdf` | `simd` |
| `rill-digital-effects` | `modulation` (enables `rill-oscillators`) |
| `rill-graph` | `serialization`, `dot` |
| `rill-patchbay` | `serde`, `json`, `cbor`, `serialization` |
| `rill-io` | `cpal` (default), `alsa`, `pipewire`, `jack`, `all-backends` |
| `rill-sampler` | `wav` (default, enables `hound`) |
| `rill-adrift` | `io`, `lofi`, `telemetry`, `osc`, `sampler` (default); `analog` (opt-in); backend passthrough |
