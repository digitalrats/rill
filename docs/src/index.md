# rill-adrift

**rill-adrift** is the umbrella crate for the **Rill** ecosystem — a modular audio/signal processing framework for Rust.

One dependency brings in the entire ecosystem:

```toml
[dependencies]
rill-adrift = "0.5.0-beta.6"
```

```rust
use rill_adrift::prelude::*;
use rill_adrift::rill_oscillators::audio::SineOsc;
```

## What is Rill?

Rill is not a monolith. It is a collection of specialized crates, each solving one problem well:

| Layer | Crates |
|---|---|
| **Core** | `rill-core` — traits, math, buffers, queues, time, macros |
| **DSP** | `rill-core-dsp` — algorithms, filters, generators, delay, vector ops |
| **Graph** | `rill-graph` — static DAG audio graph, `Port::propagate` (process_tick, process_block, spawn) |
| **Effects** | `rill-oscillators`, `rill-digital-filters`, `rill-digital-effects`, `rill-router` |
| **Automation** | `rill-patchbay` — LFO, envelopes, sensors, servos, mappings |
| **Analog** | `rill-core-model`, `rill-analog-filters`, `rill-analog-effects` — WDF circuit modeling |
| **I/O** | `rill-io` — ALSA, CPAL, PipeWire, JACK backends (pure I/O, no engine) |
| **Network** | `rill-osc` — OSC server and networking; powers `rill-patchbay` OSC sensors for graph control |
| **Monitoring** | `rill-telemetry` — probes, collectors |
| **Lo-Fi** | `rill-lofi` — bitcrush, downsampling, console emulation |

## Domain-Agnostic

Only `rill-io` is tied to audio hardware. The rest work anywhere — IoT, embedded, robotics, control systems, signal processing.

The foundation (`rill-core`) provides lock-free queues, `no_std`-compatible math traits, and real-time safe abstractions that apply far beyond audio.

## Project Status

Active development — 18 crates, 0.5.0-beta.6, 487 tests.

- [GitHub](https://github.com/DigitalRats/rill)
- [crates.io](https://crates.io/crates/rill-adrift)
- [docs.rs](https://docs.rs/rill-adrift)
