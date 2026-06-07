# rill-core-model

Wave Digital Filter (WDF) core and physical modeling — elements, adapters,
analysis, and resonant models for analog circuit and acoustic simulation.

## Key components

- **WDF Elements** — `Resistor`, `Capacitor`, `Inductor`, `Diode`, `OpAmp`
- **WDF Adapters** — `SeriesAdapter`, `ParallelAdapter`
- **WDF Filters** — `MoogLadder` (4-pole ladder), `DiodeClipper`, `RcPole`
- **Analysis** — frequency response, distortion analysis
- **Traits** — `WdfElement`, `WaveVariables` — generic over `rill_core::Transcendental` (f32/f64)
- **Physical models** — `StringModel` (1D waveguide), `PlateModel` (2D FDTD mesh),
  `ModalModel` (parallel resonant filter bank), `HelmholtzCavity` (Helmholtz resonator
  with reed excitation), `CavityArray` (coupled cavity chain)

## Dependencies

- `rill-core` — `Transcendental`, `Algorithm`, `ParameterizedAlgorithm`, vector abstractions

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-core-model>
