# Crates

The Rill workspace consists of 18 crates, all versioned synchronously.

| Crate | Version | Description | Docs |
|-------|---------|-------------|------|
| **rill-adrift** | 0.5.0-beta.7 | Umbrella crate — re-exports all workspace crates | [docs.rs](https://docs.rs/rill-adrift) |
| **rill-core** | 0.5.0-beta.7 | Core traits, math, buffers, queues, time, macros, interpolation | [docs.rs](https://docs.rs/rill-core) |
| **rill-core-dsp** | 0.5.0-beta.7 | DSP algorithms, vector ops, filters, generators, sample player | [docs.rs](https://docs.rs/rill-core-dsp) |
| **rill-core-model** | 0.5.0-beta.7 | WDF core + physical modeling — string, plate, modal, cavity | [docs.rs](https://docs.rs/rill-core-model) |
| **rill-graph** | 0.5.0-beta.7 | Static DAG signal graph with topological sort | [docs.rs](https://docs.rs/rill-graph) |
| **rill-oscillators** | 0.5.0-beta.7 | Sine, saw, noise, LFO, envelope, wavetable nodes | [docs.rs](https://docs.rs/rill-oscillators) |
| **rill-digital-filters** | 0.5.0-beta.7 | Biquad, SVF, Comb, MoogLadder filter nodes | [docs.rs](https://docs.rs/rill-digital-filters) |
| **rill-digital-effects** | 0.5.0-beta.7 | Delay, Distortion, Limiter nodes | [docs.rs](https://docs.rs/rill-digital-effects) |
| **rill-router** | 0.5.0-beta.7 | EQ (graphic, parametric) + mixer (channels, sends, master) | [docs.rs](https://docs.rs/rill-router) |
| **rill-patchbay** | 0.5.0-beta.7 | Automation — LFO, envelopes, sensors, servos, mappings | [docs.rs](https://docs.rs/rill-patchbay) |
| **rill-lofi** | 0.5.0-beta.7 | Lo-fi emulation — NES, AY-3-8910, Akai S900 | [docs.rs](https://docs.rs/rill-lofi) |
| **rill-io** | 0.5.0-beta.7 | Audio I/O — PortAudio, ALSA, PipeWire, JACK backends | [docs.rs](https://docs.rs/rill-io) |
| **rill-telemetry** | 0.5.0-beta.7 | Probes, collectors, real-time monitoring | [docs.rs](https://docs.rs/rill-telemetry) |
| **rill-analog-filters** | 0.5.0-beta.7 | WDF-based analog filters — WdfMoogLadder | [docs.rs](https://docs.rs/rill-analog-filters) |
| **rill-analog-effects** | 0.5.0-beta.7 | Analog circuit models — op-amp, tape deck, preamps | [docs.rs](https://docs.rs/rill-analog-effects) |
| **rill-osc** | 0.5.0-beta.7 | OSC — UDP server, encode/decode, pattern dispatch; parsing backend for `rill-patchbay::osc::OscSensor` | [docs.rs](https://docs.rs/rill-osc) |
| **rill-sampler** | 0.5.0-beta.7 | Sample playback + time-series reader + WAV loading | [docs.rs](https://docs.rs/rill-sampler) |

## Feature flags

| Crate | Features                                                                                      |
|-------|----------|
| `rill-core` | `serde`, `simd`                                                                               |
| `rill-core-dsp` | `simd`, `f64`, `fast_math`                                                                     |
| `rill-core-model` | (no non-default features)                                                                     |
| `rill-digital-effects` | `modulation` (enables `rill-oscillators`)                                                     |
| `rill-graph` | `serialization`                                                                               |
| `rill-patchbay` | `serde`, `json`, `cbor`, `serialization`, `midi` (MIDI input), `osc` (OSC input) |
| `rill-io` | `portaudio` (default), `midir` (default), `alsa`, `pipewire`, `jack`, `all-backends`        |
| `rill-sampler` | `wav` (default, enables `hound`)                                                              |
| `rill-adrift` | `io`, `lofi`, `telemetry`, `osc`, `sampler`, `portaudio` (default); `analog`, `midi`, `alsa`, `jack`, `pipewire` (opt-in) |
