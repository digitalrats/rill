# rill-oscillators

Graph nodes for oscillators — sine, saw, square, triangle, pulse, noise, LFO, and envelopes.

## Key components

- **Oscillators** — sine, saw, square, triangle, pulse — implementing `Source`/`Processor` traits
- **Noise generators** — white, pink, brown, blue, violet
- **LFO** — low-frequency oscillator with bipolar/unipolar mode and sync
- **Envelopes** — ADSR, AR, ASR with configurable stages

## Dependencies

- `rill-core` — `Node`, `Source`/`Processor` traits
- `rill-core-dsp` — DSP algorithms from `generators/`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-oscillators>
