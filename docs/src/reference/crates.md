# Crates

The Rill workspace consists of 17 crates, all versioned synchronously.
(`rill-digital-filters`, `rill-analog-filters`, `rill-analog-effects` were
deleted in SP-3b; their algorithms were folded into `rill-core-dsp` /
`rill-core-model`.)

| Crate | Version | Description | Docs |
|-------|---------|-------------|------|
| **rill-adrift** | 0.6.0-M2 | Umbrella crate — re-exports all workspace crates; `full_registry()` / `full_ffi()` build the builtin factory registries | [docs.rs](https://docs.rs/rill-adrift) |
| **rill-core** | 0.6.0-M2 | Core traits, math, buffers, queues, time, macros, interpolation; `MultichannelAlgorithm` and `BridgeAlgorithm` traits | [docs.rs](https://docs.rs/rill-core) |
| **rill-core-actor** | 0.6.0-M2 | Actor model — ActorRef, Actor, ActorSystem for lock-free message passing | [docs.rs](https://docs.rs/rill-core-actor) |
| **rill-core-dsp** | 0.6.0-M2 | DSP algorithms, vector ops, filters, generators, sample player (algorithms-only; rill-lang registers its factories) | [docs.rs](https://docs.rs/rill-core-dsp) |
| **rill-core-model** | 0.6.0-M2 | WDF core + physical modeling — string, plate, modal, cavity, WDF analog filters (MoogLadder) | [docs.rs](https://docs.rs/rill-core-model) |
| **rill-graph** | 0.6.0-M2 | Static DAG signal graph with topological sort; optional `lang` feature enables `build_graph_ir()` path that bridges to `rill-lang::graph_ir::GraphIr` | [docs.rs](https://docs.rs/rill-graph) |
| **rill-digital-effects** | 0.6.0-M2 | Delay, Distortion, Limiter (algorithms-only; rill-lang registers its factories) | [docs.rs](https://docs.rs/rill-digital-effects) |
| **rill-router** | 0.6.0-M2 | EQ (graphic, parametric) + mixer (channels, sends, master) + dry/wet | [docs.rs](https://docs.rs/rill-router) |
| **rill-fft** | 0.6.0-M2 | Radix-2 FFT, frequency-domain convolution, spectrum analysis, spectral effects | [docs.rs](https://docs.rs/rill-fft) |
| **rill-patchbay** | 0.6.0-M2 | Automation — LFO, envelopes, sensors, servos, mappings | [docs.rs](https://docs.rs/rill-patchbay) |
| **rill-lofi** | 0.6.0-M2 | Lo-fi emulation — NES, AY-3-8910, Akai S900 | [docs.rs](https://docs.rs/rill-lofi) |
| **rill-io** | 0.6.0-M2 | Audio I/O — PortAudio, ALSA, PipeWire, JACK backends | [docs.rs](https://docs.rs/rill-io) |
| **rill-telemetry** | 0.6.0-M2 | Probes, collectors, real-time monitoring, debug IPC | [docs.rs](https://docs.rs/rill-telemetry) |
| **rill-analyzer** | 0.6.0-M2 | **[CLI]** Interactive gdb-style debugger — signal probes, breakpoints, shmem IPC | — |
| **rill-osc** | 0.6.0-M2 | OSC — UDP server, encode/decode, pattern dispatch | [docs.rs](https://docs.rs/rill-osc) |
| **rill-sampler** | 0.6.0-M2 | Sample playback + time-series reader + WAV loading + tape write/read heads | [docs.rs](https://docs.rs/rill-sampler) |
| **rill-lang** | 0.6.0-M2 | Faust-style functional signal DSL; builtin signatures in the language (FFI catalog), factories via `ForeignRegistry`; compiles via `compile()`/`compile_with_ffi()`, or to a full `CompiledGraphEngine` via `compile_graph()` with runtime `?name` parameter support | [docs.rs](https://docs.rs/rill-lang) |

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

