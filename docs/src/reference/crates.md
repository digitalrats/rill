# Crates

The Rill workspace consists of 17 crates:

| Crate | Version | Description |
|-------|---------|-------------|
| **rill-adrift** | 0.4.0 | Umbrella crate — re-exports all workspace crates |
| **rill-core** | 0.4.0 | Core traits, math, buffers, queues, time, macros, executor, interpolation |
| **rill-core-dsp** | 0.4.0 | DSP algorithms, vector ops, filters, generators, sample player |
| **rill-core-wdf** | 0.4.0 | Wave Digital Filter core — elements, adapters, analysis |
| **rill-graph** | 0.4.0 | Static DAG audio graph with topological sort |
| **rill-oscillators** | 0.4.0 | Oscillators — Sine, Saw, Square, Noise, LFO, Envelope, Wavetable |
| **rill-digital-filters** | 0.4.0 | Digital filters — Biquad, SVF, Comb, MoogLadder |
| **rill-digital-effects** | 0.4.0 | Digital effects — Delay, Distortion, Limiter |
| **rill-router** | 0.4.0 | EQ (graphic, parametric) + mixer (channels, sends, master) |
| **rill-patchbay** | 0.4.0 | Automation — LFO, envelopes, sensors, servos, mappings |
| **rill-lofi** | 0.4.0 | Lo-fi emulation — NES, AY-3-8910, Akai S900 |
| **rill-io** | 0.4.0 | Audio I/O — ALSA, CPAL, PipeWire, JACK backends |
| **rill-telemetry** | 0.4.0 | Probes, collectors, real-time monitoring |
| **rill-analog-filters** | 0.4.0 | WDF-based analog filters — WdfMoogLadder |
| **rill-analog-effects** | 0.4.0 | Analog circuit models — op-amp, tape deck, preamps |
| **rill-osc** | 0.4.0 | OSC server — UDP, encode/decode, pattern dispatch |
| **rill-sampler** | 0.4.0 | Sample playback + time-series reader + WAV loading |

## Dependency Graph

```
rill-core
├── rill-core-dsp
│   ├── rill-oscillators
│   ├── rill-digital-filters
│   ├── rill-digital-effects
│   └── rill-router
├── rill-graph
├── rill-patchbay
├── rill-lofi
├── rill-telemetry
├── rill-io
├── rill-core-wdf
│   ├── rill-analog-filters
│   └── rill-analog-effects
├── rill-sampler
│   (depends on rill-core + rill-core-dsp)
└── rill-osc (standalone)

rill-adrift — umbrella, re-exports all of the above
```

## Feature Flags

| Crate | Features |
|-------|----------|
| `rill-core` | `serde`, `stats` |
| `rill-core-dsp` | `simd`, `f64`, `fast_math`, `unstable` |
| `rill-core-wdf` | `simd` |
| `rill-digital-effects` | `modulation` (enables `rill-oscillators`) |
| `rill-io` | `cpal` (default), `alsa`, `pipewire`, `jack`, `all-backends`, `graph` |
| `rill-sampler` | `wav` (default, enables `hound`) |
| `rill-adrift` | `io`, `lofi`, `telemetry`, `osc`, `sampler` (default); `analog` (opt-in); backend passthrough `alsa`, `cpal`, `jack`, `pipewire` |
