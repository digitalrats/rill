# rill-adrift

Umbrella crate re-exporting all [rill](https://github.com/DigitalRats/rill)
crates for signal processing application development.

## Feature flags

| Feature | Enables | Default |
|---------|---------|---------|
| `io` | `rill-io` (audio backends) | yes |
| `lofi` | `rill-lofi` (lo-fi emulation) | yes |
| `telemetry` | `rill-telemetry` (probes) | yes |
| `osc` | `rill-osc` (OSC server, requires tokio) | yes |
| `sampler` | `rill-sampler` (sample playback) | yes |
| `analog` | `rill-core-wdf` + `rill-analog-filters` + `rill-analog-effects` | no |
| `serialization` | graph/patchbay serialization (JSON/CBOR) | yes |
| `portaudio` | PortAudio backend (implies `io`) | no |
| `alsa` | ALSA backend (implies `io`) | no |
| `jack` | JACK backend (implies `io`) | no |
| `pipewire` | PipeWire backend (implies `io`) | no |

## Usage

```rust,no_run
use rill_adrift::modular::{ModularSystem, ModularConfig};
use rill_adrift::rill_core::traits::Params;

const BUF_SIZE: usize = 256;

let mut system = ModularSystem::<BUF_SIZE>::new(ModularConfig::default());
let mut builder = system.create_builder();
builder.add_node("rill/sine", &Params::new(48000.0).with("freq", rill_adrift::rill_core::ParamValue::Float(440.0)));
```

## Always-on core (no feature gate)

- `rill-core`, `rill-core-dsp`, `rill-graph`, `rill-oscillators`,
  `rill-digital-filters`, `rill-digital-effects`, `rill-router`,
  `rill-patchbay`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-adrift>
- Book: <https://rill-adrift.io>
