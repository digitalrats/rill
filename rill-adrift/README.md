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
| `analog` | `rill-core-model` + `rill-analog-filters` + `rill-analog-effects` | no |
| `serialization` | graph/patchbay serialization (JSON/CBOR) | yes |
| `portaudio` | PortAudio backend (implies `io`) | no |
| `alsa` | ALSA backend (implies `io`) | no |
| `jack` | JACK backend (implies `io`) | no |
| `pipewire` | PipeWire backend (implies `io`) | no |
| `debug` | Diagnostic & debug infrastructure (probes, command log, IPC, lifecycle logging) | no |

### Debug infrastructure (`debug` feature)

- **`debug_init`** — `init_shmem()` and `init_shmem_from_env()` create the shared
  memory region for `rill-analyzer` attach/launch. Called automatically in
  `ModularSystem::launch()`.
- **Lifecycle logging** — `ModularSystem` logs rack creation, engine build,
  backend connection, and shutdown via the `log` crate.
- **Auto-probes** — each graph node gets a signal probe at its output, wire into
  `rill-telemetry`'s `CollectorThread` for formatted output.

## Usage

```rust,no_run
use rill_adrift::modular::{ModularSystem, ModularConfig};

const BUF_SIZE: usize = 256;

let system = ModularSystem::<BUF_SIZE>::new(ModularConfig::default());
// Graphs are built from ModularSystemDef documents (serialization)
```

## Always-on core (no feature gate)

- `rill-core`, `rill-core-dsp`, `rill-graph`, `rill-oscillators`,
  `rill-digital-filters`, `rill-digital-effects`, `rill-router`,
  `rill-patchbay`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-adrift>
- Book: <https://rill-adrift.io>
