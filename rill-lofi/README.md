# rill-lofi

Lo-fi audio emulation — bitcrushing, downsampling, noise, wow & flutter.

## Key components

- **`LofiProcessor`** — configurable lo-fi processor
- **`LofiConfig`** — bit depth, sample rate reduction, noise, distortion, wow & flutter,
  `dc_offset` (DC removal, e.g. 0.5 for AY-3-8910), `output_ceiling` (hard clamp ±value),
  `output_gain` (0.0–4.0)
- **`ClassicSystem`** — presets for classic systems (NES, AY-3-8910, Akai S900, C64, etc.)
- **`LofiInput`** — source node wrapping any `IoBackend` with lofi processing

## Dependencies

- `rill-core` — `Node`, `Processor` trait

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-lofi>
