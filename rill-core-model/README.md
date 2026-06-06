# rill-core-model

Wave Digital Filter (WDF) core — elements, adapters, and analysis for analog circuit modeling.

## Key components

- **Elements** — `Resistor`, `Capacitor`, `Inductor`, `Diode`
- **Adapters** — `SeriesAdapter`, `ParallelAdapter`
- **Analysis** — frequency response, distortion analysis
- **Filters** — `MoogLadder` (WDF-based 4-pole ladder filter)
- **Traits** — `WdfElement`, `WaveVariables` — generic over `rill_core::AudioNum` (f32/f64)
- **SIMD** — optional `simd` feature enables vectorization via `rill_core::vector`

## Dependencies

- `rill-core` — `AudioNum`, vector abstractions

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-core-model>
