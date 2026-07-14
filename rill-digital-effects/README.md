# rill-digital-effects

Graph nodes for digital signal effects — Delay, Distortion, Limiter, and more.

## Key components

- **Delay** — configurable delay line with feedback and dry/wet mix
- **Distortion** — hard clip, soft clip, with configurable threshold and drive
- **Limiter** — look-ahead limiter with attack, release, and ceiling
- **Reverb** — algorithmic reverb with configurable room size and damping

## Dependencies

- `rill-core` — `Node`, `Processor` trait
- `rill-core-dsp` — delay algorithms from `delay/`
- `rill-graph` (optional `graph` feature) — graph node wrappers

## Feature flags

| Feature | Description | Default |
|---------|-------------|---------|
| `graph` | Graph node wrappers (enables `rill-graph`) | no |

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-digital-effects>
