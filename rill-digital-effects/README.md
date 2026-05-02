# rill-digital-effects

Graph nodes for digital audio effects — Delay, Distortion, Limiter, and more.

## Key components

- **Delay** — configurable delay line with feedback and dry/wet mix
- **Distortion** — hard clip, soft clip, with configurable threshold and drive
- **Limiter** — look-ahead limiter with attack, release, and ceiling
- **Modulation** (optional `modulation` feature) — LFO modulation of effect parameters via `rill-oscillators`

## Dependencies

- `rill-core` — `SignalNode`, `Processor` trait
- `rill-core-dsp` — delay algorithms from `delay/`
- `rill-oscillators` (optional) — LFO modulation

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-digital-effects>
