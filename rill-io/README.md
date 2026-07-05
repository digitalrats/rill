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

The process callback (registered by the orchestrator) does, per `block_size`
chunk of the callback's buffer:
1. `ProcessingState::process_block(&tick)` — adopts the tick's sample rate
   (re-initialising nodes if the hardware rate differs), drains the actor
   mailbox and applies `SetParameter` writes (sample-accurate ones at the block
   matching their `sample_pos`), then generates/processes/propagates
2. `ProcessingState::send_clock_tick(&tick)` — forwards the tick to control
   actors (chunking backends dispatch one tick per block)

## Backends

| Backend | Feature | Thread model |
|---------|---------|-------------|
| `PortAudioBackend` | `portaudio` (default) | RT callback, large buffer chunked into `block_size` pieces |
| `PipewireBackend` | `pipewire` | RT callback (PW thread), buffer negotiated via `SPA_PARAM_Buffers`, chunked into `block_size` pieces |
| `JackBackend` | `jack` | RT callback (JACK thread) |
| `AlsaBackend` | `alsa` | `snd_pcm_wait()` — poll‑driven, exact period required |
| `NullBackend` | *(always)* | No‑op, for testing |

### Buffer sizing (callback-driven backends)

A single `block_size` (256-frame) period is unstable through PipeWire (crackling
via the ALSA plugin, xruns), so callback-driven backends request a larger DMA
buffer and chunk it back into `block_size` pieces in the callback, emitting one
`ClockTick` per rill block (the same model PipeWire uses internally):

- **PipeWire** negotiates `BUFFER_BLOCKS × block_size` (16 × 256 = 4096 frames)
  via a `SPA_PARAM_Buffers` object on connect, instead of PipeWire's large
  default (~12288 frames).
- **PortAudio** requests `PA_BUFFER_BLOCKS × block_size` (16 × 256 = 4096) as
  `frames_per_buffer`.

Because the whole buffer is one I/O callback, its duration is also the
async-control look-ahead (`ClockTick.io_quantum`); both constants are documented
one-line tunables trading control latency (~93 ms at 16 blocks) against
stability (the stable minimum is hardware/config dependent).

Sample rate negotiation:
- **JACK**: reads `client.sample_rate()` after activation and puts the *actual*
  hardware rate in the `ClockTick`; the graph re-initialises its nodes to it
- **ALSA**: queries `hw.get_rate()` after `set_rate(Nearest)`, checks `hw.get_period_size() == BUF_SIZE`
- **PipeWire**: output uses requested rate, input reads negotiated rate atomically
- **PortAudio**: opens stream with exact requested rate and buffer size
- **Null**: uses `config.sample_rate` directly

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-io>
