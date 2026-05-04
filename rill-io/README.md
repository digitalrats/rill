# rill-io

Audio I/O backends — ALSA, CPAL, PipeWire, JACK.

This crate provides I/O backends and the `AudioInput`/`AudioOutput` graph
nodes that own the reactive stream (PipeWire callback or similar).

All backends implement [`AudioIo`] — a trait for reactive stream processing:
`set_process_callback`, `read_input`, `write_output`, `start`, `stop`.

## Nodes

- **`AudioInput`** — `Source` node (push model). Owns the backend
  (`Box<dyn AudioIo>`). Creates the process callback via [`start()`].
  Backend can be created externally via [`set_backend()`] or by name via
  [`init_backend()`].
  ```rust
  let mut input = AudioInput::<f32, 256>::new();
  input.init_backend("pipewire", config)?;
  input.start(nodes_ptr, source_idx, drain_fn, sample_rate);
  ```

- **`AudioOutput`** — `Sink` node (push model or pull model). Borrows
  backend via [`AudioIoPtr`]. In pull model, [`set_active()`] stores the
  source index and [`start()`] drives the graph from that source:
  ```rust
  let mut output = AudioOutput::<f32, 256>::new();
  output.set_backend(ptr);
  output.set_active(source_idx);
  output.start(nodes_ptr, drain_fn, sample_rate);
  ```

## Processing models

| Model | Active node | Callback owner |
|-------|-------------|----------------|
| **Push** | `AudioInput` (Source) | `AudioInput::start()` |
| **Pull** | `AudioOutput` (Sink) | `AudioOutput::start()` |

In both cases the callback does:
1. Drain `MpscQueue<ParameterCommand>` into graph nodes
2. `process_block()` on the source → `generate()` fills output ports
3. `Port::propagate()` — recursive DAG traversal, data lands in port buffers
4. `AudioOutput::consume()` reads from its input ports → `write_output()`

## Backends

| Backend | Feature | AudioIo | Mechanism |
|---------|---------|---------|-----------|
| `NullBackend` | *(always)* | ✅ | No-op, for testing |
| `PipewireBackend` | `pipewire` | ✅ | RT callback (PW thread) |
| `JackBackend` | `jack` | ✅ | RT callback (JACK thread) |
| `AlsaBackend` | `alsa` | ✅ | `snd_pcm_wait()` — event‑driven |
| `CpalBackend` | `cpal` | ✅ | Thread + polling |

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-io>
