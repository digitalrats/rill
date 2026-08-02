# Crates

The Rill workspace consists of 20 crates, all versioned synchronously.

| Crate | Version | Description | Docs |
|-------|---------|-------------|------|
| **rill-adrift** | 0.6.0-M2 | Umbrella crate — re-exports all workspace crates; `lang` feature replaces `register_all_nodes()` with rill-lang builtin registries via `full_registry()` / `full_registry_f32()` | [docs.rs](https://docs.rs/rill-adrift) |
| **rill-core** | 0.6.0-M2 | Core traits, math, buffers, queues, time, macros, interpolation; `builtin` module (Registry of lang built-ins), `MultichannelAlgorithm` and `BridgeAlgorithm` traits | [docs.rs](https://docs.rs/rill-core) |
| **rill-core-actor** | 0.6.0-M2 | Actor model — ActorRef, Actor, ActorSystem for lock-free message passing | [docs.rs](https://docs.rs/rill-core-actor) |
| **rill-core-dsp** | 0.6.0-M2 | DSP algorithms, vector ops, filters, generators, sample player | [docs.rs](https://docs.rs/rill-core-dsp) |
| **rill-core-model** | 0.6.0-M2 | WDF core + physical modeling — string, plate, modal, cavity | [docs.rs](https://docs.rs/rill-core-model) |
| **rill-graph** | 0.6.0-M2 | Static DAG signal graph with topological sort; optional `lang` feature enables `build_graph_ir()` path that bridges to `rill-lang::graph_ir::GraphIr` | [docs.rs](https://docs.rs/rill-graph) |
| **rill-digital-filters** | 0.6.0-M2 | Biquad, SVF, Comb, MoogLadder filter nodes | [docs.rs](https://docs.rs/rill-digital-filters) |
| **rill-digital-effects** | 0.6.0-M2 | Delay, Distortion, Limiter nodes | [docs.rs](https://docs.rs/rill-digital-effects) |
| **rill-router** | 0.6.0-M2 | EQ (graphic, parametric) + mixer (channels, sends, master) | [docs.rs](https://docs.rs/rill-router) |
| **rill-fft** | 0.6.0-M2 | Radix-2 FFT, frequency-domain convolution, spectrum analysis, spectral effects | [docs.rs](https://docs.rs/rill-fft) |
| **rill-patchbay** | 0.6.0-M2 | Automation — LFO, envelopes, sensors, servos, mappings | [docs.rs](https://docs.rs/rill-patchbay) |
| **rill-lofi** | 0.6.0-M2 | Lo-fi emulation — NES, AY-3-8910, Akai S900 | [docs.rs](https://docs.rs/rill-lofi) |
| **rill-io** | 0.6.0-M2 | Audio I/O — PortAudio, ALSA, PipeWire, JACK backends | [docs.rs](https://docs.rs/rill-io) |
| **rill-telemetry** | 0.6.0-M2 | Probes, collectors, real-time monitoring, debug IPC | [docs.rs](https://docs.rs/rill-telemetry) |
| **rill-analyzer** | 0.6.0-M2 | **[CLI]** Interactive gdb-style debugger — signal probes, breakpoints, shmem IPC | — |
| **rill-analog-filters** | 0.6.0-M2 | WDF-based analog filters — WdfMoogLadder | [docs.rs](https://docs.rs/rill-analog-filters) |
| **rill-analog-effects** | 0.6.0-M2 | Analog circuit models — cassette deck, tape bridge/delay | [docs.rs](https://docs.rs/rill-analog-effects) |
| **rill-osc** | 0.6.0-M2 | OSC — UDP server, encode/decode, pattern dispatch | [docs.rs](https://docs.rs/rill-osc) |
| **rill-sampler** | 0.6.0-M2 | Sample playback + time-series reader + WAV loading | [docs.rs](https://docs.rs/rill-sampler) |
| **rill-lang** | 0.6.0-M2 | Faust-style functional signal DSL; compiles to `Algorithm<T>` or `MultichannelAlgorithm<T>` via `compile()`/`compile_with()`, or to a full `CompiledGraphEngine` via `compile_graph()` with runtime `?name` parameter support | [docs.rs](https://docs.rs/rill-lang) |

## Feature flags

| Crate | Features |
|-------|----------|
| `rill-core` | `serde`, `simd` |
| `rill-core-dsp` | `simd`, `f64`, `fast_math` |
| `rill-core-model` | `lang` |
| `rill-fft` | `simd`, `f64`, `graph`, `lang` |
| `rill-graph` | `debug`, `serialization` |
| `rill-lang` | `router`, `serde`, `debug` |
| `rill-patchbay` | `debug`, `serde`, `json`, `cbor`, `serialization`, `midi` (MIDI input), `osc` (OSC input), `alsa` |
| `rill-io` | `portaudio` (default), `midir` (default), `alsa`, `pipewire`, `jack`, `all-backends`, `serde-config` |
| `rill-sampler` | `wav` (default, enables `hound`), `graph`, `lang` |
| `rill-adrift` | `io`, `lofi`, `telemetry`, `osc`, `sampler`, `fft`, `portaudio`, `serialization` (default); `debug`, `analog`, `midi`, `alsa`, `jack`, `pipewire` |

