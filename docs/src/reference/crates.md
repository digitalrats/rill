# Crates

The Rill workspace consists of 20 crates, all versioned synchronously.

| Crate | Version | Description | Docs |
|-------|---------|-------------|------|
| **rill-adrift** | 0.5.0 | Umbrella crate — re-exports all workspace crates; `lang` feature replaces `register_all_nodes()` with rill-lang builtin registries via `full_registry()` / `full_registry_f32()` | [docs.rs](https://docs.rs/rill-adrift) |
| **rill-core** | 0.5.0 | Core traits, math, buffers, queues, time, macros, interpolation; `builtin` module (Registry of lang built-ins), `MultichannelAlgorithm` and `BridgeAlgorithm` traits | [docs.rs](https://docs.rs/rill-core) |
| **rill-core-actor** | 0.5.0 | Actor model — ActorRef, Actor, ActorSystem for lock-free message passing | [docs.rs](https://docs.rs/rill-core-actor) |
| **rill-core-dsp** | 0.5.0 | DSP algorithms, vector ops, filters, generators, sample player | [docs.rs](https://docs.rs/rill-core-dsp) |
| **rill-core-model** | 0.5.0 | WDF core + physical modeling — string, plate, modal, cavity | [docs.rs](https://docs.rs/rill-core-model) |
| **rill-graph** | 0.5.0 | Static DAG signal graph with topological sort; optional `lang` feature enables `build_graph_ir()` path that bridges to `rill-lang::graph_ir::GraphIr` | [docs.rs](https://docs.rs/rill-graph) |
| **rill-oscillators** | 0.5.0 | Sine, saw, noise, LFO, envelope, wavetable nodes | [docs.rs](https://docs.rs/rill-oscillators) |
| **rill-digital-filters** | 0.5.0 | Biquad, SVF, Comb, MoogLadder filter nodes | [docs.rs](https://docs.rs/rill-digital-filters) |
| **rill-digital-effects** | 0.5.0 | Delay, Distortion, Limiter nodes | [docs.rs](https://docs.rs/rill-digital-effects) |
| **rill-router** | 0.5.0 | EQ (graphic, parametric) + mixer (channels, sends, master) | [docs.rs](https://docs.rs/rill-router) |
| **rill-fft** | 0.5.0 | Radix-2 FFT, frequency‑domain convolution, spectrum analysis, spectral effects | [docs.rs](https://docs.rs/rill-fft) |
| **rill-patchbay** | 0.5.0 | Automation — LFO, envelopes, sensors, servos, mappings | [docs.rs](https://docs.rs/rill-patchbay) |
| **rill-lofi** | 0.5.0 | Lo-fi emulation — NES, AY-3-8910, Akai S900 | [docs.rs](https://docs.rs/rill-lofi) |
| **rill-io** | 0.5.0 | Audio I/O — PortAudio, ALSA, PipeWire, JACK backends | [docs.rs](https://docs.rs/rill-io) |
| **rill-telemetry** | 0.5.0 | Probes, collectors, real-time monitoring | [docs.rs](https://docs.rs/rill-telemetry) |
| **rill-analog-filters** | 0.5.0 | WDF-based analog filters — WdfMoogLadder | [docs.rs](https://docs.rs/rill-analog-filters) |
| **rill-analog-effects** | 0.5.0 | Analog circuit models — op-amp, tape deck, preamps | [docs.rs](https://docs.rs/rill-analog-effects) |
| **rill-osc** | 0.5.0 | OSC — UDP server, encode/decode, pattern dispatch | [docs.rs](https://docs.rs/rill-osc) |
| **rill-sampler** | 0.5.0 | Sample playback + time-series reader + WAV loading | [docs.rs](https://docs.rs/rill-sampler) |
| **rill-lang** | 0.5.0 | Faust-style functional signal DSL; compiles to `Algorithm<T>` or `MultichannelAlgorithm<T>` via `compile()`/`compile_with()`, or to a full `RillGraphEngine` via `compile_graph()` with runtime `?name` parameter support | [docs.rs](https://docs.rs/rill-lang) |

## Feature flags

| Crate | Features |
|-------|----------|
| `rill-core` | `serde`, `simd` |
| `rill-core-dsp` | `simd`, `f64`, `fast_math` |
| `rill-core-model` | (no non-default features) |
| `rill-digital-effects` | `modulation` (enables `rill-oscillators`) |
| `rill-fft` | `simd`, `f64` |
| `rill-graph` | `serialization` |
| `rill-lang` | `serde` |
| `rill-patchbay` | `serde`, `json`, `cbor`, `serialization`, `midi` (MIDI input), `osc` (OSC input) |
| `rill-io` | `portaudio` (default), `midir` (default), `alsa`, `pipewire`, `jack`, `all-backends` |
| `rill-sampler` | `wav` (default, enables `hound`) |
| `rill-adrift` | `io`, `lofi`, `telemetry`, `osc`, `sampler`, `fft`, `portaudio`, `serialization` (default); `analog`, `midi`, `alsa`, `jack`, `pipewire`, `lang` (opt-in) |
