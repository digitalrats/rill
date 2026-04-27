# rill-core-dsp

Core DSP infrastructure — vector abstractions, algorithms, and macros for audio processing.

## Key components

- **Vector abstractions** — `ScalarVector1`, `ScalarVector2`, `ScalarVector4` for portable SIMD
- **`Algorithm` trait** — unified interface for all DSP components with block processing (`process_block`)
- **Filters** — Biquad, OnePole, SVF, Butterworth, Chebyshev I/II, Comb, MoogLadder
- **Generators** — Sine, Saw, Square, Triangle, Pulse, Noise (White/Pink/Brown/Blue/Violet), LFO, Envelope (ADSR/AR/ASR), FM synthesis
- **Delay** — Delay, MultiTapDelay, DiffusionDelay, ModulatedDelay
- **Macros** — `simple_algorithm!`, `parameterized_algorithm!`, `filter_algorithm!`, `effect_algorithm!`, `generator_algorithm!`

## Dependencies

- `rill-core` — `AudioNum`, math abstractions

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-core-dsp>
