# rill-analog-effects

Analog circuit models — operational amplifiers, tape decks, preamps.

## Key components

- **`OperationalAmplifier`** — op-amp model with slew-rate limiting, bandwidth, and rail-clamping
- **`CassetteDeckModel`** — tape deck emulation with tape saturation, wow & flutter, noise
- **Preamp models** — configurable circuit models

## Dependencies

- `rill-core` — `SignalNode`, `Processor` trait
- `rill-core-wdf` — WDF elements and analysis

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-analog-effects>
