# rill-fft

Fast Fourier Transform and frequency‑domain signal processing for the Rill ecosystem.

## Key components

| Type | Purpose |
|---|---|
| `ComplexFft<T>` | Radix-2 DIT complex FFT — forward + inverse, interleaved and SoA APIs |
| `RealFft<T>` | Real‑valued FFT via half‑size complex packing — N real → N/2+1 bins |
| `OverlapAddConvolver<T, BUF>` | Frequency‑domain convolution for medium‑length IRs (up to ~16k) |
| `PartitionedConvolver<T, BUF>` | Uniform partitioned convolution for very long IRs (100k+ samples) |
| `FftSpectrumAnalyzer<T>` | FFT‑based spectrum analyser — magnitude spectrum with windowing |
| `SpectralGate<T, BUF>` | Frequency‑domain noise gate — silences bins below threshold |
| `SpectralDelay<T, BUF, MAX_DELAY>` | Frequency‑dependent delay with feedback — metallic resonances, comb filtering |

## Quick example

```rust
use rill_fft::complex_fft::ComplexFft;
use num_complex::Complex;

let fft = ComplexFft::<f32>::new(1024);
let mut data: Vec<Complex<f32>> = (0..1024)
    .map(|i| Complex::new((i as f32 * 0.1).sin(), 0.0))
    .collect();

fft.forward(&mut data);
// ... manipulate spectrum ...
fft.inverse(&mut data);
```

### Convolution

```rust
use rill_fft::partitioned_conv::PartitionedConvolver;

// IR of 16384 samples, processing block size 128
let mut conv = PartitionedConvolver::<f32, 128>::new(16384);
conv.set_ir(&impulse_response);

let mut output = [0.0f32; 128];
conv.process(&input, &mut output);
```

## Real‑time safety

All scratch buffers (twiddle tables, delay lines, overlap buffers) are
pre‑allocated in constructors. `process()` performs **zero heap allocations**
on the hot path — verified by panic‑on‑alloc tests in `tests/rt_safety.rs`.

`#![deny(unsafe_code)]` — pure safe Rust. SIMD acceleration is handled by
LLVM auto‑vectorisation or the `simd` feature (via `rill-core/wide`).

## Performance (f32, x86_64, release build)

| Operation | Size | Time | Throughput |
|---|---|---|---|
| `ComplexFft::forward` | 1024 | 6.7 µs | 153 Melem/s |
| `RealFft::forward` | 1024 | 6.2 µs | 165 Melem/s |
| `ComplexFft::forward` | 16384 | 177 µs | 92 Melem/s |
| `OverlapAddConvolver` | IR 2048, BUF 128 | 61 µs/block | ~2100 blocks/s |
| `PartitionedConvolver` | IR 65536, BUF 128 | 104 µs/block | ~9600 blocks/s |

At 44.1 kHz with block size 128 the per‑block budget is ~2.9 ms.
All operations fit comfortably within the real‑time budget.

## Feature flags

| Feature | Effect |
|---|---|
| `simd` | Enables `rill-core/simd` — hardware‑accelerated `F32x4` / `F64x2` |
| `f64` | Enables f64‑precision (no extra deps) |
| `graph` | Pulls in `rill-graph` — enables graph‑node wrappers |
| `lang` | Pulls in `rill-lang` — enables DSL builtins: `spectralgate`, `spectraldelay`, `convolver` |

## rill-lang builtins (`lang` feature)

```
main = spectralgate _ 0.01 0.0;     // spectral noise gate
main = spectraldelay _ 0.5 0.3;     // spectral delay with feedback
main = convolver _ 1.0 1.0;         // partitioned convolution
```

## Benchmarks

```bash
cargo bench -p rill-fft --bench fft_complex_bench
cargo bench -p rill-fft --bench fft_real_bench
cargo bench -p rill-fft --bench convolver_bench
cargo bench -p rill-fft --bench rt_timing_bench
```

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-fft>
