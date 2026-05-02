# rill-router

Audio routing, mixing, and equalization — unified EQ + mixer crate.

## Key components

- **`GraphicEq`** — graphic equalizer with configurable band count and frequencies
- **`ParametricEq`** — parametric equalizer with frequency, Q, and gain per band
- **`MixerNode`** — multi-channel mixer with per-channel volume, pan, mute, and sends
- **Sends** — pre/post-fader send buses
- **`BiquadFactory`** — filter factory for custom filter types

## Dependencies

- `rill-core` — `SignalNode`, `Processor` trait, port/parameter types
- `rill-core-dsp` — Biquad and other filter algorithms

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-router>
