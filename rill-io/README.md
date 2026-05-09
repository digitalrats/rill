# rill-io

Audio I/O backends — PortAudio, ALSA, PipeWire, JACK.

This crate provides I/O backends and the `Output`/`Input` graph
nodes that own the reactive stream (process callback or similar).

All backends implement [`IoBackend`] — a trait for reactive stream processing:
`set_process_callback`, `read`, `write`, `run`, `stop`.

The process callback receives the actual negotiated sample rate (`f32`)
from the backend so that `ClockTick` always contains the true device rate.

## Nodes

- **`Output`** — `Sink` node. Borrows backend via [`IoBackendPtr`].
  `start()` registers the process callback and drives the graph:

  ```rust
  let mut output = Output::<f32, 256>::with_channels(2);
  ```

- **`Input`** — `Source` node (push model). Borrows backend via
  [`IoBackendPtr`]. `start()` registers the process callback.

In both cases the callback does:
1. Drain `MpscQueue<ParameterCommand>` into graph nodes
2. Create `ClockTick` with the backend‑supplied sample rate
3. `process_block()` on the driver node
4. `Port::propagate()` — recursive DAG traversal

## Backends

| Backend | Feature | Thread model |
|---------|---------|-------------|
| `PortAudioBackend` | `portaudio` (default) | RT callback, exact buffer size |
| `PipewireBackend` | `pipewire` | RT callback (PW thread) |
| `JackBackend` | `jack` | RT callback (JACK thread) |
| `AlsaBackend` | `alsa` | `snd_pcm_wait()` — poll‑driven, exact period required |
| `NullBackend` | *(always)* | No‑op, for testing |

Sample rate negotiation:
- **JACK**: reads `client.sample_rate()` after activation
- **ALSA**: queries `hw.get_rate()` after `set_rate(Nearest)`, checks `hw.get_period_size() == BUF_SIZE`
- **PipeWire**: output uses requested rate, input reads negotiated rate atomically
- **PortAudio**: opens stream with exact requested rate and buffer size
- **Null**: uses `config.sample_rate` directly

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-io>
