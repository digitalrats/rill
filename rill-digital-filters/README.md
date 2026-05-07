# rill-digital-filters

Graph nodes for digital filters — Biquad, OnePole, State Variable, Butterworth, Chebyshev, and comb filters.

## Key components

- **Filter types** — Biquad, OnePole, SVF, Butterworth, Chebyshev I/II, Comb, MoogLadder
- **All filters** implement `Processor` trait from `rill-core`
- **DSP backend** — algorithms from `rill-core-dsp::filters`

## Dependencies

- `rill-core` — `Node`, `Processor` trait
- `rill-core-dsp` — filter algorithms from `filters/`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-digital-filters>
