# rill-io

Audio I/O backends — PortAudio, ALSA, PipeWire, JACK.

This crate provides I/O backends and the `Output`/`Input` graph
nodes that own the reactive stream (process callback or similar).

I/O is split into three orthogonal traits:

- **`IoDriver`** — `set_process_callback`, `run`, `stop` (owns the timing loop)
- **`IoCapture`** — `read_input(channel, &mut [f32])`, `num_input_channels()`
- **`IoPlayback`** — `write_output(channel, &[f32])`, `num_output_channels()`

A single backend struct (e.g. `PipewireBackend`) implements `IoDriver`
and optionally `IoCapture` / `IoPlayback`.
`IoBackend` is a backward-compatible alias: `pub trait IoBackend: IoDriver {}`.

The process callback is registered via `IoDriver::set_process_callback()`.

## Nodes

- **`Output`** — `Sink` node. Holds `Arc<dyn IoPlayback>` and calls
  `write_output()` directly in `consume()`.  The backend is injected via
  `Sink::set_playback()` by `ProcessingState::wire_backends()`.

  ```rust
  let playback: Arc<dyn IoPlayback> = ...;
  let mut output = Output::<f32, 256>::with_channels(playback, 2);
  ```

- **`Input`** — `Source` node (push model). Holds `Arc<dyn IoCapture>`.
  Same pattern — `Source::set_capture()` injects the backend.

The process callback (registered by the orchestrator) does:
1. Drain actor mailbox (`SetParameter` commands) into graph nodes
2. `ProcessingState::process_block(&tick)` — generates, processes, propagates
3. `ProcessingState::send_clock_tick(&tick)` — dispatches to control actors

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
