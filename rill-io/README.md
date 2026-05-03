# rill-io

Audio I/O backends — ALSA, CPAL, PipeWire, JACK.

This crate provides I/O backends and the `AudioInput`/`AudioOutput` graph
nodes that own the reactive stream (PipeWire callback or similar).

## Key components

- **`AudioIo` trait** — abstract reactive stream backend (`read_input`,
  `write_output`, `set_process_callback`, `start`, `stop`)
- **`BackendRegistry`** — global map `name → AudioIo`, populated during graph
  construction by the node factory
- **`AudioInput`** — `Source` node that owns the processing callback:
  1. Drain command queue into graph nodes
  2. `read_input()` from backend → fill output ports
  3. `Port::propagate()` recursively processes the DAG
- **`AudioOutput`** — `Sink` node, writes processed data via `write_output()`
- **Backends** (each behind a feature flag):
  - `pipewire` — PipeWire backend (primary, tested with virtual devices)
  - `cpal` — cross-platform audio I/O via CPAL (legacy)
  - `alsa` — Linux ALSA backend
  - `jack` — JACK Audio Connection Kit backend

## Processing model

No external engine. `AudioInput` creates the backend callback, which:
1. Drains `MpscQueue<ParameterCommand>` into graph nodes
2. Calls `Source::generate()` (reads backend → fills output ports)
3. Calls `Port::propagate()` → recursively cascades through the DAG

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-io>
