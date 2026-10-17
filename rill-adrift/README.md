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
| `fft` | `rill-fft` (FFT/spectral processing) | yes |
| `serialization` | modular document + patchbay serialization (JSON/CBOR) | yes |
| `portaudio` | PortAudio backend (implies `io`) | yes |
| `analog` | `rill-core-model` (WDF algorithms) + `rill-lang/model` (analog_moog factory) | no |
| `alsa` | ALSA backend (implies `io`) | no |
| `jack` | JACK backend (implies `io`) | no |
| `pipewire` | PipeWire backend (implies `io`) | no |
| `midi` | MIDI passthrough to `rill-patchbay` + `rill-io` | no |
| `debug` | Diagnostic & debug infrastructure (probes, command log, IPC, lifecycle logging) | no |

### Debug infrastructure (`debug` feature)

- **`debug_init`** — `init_shmem()` and `init_shmem_from_env()` create the shared
  memory region for `rill-analyzer` attach/launch.
- **Lifecycle logging** — `ModularSystem` logs rack creation, program
  compilation/execution, and shutdown via the `log` crate.

## Usage

```rust,no_run
use rill_adrift::modular::{ModularSystem, ModularConfig};

const BUF_SIZE: usize = 256;

let system = ModularSystem::<BUF_SIZE>::new(ModularConfig::default());
// Racks are launched from ModularSystemDef documents (serialization); each
// rack's rill-lang `main : (String…) -> IO ()` program runs to completion on
// its own thread through the IO runner.
```

## Always-on core (no feature gate)

- `rill-core`, `rill-core-actor`, `rill-core-dsp`, `rill-digital-effects`,
  `rill-patchbay`, `rill-lang`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-adrift>
- Book: <https://rill-adrift.io>
