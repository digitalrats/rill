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
| `debug` | Shared-memory IPC (`debug_init`) for `rill-analyzer` attach. The modular host does not auto-create the shmem region or wire probes/command logs; lifecycle logging is unconditional (see below) | no |

### Debug infrastructure (`debug` feature)

- **`debug_init`** — `init_shmem()` and `init_shmem_from_env()` create the shared
  memory region for `rill-analyzer` attach/launch. Call `init_shmem()` explicitly;
  `ModularSystem::launch` does not create it for you.
- **Lifecycle logging** — `ModularSystem` logs rack creation, program
  compilation/execution, run-time failures, and shutdown via the `log` crate.
  Logging is unconditional (`log` is a plain dependency), not gated by `debug`.
- **Control modules are inert** — modules declared in a rack are constructed at
  launch, but no clock or parameter source is wired: servos, sensors, and clocks
  receive no `ClockTick` and drive nothing. The module path is reserved for a
  future control plan; see the `modular` module docs.

## Usage

```rust,no_run
use rill_adrift::modular::{ModularSystem, ModularConfig};

const BUF_SIZE: usize = 256;

let system = ModularSystem::<BUF_SIZE>::new(ModularConfig::default());
// Racks are launched from ModularSystemDef documents (serialization); each
// rack's rill-lang `main : IO ()` program runs to completion on its own thread
// through the IO runner.
```

`launch` returns an error when a rack program does not compile
(`ModularError::Program`) or when a rack control module cannot be constructed
(`ModularError::Rack`). A program that fails at run time is reported through
`ModularSystem::rack_run_error` after `stop()` has joined the rack thread.

The control-module host is currently **inert**: modules are constructed but no
clock/parameter source drives them, so servos, sensors, and clocks do nothing.
That path is reserved for a future control/audio plan.

## Always-on core (no feature gate)

- `rill-core`, `rill-core-actor`, `rill-core-dsp`, `rill-digital-effects`,
  `rill-patchbay`, `rill-lang`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-adrift>
- Book: <https://rill-adrift.io>
