# rill-io

Audio I/O backends — ALSA, CPAL, PipeWire, JACK.

## Key components

- **`AudioBackend` trait** — common interface for all I/O backends
- **`AudioEngine<B, P>`** — main engine combining a backend with a processor
- **Backends** (each behind a feature flag):
  - `cpal` — cross-platform audio I/O via CPAL (default)
  - `alsa` — Linux ALSA backend
  - `pipewire` — PipeWire backend
  - `jack` — JACK Audio Connection Kit backend
- **Optional `graph` feature** — integration with `rill-graph`
- **`GainProcessor`** — simple gain processor for testing

## Dependencies

- `rill-core` — `AudioNode`, `Processor` trait
- `rill-graph` (optional) — audio graph integration

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-io>
