# rill-io

Audio I/O backends — ALSA, CPAL, PipeWire, JACK.

This crate provides only the hardware abstraction layer. Graph processing is
handled by [`rill-graph::AudioEngine`](https://docs.rs/rill-graph) —
this crate is purely about backend I/O.

## Key components

- **`AudioBackend` trait** — common interface for all I/O backends
- **Backends** (each behind a feature flag):
  - `cpal` — cross-platform audio I/O via CPAL (default)
  - `alsa` — Linux ALSA backend
  - `pipewire` — PipeWire backend
  - `jack` — JACK Audio Connection Kit backend

## Two-thread architecture

Audio processing is separated into two threads:

- **Audio thread** (hard RT): runs [`rill-graph::AudioEngine`] which calls
  `process_tick()` for clock boundary and `process_block()` for graph
  processing. Source/Sink nodes own the I/O buffers.
- **Control thread** (soft RT): runs `PatchbayManager` for automata,
  sensors, and servos. Communicates via `CommandQueue`/`TelemetryQueue`.

See [`rill-graph` documentation](https://docs.rs/rill-graph) for details.

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-io>
