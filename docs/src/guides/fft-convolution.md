# FFT and Convolution in Rill

The `rill-fft` crate provides Fast Fourier Transform, frequency-domain convolution,
and spectral processing — all RT‑safe, allocation‑free in the process path.

## Crate overview

`rill-fft` is an optional workspace crate, feature‑gated behind `fft` in
`rill-adrift` (enabled by default). Dependencies: `rill-core` + `rill-core-dsp`.

| Module | Key types | Purpose |
|---|---|---|
| `complex_fft` | `ComplexFft<T>` | Radix‑2 DIT complex FFT (forward + inverse) |
| `real_fft` | `RealFft<T>` | Real‑valued FFT via two‑for‑one packing |
| `overlap_add` | `OverlapAddConvolver<T, BUF>` | Frequency‑domain convolution (medium IRs) |
| `partitioned_conv` | `PartitionedConvolver<T, BUF>` | Partitioned convolution (long IRs) |
| `spectrum` | `FftSpectrumAnalyzer<T>` | FFT‑based spectrum analyser |
| `effects` | `SpectralGate`, `SpectralDelay` | Frequency‑domain effects |
| `nodes` | `ConvolverNode` | Graph‑node wrappers |

All types are generic over `T: Transcendental`, supporting both `f32` and `f64`.

## RT safety

Every scratch buffer — twiddle tables, overlap accumulators, spectrum arrays,
delay‑line ring buffers — is **pre‑allocated in the constructor**. The `process()`
methods perform zero heap allocations, verified by a custom panic‑on‑alloc
allocator in `tests/rt_safety.rs`.

```rust,no_run
use rill_fft::complex_fft::ComplexFft;

// All buffers pre-allocated here:
let fft = ComplexFft::<f32>::new(1024);

// In the signal thread — zero allocations:
fft.forward(&mut data);
fft.inverse(&mut data);
```

No `unsafe` code — `#![deny(unsafe_code)]` is enforced.

## FFT

### Complex FFT

`ComplexFft<T>` implements the Cooley–Tukey radix‑2 Decimation‑In‑Time algorithm.
Twiddle factors and bit‑reversal tables are pre‑computed on construction.
Sizes must be powers of two, ≥ 2.

```rust,no_run
use rill_fft::complex_fft::ComplexFft;
use num_complex::Complex;

let fft = ComplexFft::<f32>::new(1024);

// Fill data buffer
let mut data: Vec<Complex<f32>> = (0..1024)
    .map(|i| Complex::new((i as f32 * 0.1).sin(), 0.0))
    .collect();

// Forward transform (in-place)
fft.forward(&mut data);

// Manipulate spectrum here...

// Inverse transform (in-place, scaled by 1/N)
fft.inverse(&mut data);
```

### Real FFT

`RealFft<T>` uses the half‑size complex FFT with real packing/unpacking
(two‑for‑one method). Transforms *N* real samples into *N/2 + 1* complex bins:

```rust,no_run
use rill_fft::real_fft::RealFft;
use num_complex::Complex;

let mut fft = RealFft::<f32>::new(1024);

let input: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.1).sin()).collect();
let mut spectrum = vec![Complex::new(0.0, 0.0); 513];

// Forward: N real → N/2+1 complex
fft.forward(&input, &mut spectrum);

// Inverse: N/2+1 complex → N real
let mut reconstructed = vec![0.0f32; 1024];
fft.inverse(&spectrum, &mut reconstructed);
```

Only the non‑redundant half of the spectrum is stored — bins 0 (DC) and
*N/2* (Nyquist) are purely real.

## Convolution

Rill provides three convolution methods, each optimal for a different
impulse response length.

### DirectConvolver — short IRs (up to ~128 samples)

Time‑domain convolution in `rill-core-dsp`. Stack‑allocated via const generics,
no heap allocations at all. Best for short filtering, EQ emulation, cabinet
simulation with short IRs.

```rust,no_run
use rill_core_dsp::DirectConvolver;

let mut conv = DirectConvolver::<f32, 64, 128>::new();
conv.set_ir(&[0.3, 0.5, 0.2, 0.1, 0.0, /* ... */]);

let input = vec![0.5f32; 128];
let mut output = vec![0.0f32; 128];
conv.process(Some(&input), &mut output).unwrap();
```

Implements `Algorithm<T>` — usable as a port‑level algorithm in any graph node.

### OverlapAddConvolver — medium IRs (256…16384 samples)

Frequency‑domain convolution via real FFT. The IR is FFT‑transformed once
on `set_ir()`. Each input block is FFT‑transformed, multiplied by the IR
spectrum, inverse‑transformed, and overlap‑added to produce the output.

```rust,no_run
use rill_fft::overlap_add::OverlapAddConvolver;

let mut conv = OverlapAddConvolver::<f32, 128>::new(2048);

let ir: Vec<f32> = load_wav("reverb.wav");
conv.set_ir(&ir);

let input = [0.5f32; 128];
let mut output = [0.0f32; 128];
conv.process(&input, &mut output);
```

### PartitionedConvolver — long IRs (up to hundreds of thousands of samples)

Uniform partitioned convolution. The IR is split into partitions of
`BUF_SIZE` samples; each partition is FFT‑transformed once. Input blocks
are FFT‑transformed and stored in a circular buffer. Scales to IRs of
hundreds of thousands of samples with predictable per‑block cost.

```rust,no_run
use rill_fft::partitioned_conv::PartitionedConvolver;

// 5-second reverb at 44.1 kHz ≈ 220 500 samples
let mut conv = PartitionedConvolver::<f32, 128>::new(220_500);

let ir: Vec<f32> = load_wav("cathedral.wav");
conv.set_ir(&ir);

let input = [0.5f32; 128];
let mut output = [0.0f32; 128];
conv.process(&input, &mut output);
```

### Choosing a convolution method

| IR length | Method | Per‑block cost (128‑sample block) |
|---|---|---|
| ≤ 128 | `DirectConvolver` | ~10 µs |
| 256…16384 | `OverlapAddConvolver` | ~60 µs (IR 2048) |
| > 16384 | `PartitionedConvolver` | ~104 µs (IR 65536) |

At 44.1 kHz, the per‑block budget is ~2.9 ms. Even the partitioned convolver
with a 65536‑sample IR uses only ~3.6 % of the budget.

## Spectrum analysis

`FftSpectrumAnalyzer<T>` implements the `SpectrumAnalyzer` trait from
`rill-core-dsp`. It applies a Hann window, runs the real FFT, and
computes per‑bin magnitudes:

```rust,no_run
use rill_fft::spectrum::FftSpectrumAnalyzer;
use rill_core_dsp::analyzer::SpectrumAnalyzer;

let mut analyzer = FftSpectrumAnalyzer::<f32>::new(256);

// Feed blocks of signal data
analyzer.process(Some(&signal_block), &mut output).unwrap();

// Query the magnitude spectrum
let spectrum = analyzer.spectrum();        // &[f32] — N/2+1 bins
let amp_440 = analyzer.amplitude_at(440.0, 44100.0);
```

Implements `Algorithm<T>` directly — can be used as a port‑level analyser
in any graph node.

## Frequency‑domain effects

### SpectralGate

A frequency‑domain noise gate. Transforms the signal block via overlap‑add
FFT, silences bins whose magnitude falls below a threshold, then transforms
back. Useful for noise reduction and creative spectral gating:

```rust,no_run
use rill_fft::effects::spectral_gate::SpectralGate;

let mut gate = SpectralGate::<f32, 128>::new();
gate.set_threshold(0.01);
gate.set_ratio(0.0);  // 0.0 = hard gate, 1.0 = passthrough

gate.process(&input, &mut output);
```

### SpectralDelay

Applies different delay times to different frequency bins. Lower frequencies
receive longer delays, creating metallic resonances, comb‑filter sweeps, and
spectral shimmer effects. Stores a circular buffer of past FFT frames:

```rust,no_run
use rill_fft::effects::spectral_delay::SpectralDelay;

// MAX_DELAY = 16 frames (~2048 samples at BUF=128)
let mut delay = SpectralDelay::<f32, 128, 16>::new();
delay.set_mix(0.5);
delay.set_feedback(0.3);

delay.process(&input, &mut output);
```

## Graph integration

`ConvolverNode` wraps `PartitionedConvolver` as a `Processor` graph node,
registered as `"rill/convolver"` when the `fft` feature is enabled:

```
[Source] → [ConvolverNode] → [Sink]
```

Parameters: `ir_gain` (0.0–4.0), `mix` (0.0–1.0), `ir_loaded` (bool).

To load an impulse response at runtime, obtain a reference to the node
and call `set_ir()`:

```rust,no_run
use rill_fft::nodes::convolver_node::ConvolverNode;

// After graph construction
let node: &mut ConvolverNode<f32, 128> = graph.get_node_mut(node_id).unwrap();
node.set_ir(&ir_samples);
```

## Performance (f32, x86_64, release profile)

| Operation | Size | Time | Throughput |
|---|---|---|---|
| `ComplexFft::forward` | 1024 | 6.7 µs | 153 Melem/s |
| `RealFft::forward` | 1024 | 6.2 µs | 165 Melem/s |
| `ComplexFft::forward` | 16384 | 177 µs | 92 Melem/s |
| `OverlapAddConvolver` | IR 2048, BUF 128 | 61 µs/block | ~2100 blocks/s |
| `PartitionedConvolver` | IR 65536, BUF 128 | 104 µs/block | ~9600 blocks/s |
| `DirectConvolver` | 128 taps, BUF 128 | 10 µs/block | 12.7 Melem/s |

The 16384‑point FFT shows O(*N* log *N*) scaling: 5.7 × larger than 1024,
11.6 × slower (5.7 log₂ 5.7 ≈ 14.3). Efficiency drops from 153 Melem/s at
1024 to 92 Melem/s at 16384 — mainly due to L2/L3 cache effects on the
twiddle table.

## DIY FFT design note

`rill-fft` implements a home‑grown Cooley–Tukey FFT rather than wrapping
`rustfft` or `realfft`. The decision was driven by:

1. **RT control** — scratch buffers and twiddle tables are fully owned by
   the FFT struct, allocated exactly once in the constructor. Third‑party
   libraries often use internal scratch allocation during `process()`.

2. **Safe Rust** — `#![deny(unsafe_code)]` guarantees no UB in the FFT
   path. The `wide` crate provides safe SIMD wrappers when the `simd`
   feature is enabled.

3. **Minimal dependencies** — only `num-complex` (already in `rill-core-dsp`
   for filter design) and `num-traits` (workspace dep).

4. **Predictable shapes** — sizes are always powers of two, matching the
   radix‑2 algorithm perfectly.

Benchmarks show the implementation is competitive with established FFT
libraries for the target sizes (64…16384), and the RT‑safety guarantees
are verified at the allocator level.

## Complex number support

### Complex matrix helpers (`rill-core-dsp`)

`ComplexMat2<T>` and `ComplexMat3<T>` provide closed‑form 2×2 and 3×3
complex matrix operations for filter analysis and RT signal processing:

```rust,no_run
use rill_core_dsp::complex_mat::ComplexMat2;
use num_complex::Complex;

let m = ComplexMat2::<f32>::new(
    Complex::new(2.0, 0.0), Complex::new(1.0, 0.0),
    Complex::new(1.0, 0.0), Complex::new(3.0, 0.0),
);

let det = m.det();
let inv = m.inv().unwrap();
let ev = m.eigenvalues().unwrap();
let [re, _im] = m.mul_vec(Complex::new(1.0, 0.0), Complex::new(0.0, 0.0));
```

Free functions provide canonical complex multiplication:

```rust,no_run
use rill_core_dsp::complex_mat::{mul_complex, mul_complex_add};
use num_complex::Complex;

let a = Complex::new(1.0f32, 2.0);
let b = Complex::new(3.0, 4.0);
let c = mul_complex(a, b);  // a * b

let mut acc = Complex::new(0.0, 0.0);
mul_complex_add(&mut acc, a, b);  // acc += a * b
```

### Real‑valued matrix types (`glam`)

`glam` is re‑exported from `rill-core` — zero dependencies, stack‑only,
SIMD‑accelerated `Mat2`/`Mat3`/`Mat4` and `Vec2`/`Vec3`/`Vec4`:

```rust,no_run
use rill_core::glam::{Mat2, Vec2, mat2, vec2};

let rot = mat2([0.866, 0.5], [-0.5, 0.866]);  // 60° rotation
let v = vec2(1.0, 0.0);
let r = rot * v;  // ≈ [0.866, 0.5]
```

## Complex numbers in rill‑lang

Eight built‑ins provide complex arithmetic in the DSL:

| Builtin | I/O channels | Description |
|---|---|---|
| `complex(re, im)` | 0 → 2 | Generator |
| `conj(x)` | 2 → 2 | Conjugate |
| `re(x)`, `im(x)` | 2 → 1 | Real / imaginary part |
| `norm(x)` | 2 → 1 | Magnitude |
| `arg(x)` | 2 → 1 | Phase (`atan2`) |
| `cmul(a, b)` | 4 → 2 | Complex multiply |
| `cadd(a, b)` | 4 → 2 | Complex add |

```faust
// (3+4i) × (2+0i) = 6+8i → extract real part
process = complex(3.0, 4.0), complex(2.0, 0.0) : cmul() : re();  // → 6.0

// norm of 3+4i
process = complex(3.0, 4.0) : norm();  // → 5.0
```

Spectral effects are also available as DSL builtins behind the `fft` feature:

```faust
process = _ : spectralgate(0.01, 0.0);             // spectral noise gate
process = _ : spectraldelay(0.5, 0.3);             // shimmer
process = _ : spectralgate(0.01, 0.0) : spectraldelay(0.5, 0.3);  // chain
```

## Examples

All examples live in `rill-adrift/examples/`:

```bash
cargo run --example convolver        --features fft
cargo run --example spectral_effects --features fft
cargo run --example complex_dsl      --features lang
cargo run --example dsl_spectral     --features "lang,fft"
```

## Dependencies and features

Add to your `Cargo.toml`:

```toml
[dependencies]
rill-adrift = { version = "0.5.0", features = ["fft"] }
# Or directly:
rill-fft = "0.5.0"
```

The `simd` feature forwards to `rill-core/simd`, enabling `wide`‑based
SIMD acceleration for FFT butterflies.
