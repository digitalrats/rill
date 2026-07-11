# rill-adrift

**rill-adrift** is the umbrella crate for the **Rill** ecosystem — a modular audio/signal processing framework for Rust.

One dependency brings in the entire ecosystem:

```toml
[dependencies]
rill-adrift = "0.6.0-M1"
```

```rust
use rill_adrift::prelude::*;
use rill_adrift::rill_oscillators::signal::SineOsc;
```

## What is Rill?

Rill is not a monolith. It is a collection of specialized crates, each solving one problem well:

| Layer | Crates |
|---|---|
| **Core** | `rill-core` — traits, math, buffers, queues, time, macros |
| **Actor** | `rill-core-actor` — lock-free actor model (ActorRef, ActorSystem) |
| **DSP** | `rill-core-dsp` — algorithms, filters, generators, delay, vector ops |
| **Graph** | `rill-graph` — static DAG signal graph, `Port::propagate` |
| **Effects** | `rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`, `rill-router` |
| **FFT** | `rill-fft` — radix-2 FFT, frequency-domain convolution, spectral effects |
| **Automation** | `rill-patchbay` — LFO, envelopes, sensors, servos, mappings |
| **Language** | `rill-lang` — Faust-style functional signal DSL, compiles to `Algorithm<T>` or `MultichannelAlgorithm<T>`, or to `RillGraphEngine` for whole-graph compilation |
| **Analog** | `rill-core-model`, `rill-analog-filters`, `rill-analog-effects` — WDF circuit modeling |
| **I/O** | `rill-io` — ALSA, PortAudio, PipeWire, JACK backends (pure I/O, no engine) |
| **Network** | `rill-osc` — OSC server and networking; powers `rill-patchbay` OSC sensors for graph control |
| **Monitoring** | `rill-telemetry` — probes, collectors |
| **Lo-Fi** | `rill-lofi` — bitcrush, downsampling, console emulation |

## Domain-Agnostic

Only `rill-io` is tied to audio hardware. The rest work anywhere — IoT, embedded, robotics, control systems, signal processing.

The foundation (`rill-core`) provides lock-free queues, `no_std`-compatible math traits, and real-time safe abstractions that apply far beyond audio.

## Project Status

Active development — 20 crates, version 0.6.0-M1, 706 tests.

- [GitHub](https://github.com/DigitalRats/rill)
- [crates.io](https://crates.io/crates/rill-adrift)
- [docs.rs](https://docs.rs/rill-adrift)
