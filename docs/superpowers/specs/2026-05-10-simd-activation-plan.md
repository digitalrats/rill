# SIMD Activation Plan

**Status:** ✅ completed (see benchmark results)
**Date:** 2026-05-10
**Target:** `rill-core`, `rill-core-dsp`, `rill-core-model`

## Executive Summary

Rill has SIMD infrastructure (`wide` crate, vector types, traits) but it is **not wired** into any DSP algorithm. All filters, oscillators, and effects operate sample-by-sample with `ScalarVector1<T>` (wrapped scalar). The `SimdDetector` is a stub, `VectorMask` is incomplete, and there are no benchmarks.

This plan activates SIMD in three stages:
1. **Foundation** — fix SIMD infrastructure gaps
2. **Trivially vectorizable** — algorithms with no feedback (oscillators, comb filter, simple noise)
3. **Advanced** — algorithms requiring block state-space reformulation (biquad, one-pole, SVF)

## Current State

### What works

| Component | Status | Location |
|---|---|---|
| `Vector<T, N>` trait | Complete, tested | `rill-core/src/math/vector/traits.rs` |
| `VectorTranscendental<T, N>` trait | Complete, tested | `rill-core/src/math/vector/traits.rs:63` |
| `ScalarVector1/2/4/8<T>` | Complete, tested | `rill-core/src/math/vector/scalar.rs` |
| `F32x4`, `F32x8`, `F64x2`, `F64x4` (wide) | Complete, tested | `rill-core/src/math/vector/simd/wide.rs` |
| `vec_map!` macro | Complete (hardcoded ScalarVector4) | `rill-core/src/math/vector/macros.rs` |
| `sin_slice`, `cos_slice`, `exp_slice` etc. | Complete, untested | `rill-core/src/math/vector/math.rs` |
| `add_slices`, `sub_slices`, `mul_slices` etc. | Complete, untested | `rill-core/src/math/vector/ops.rs` |
| WDF SIMD leaf elements | Complete (f64 only) | `rill-core-model/src/simd.rs` |

### What is broken or missing

| Issue | Severity | Impact |
|---|---|---|
| `SimdDetector` — always returns scalar width (hardcoded `false`) | **Critical** | No runtime SIMD dispatch; `recommended_simd_width()` returns 1 |
| `vec_expr!` / `vec_eval!` — identity stubs | **Medium** | Expression system disabled; `expr` module has compilation errors |
| `VectorMask` — only implemented for `F64x4` | **High** | Can't use SIMD comparisons/select on `F32x4`, `F32x8`, `F64x2` |
| `VectorReduce` — not implemented | **Low** | Horizontal sum/product not available |
| `VectorScalarOps` — not implemented | **Low** | No scalar-vector broadcasts |
| `expr` module — disabled | **Low** | Lazy expression evaluation not available |
| Zero SIMD usage in DSP algorithms | **Critical** | All DSP code uses `ScalarVector1<T>` (effectively scalar) |
| No benchmarks | **Medium** | Can't measure SIMD gains |
| x86/ARM/WASM native backends — scalar stubs only | **Low** | `wide` crate already provides portable SIMD; native backends are for direct intrinsics (future) |
| `vec_map!` hardcoded to `ScalarVector4` | **Medium** | Doesn't use `F32x4` from `wide` even when `simd` feature is on |

## Architecture: Where SIMD fits

### Signal processing flow

```
Backend callback (PW/JACK/PA/ALSA)
  │
  ├─ drain MpscQueue (parameter changes)
  │
  ├─ Source::generate()         ← SIMD opportunity: Input deinterleaving
  │
  ├─ Port::propagate()          ← BLOCK-level propagation (no SIMD needed)
  │     │
  │     ├─ copy buffer to downstream ports
  │     ├─ run_action() on each port
  │     └─ for each downstream node:
  │           ├─ pre_process() (feedback mix)  ← SIMD: element-wise add
  │           ├─ process_block()
  │           │     └─ Processor::process()     ← SIMD: DSP inner loop ★
  │           │           for i in 0..BUF_SIZE {
  │           │               out[i] = filter_math(inp[i]);  ← VECTORIZE THIS
  │           │           }
  │           ├─ snapshot_feedback()
  │           └─ propagate() (recurse)
  │
  └─ Sink::consume()           ← SIMD opportunity: Output interleaving
```

**Key insight:** SIMD lives **inside node `process()` methods**, not in the graph propagation layer. The graph already operates at block granularity; the per-sample loops inside nodes are what need vectorization.

### SIMD width strategy

```rust
// Runtime dispatch based on CPU features
fn simd_width<T>() -> usize {
    #[cfg(feature = "simd")]
    {
        if has_avx() { 8 }      // F32x8 on AVX
        else if has_sse() { 4 }  // F32x4 on SSE/NEON/WASM
        else { 1 }               // ScalarVector1 fallback
    }
    #[cfg(not(feature = "simd"))]
    { 1 }
}

// Algorithm pattern after SIMD activation:
fn process(&mut self, _input: Option<&[T]>, output: &mut [T], _ctx: &ActionContext) {
    const W: usize = simd_width::<T>();  // evaluated once at monomorphization
    let chunks = output.len() / W;

    for chunk in 0..chunks {
        let offset = chunk * W;
        let out_vec = self.process_vector(&input[offset..offset + W]);
        out_vec.store(&mut output[offset..offset + W]);
    }

    // Remainder: scalar fallback for last 0..W-1 samples
    for i in chunks * W..output.len() {
        output[i] = self.process_scalar(input[i]);
    }
}
```

## Phase 1: Foundation Fixes

**Target crate:** `rill-core`
**Effort:** ~200 LOC
**When:** blocked on nothing — pure infrastructure completion

### 1.1 Fix SimdDetector

```rust
// rill-core/src/math/vector/simd/mod.rs
use std::arch::is_x86_feature_detected;

impl SimdDetector {
    pub fn new() -> Self {
        Self {
            #[cfg(target_arch = "x86_64")]
            has_sse2: is_x86_feature_detected!("sse2"),
            #[cfg(target_arch = "x86_64")]
            has_sse4_1: is_x86_feature_detected!("sse4.1"),
            #[cfg(target_arch = "x86_64")]
            has_avx: is_x86_feature_detected!("avx"),
            #[cfg(target_arch = "x86_64")]
            has_avx2: is_x86_feature_detected!("avx2"),
            #[cfg(target_arch = "x86_64")]
            has_avx512: is_x86_feature_detected!("avx512f"),
            #[cfg(target_arch = "aarch64")]
            has_neon: std::arch::is_aarch64_feature_detected!("neon"),
            // wasm simd128: always available on simd-enabled wasm targets
            #[cfg(target_arch = "wasm32")]
            has_wasm_simd128: true,
            ..Self::empty() // rest = false
        }
    }

    pub fn recommended_simd_width<T: crate::Transcendental>() -> usize {
        let det = Self::new();
        if det.has_avx { return 8; }       // f32x8
        if det.has_sse2 || det.has_neon || det.has_wasm_simd128 { return 4; } // f32x4
        1 // scalar fallback
    }
}
```

**Dependency:** Requires `std` (CPU detection via `std::arch`). Acceptable — rill requires `std` (it uses `tokio`, `parking_lot`, etc.).

**On `no_std` target architectures:** `is_x86_feature_detected!` works in `no_std` on x86. On other architectures without runtime detection, the `#[cfg]` gates ensure only supported targets are included.

### 1.2 Complete VectorMask for all SIMD types

Currently only `F64x4` implements `VectorMask`. Need to implement for:
- `F32x4` — SSE NEON WASM-SIMD
- `F32x8` — AVX
- `F64x2` — SSE2 NEON
- `ScalarVector4<T>` — scalar fallback

Each implementation delegates to `wide` crate's `CmpEq`, `CmpNe`, `CmpGt`, `CmpGe`, `CmpLt`, `CmpLe` traits and `blend`/`move_mask` methods.

**Effort:** ~150 LOC (delegate to `wide` crate per type, same pattern as `F64x4`).

### 1.3 Implement VectorReduce

```rust
pub trait VectorReduce<T: Scalar, const N: usize>: Vector<T, N> {
    fn horizontal_sum(&self) -> T;
    fn horizontal_product(&self) -> T;
    fn horizontal_min(&self) -> T;
    fn horizontal_max(&self) -> T;
    fn horizontal_mean(&self) -> T { self.horizontal_sum() / T::from_usize(N) }
}
```

For SIMD types, `horizontal_sum` can use shuffle+add reduction. For scalar fallback, use `fold`.

**Effort:** ~80 LOC.

### 1.4 Implement VectorScalarOps

```rust
pub trait VectorScalarOps<T: Scalar, const N: usize>: Vector<T, N> {
    fn add_scalar(&self, scalar: T) -> Self { self.add(&Self::splat(scalar)) }
    fn mul_scalar(&self, scalar: T) -> Self { self.mul(&Self::splat(scalar)) }
    fn sub_scalar(&self, scalar: T) -> Self { self.sub(&Self::splat(scalar)) }
}
```

Provide default impls via `splat` + vector op (acceptable performance for now).

**Effort:** ~30 LOC.

### 1.5 Fix or remove expr module

The `expr` module is disabled with a comment: `"temporarily disabled due to compilation errors"`. Either fix the compilation errors or remove the module and its associated stubs (`vec_expr!`, `vec_eval!`).

**Decision:** Remove `expr` module and `vec_expr!`/`vec_eval!` stubs. The expression system is not needed for the SIMD activation plan. If needed later, it can be re-implemented.

**Effort:** ~10 LOC (remove)

### 1.6 Make vec_map! SIMD-aware

Current `vec_map!` is hardcoded to `ScalarVector4`. Update to dispatch based on SIMD feature:

```rust
macro_rules! vec_map {
    ($input:expr, $output:expr, |$x:ident| $($body:tt)*) => {{
        #[cfg(feature = "simd")]
        {
            use $crate::math::vector::simd::wide::F32x4;
            // ... can use whichever type is available
        }
        // scalar fallback
    }};
}
```

**Effort:** ~50 LOC.

### 1.7 Add benchmarks

Add `rill-core/benches/vector_bench.rs` using Criterion:

```rust
// Benchmark categories:
// 1. Vector add/mul on 1024-element slices (scalar vs SIMD)
// 2. Vector sin/cos/exp on 1024-element slices
// 3. Biquad filter — scalar vs theoretical 4-wide SIMD
// 4. Sine oscillator — scalar vs 4-wide SIMD
```

**Effort:** ~100 LOC + Criterion dev-dependency.

### Phase 1 Checklist

- [ ] Fix `SimdDetector::new()` — real CPU detection
- [ ] Fix `SimdDetector::recommended_simd_width()` — return actual width
- [ ] `VectorMask` for `F32x4`, `F32x8`, `F64x2`, `ScalarVector4`
- [ ] `VectorReduce` trait + impls
- [ ] `VectorScalarOps` trait + impls
- [ ] Remove disabled `expr` module
- [ ] `vec_map!` SIMD dispatch
- [ ] Criterion benchmarks
- [ ] All existing tests pass (152 in `rill-core` + 10 in `test_vector.rs`)

## Phase 2: Trivially Vectorizable Algorithms

**Target crate:** `rill-core-dsp`
**Effort:** ~500 LOC
**Prerequisite:** Phase 1 complete

### Definition of "Trivially vectorizable"

An algorithm is trivially vectorizable if:
1. Each output sample depends only on the corresponding input sample + constants + global state
2. No feedback path from `output[n]` to `output[n+1]`
3. The inner loop is `for i in 0..len { output[i] = f(input[i], state) }` where `f` is element-wise

### Algorithm migration plan

| Algorithm | File | SIMD Width | Key Change |
|---|---|---|---|
| `BasicOscillator` (Sine) | `basic.rs:106` | 4 | Compute 4 phases, `sin()` on vector |
| `BasicOscillator` (Triangle) | `basic.rs:155` | 4 | `abs(sub(phase, 0.5))` on vector |
| `BasicOscillator` (Square) | `basic.rs:144` | 4 | `select(amp, -amp, phase < 0.5)` |
| `BasicOscillator` (Pulse) | `basic.rs:165` | 4 | Same as Square with variable threshold |
| `BasicOscillator` (Saw raw) | `basic.rs:113` | 4 | `2*phase - 1` on vector |
| `CombFilter` | `comb.rs:67` | 4* | *Only when `delay_samples >= 4` |
| `NoiseGenerator` (Blue) | `noise.rs:150` | 4 | Shift-and-subtract vector |
| `NoiseGenerator` (Violet) | `noise.rs:162` | 4 | Double shift-and-subtract vector |
| `LFO` | `lfo.rs:125` | 4 | Same as BasicOscillator |
| `InterpolatedReader` (linear) | `reader.rs:195` | 4 | Compute 4 positions, 4 lerps, gather loads |
| `Resampler` | `resampler.rs` | 4 | Delegates to `InterpolatedReader` |

### Sine oscillator example (before/after)

**Before (scalar):**
```rust
fn generate_sine(&self) -> ScalarVector1<T> {
    let phase = self.phase.extract(0);
    let value = (phase * PI2).sin();
    ScalarVector1::splat(value) * self.amplitude
}
```

**After (SIMD):**
```rust
fn generate_block_simd(&mut self, output: &mut [T]) {
    const W: usize = 4; // or runtime-detected
    let chunks = output.len() / W;
    let mut phase = self.phase;
    let inc = self.phase_inc;
    let amp = self.amplitude;
    let pi2 = T::PI * T::from_f32(2.0);

    for chunk in 0..chunks {
        let offset = chunk * W;
        // Compute 4 phases: p, p+inc, p+2*inc, p+3*inc
        let p0 = phase;
        let p1 = phase + inc;
        let p2 = phase + inc + inc;
        let p3 = phase + inc + inc + inc;
        phase = phase + inc * T::from_usize(W);

        // Load into SIMD vector, compute sin, scale
        let phases = V4::new(p0, p1, p2, p3);
        let values = (phases * V4::splat(pi2)).sin() * V4::splat(amp);
        values.store(&mut output[offset..offset + W]);
    }

    self.phase = phase;
    // Handle remainder with scalar fallback...
}
```

### Saw BLEP (bandlimited) approach

The bandlimited sawtooth uses a per-sample conditional: `if next_phase >= 1.0 { BLEP correction }`. For SIMD, compute BLEP correction unconditionally for all lanes, then use `VectorMask::select()` to blend:

```rust
let next_phases = phases + V4::splat(inc);
let wrap_mask = next_phases.ge(&V4::splat(T::ONE));
let raw = phases * V4::splat(T::from_f32(2.0)) - V4::splat(T::ONE);
let blep = compute_blep_vector(next_phases); // polynomial correction
let result = V4::select(&wrap_mask, &(raw - blep), &raw);
```

### Phase 2 Checklist

- [ ] `BasicOscillator` — 6 waveforms, SIMD `generate_block()` path
- [ ] `CombFilter` — SIMD path when `delay_samples >= W`
- [ ] `NoiseGenerator` — Blue/Violet SIMD + batched xorshift for White
- [ ] `LFO` — SIMD path (delegates to `BasicOscillator`)
- [ ] `InterpolatedReader` — SIMD `render_block()` with 4-way positions
- [ ] `Resampler` — inherits `InterpolatedReader` SIMD
- [ ] Scalar fallback preserved (when `simd` feature off, or remainder samples)
- [ ] All existing tests pass

## Phase 3: Algorithms Requiring Block State-Space Reformulation

**Target crate:** `rill-core-dsp`
**Effort:** ~800 LOC
**Prerequisite:** Phase 2 complete

### Biquad — Block State-Space

Direct Form II Transposed biquad has 4 feedback states. The `n`th sample depends on `n-1` and `n-2`. To process 4 samples at once, the 4-output block can be expressed as:

```
[y[n]]   [b0 0 0 0] [x[n]]   [a1*b0+b1     a2*b0+b2     0  0] [y[n-1]]
[y[n+1]]=[b1 b0 0 0] [x[n+1]]+[a1*b1+a2*b0 a2*b1  0  0] [y[n-2]]
[y[n+2]] [b2 b1 b0 0] [x[n+2]] [a1*b2        a2*b2     0  0] [y[n-3]]
[y[n+3]] [0  b2 b1 b0][x[n+3]] [a1*0+a2*b0   a2*0+a2*b1  0  0] [y[n-4]]
```

This is a 4×4 matrix-vector multiply — ideal for SIMD. Coefficients are recomputed when filter parameters change.

**Effort:** ~200 LOC per filter type (Biquad, OnePole, SVF).

### OnePole — Geometric Series Unrolling

```rust
// y[n] = alpha*x[n] + (1-alpha)*y[n-1]
// y[n+1] = alpha*x[n+1] + (1-alpha)*alpha*x[n] + (1-alpha)^2*y[n-1]
// y[n+2] = alpha*x[n+2] + (1-alpha)*alpha*x[n+1] + (1-alpha)^2*alpha*x[n] + (1-alpha)^3*y[n-1]
// y[n+3] = alpha*x[n+3] + (1-alpha)*alpha*x[n+2] + ... + (1-alpha)^4*y[n-1]
```

Pre-compute `feedback_pow = [(1-a)^1, (1-a)^2, (1-a)^3, (1-a)^4]` and `feedforward = [a, a*(1-a), a*(1-a)^2, a*(1-a)^3]`.

**Effort:** ~100 LOC.

### SVF — 3×3 Block Matrix

State variables `lp, hp, bp` are mutually dependent within a sample but independent across samples. Block form: compute `[lp[n+3], hp[n+3], bp[n+3]]` from `[lp[n-1], hp[n-1], bp[n-1]]` via a pre-computed matrix.

**Effort:** ~200 LOC.

### WavetableOscillator — Gather Loads

The main challenge is reading 4 non-contiguous samples from `Box<[T]>`. On x86 with AVX2, use `_mm_i32gather_ps`. For SSE/NEON, use 4 scalar loads + insert into vector. Performance depends on cache locality (wavetables are typically 256-4096 samples — L1 resident).

**Effort:** ~150 LOC.

### Noise — Batched RNG

White noise uses xorshift which is sequential. To generate 4 samples at once:
- Run xorshift 4 times (batched — each advance is independent, just compute 4 states)
- Or use 4 parallel RNG states

**Effort:** ~80 LOC.

### Phase 3 Checklist

- [ ] `Biquad` — block state-space, 4×4 matrix per 4 samples
- [ ] `BiquadSection` — used by Butterworth/Chebyshev, same approach
- [ ] `OnePole` — geometric series unrolling
- [ ] `SVF` — 3×3 block matrix
- [ ] `Butterworth` — cascaded BiquadSections (each section has independent SIMD)
- [ ] `ChebyshevI` / `ChebyshevII` — same cascaded approach
- [ ] `WavetableOscillator` — SIMD `render_block()` with gather loads
- [ ] `NoiseGenerator` (White, Brown, Pink) — batched RNG + unrolled integrators
- [ ] Scalar fallback for remainder samples
- [ ] All existing tests pass (75 tests in `rill-core-dsp`)

## Phase 4: WDF — Unified Trait (Lower Priority)

**Target crate:** `rill-core-model`
**Effort:** ~350 LOC net (delete ~400, write ~750)
**Prerequisite:** Phase 1 complete (`VectorMask` for all types, `SimdDetector`)

### The problem: two parallel type hierarchies

The current WDF crate maintains **two independent worlds** linked by nothing:

```
Scalar side (prod):              SIMD side (simd.rs, separate!):
  WdfElement<T>                    SimdWdfElement
  Resistor                         SimdResistor
  Capacitor                        SimdCapacitor
  Diode                            SimdDiode
  SeriesAdapter                    (none)
  ParallelAdapter                  (none)
  wdf_cascade! → MoogLadder        (none)
```

This means:
- SIMD adapters would need their own independent implementations — doubling maintenance
- Adding SIMD to a filter means writing a parallel `*Simd` struct with identical physics
- 378 lines in `simd.rs` cover only leaf elements — the heavy parts (adapters, filters) remain scalar

### Solution: Unify through `Vector<T, N>` from `rill-core`

Replace TWO traits with ONE, generic over vector width `N`:

```rust
// OLD — two unrelated traits:
pub trait WdfElement<T: Transcendental>: Send + Sync {
    fn process_incident(&mut self, a: T) -> T;              // scalar
}
pub trait SimdWdfElement: Send + Sync {
    type SimdType;
    fn process_incident_simd(&mut self, a: Self::SimdType) -> Self::SimdType; // SIMD
}

// NEW — one trait, scalar when N=1, SIMD when N=4/8:
pub trait WdfElement<T: Transcendental, const N: usize>: Send + Sync
where
    ScalarVector<T, N>: Vector<T, N> + VectorTranscendental<T, N> + VectorMask<T, N>,
{
    type V: Vector<T, N> + VectorTranscendental<T, N> + VectorMask<T, N>;

    fn port_resistance(&self) -> Self::V;
    fn process_incident(&mut self, a: Self::V) -> Self::V;
    fn update_state(&mut self);
    fn voltage(&self) -> Self::V;
    fn current(&self) -> Self::V;
    fn reset(&mut self);
}
```

**Backward compatibility** via type alias:

```rust
// Existing code using WdfElement<f64> continues to work:
type WdfElement1<T> = WdfElement<T, 1>;
```

### What gets unified

| Old (2 structs per element) | New (1 struct) |
|---|---|
| `Resistor<T>` + `SimdResistor` | `Resistor<V>` |
| `Capacitor<T>` + `SimdCapacitor` | `Capacitor<V>` |
| `Inductor<T>` *(scalar only)* | `Inductor<V>` |
| `Diode<T>` + `SimdDiode` | `Diode<V>` |
| `OpAmp<T>` *(scalar only)* | `OpAmp<V>` |
| `SeriesAdapter<T>` *(scalar only)* | `SeriesAdapter<V>` |
| `ParallelAdapter<T>` *(scalar only)* | `ParallelAdapter<V>` |

All math delegates to `rill-core::math::vector` traits (`+ - * / sqrt exp ln abs min max clamp select`). Zero WDF-specific SIMD code — the vector infrastructure carries it.

### Element example — Capacitor (before/after)

**Before (scalar only):**
```rust
pub struct Capacitor<T: Transcendental> {
    capacitance: f64,
    sample_rate: f64,
    port_resistance: T,
    state: T,
}

impl<T: Transcendental> WdfElement<T> for Capacitor<T> {
    fn process_incident(&mut self, a: T) -> T {
        self.state - a
    }
}
```

**After (generic over vector width):**
```rust
pub struct Capacitor<V> {
    capacitance: f64,
    sample_rate: f64,
    port_resistance: V,
    state: V,
}

impl<T: Transcendental, const N: usize> WdfElement<T, N>
    for Capacitor<<Self as WdfElement<T, N>>::V>
{
    type V = ScalarVector<T, N>;

    fn process_incident(&mut self, a: Self::V) -> Self::V {
        // Same math — works for N=1 (scalar) and N=4 (SIMD)
        self.state - a
    }
}
```

### Diode Newton-Raphson — the only per-element nuance

The diode uses iterative Newton-Raphson. In scalar mode (N=1) we iterate all 10 times. In SIMD mode (N=4) we can early-exit when all lanes converge:

```rust
fn solve_newton<V>(&self, a: V, r: V) -> V
where
    V: Vector<T, N> + VectorTranscendental<T, N> + VectorMask<T, N>,
{
    let guess = self.vt * (V::ONE + a / (r * self.is)).ln();
    let mut v = guess.max(&V::ZERO);

    for _ in 0..10 {
        let i = self.is * ((v / self.vt).exp() - V::ONE);
        let g = self.is * (v / self.vt).exp() / self.vt;
        let f = v + r * i - a;

        if N > 1 {
            // SIMD path — early exit when all 4 lanes converged
            if f.abs().lt(&self.tolerance).all() { break; }
        }

        let df = V::ONE + r * g;
        v = v - f / df;
    }
    v
}
```

### Filters get SIMD for free

Because `MoogLadder`, `RcPole`, and `DiodeClipper` are built via macros (`wdf_cascade!`, `wdf_compose!`) that compose `WdfElement` traits, they automatically become SIMD-capable once the underlying elements are generic:

```rust
// Before: scalar-only cascade
wdf_cascade! {
    struct MoogLadder<T: Transcendental> { ... }
    // inner loop: a = WdfElement::process_incident(&mut self.poles[i], a);
}

// After: generic over N
wdf_cascade! {
    struct MoogLadder<T: Transcendental, const N: usize> { ... }
    // inner loop: a = WdfElement::process_incident(&mut self.poles[i], a);
    //                   ^^^ same code, works for N=1 and N=4 ^^^
}
```

### What gets removed

| File/Item | LOC | Reason |
|---|---|---|
| `rill-core-model/src/simd.rs` | 378 | Superseded by unified `WdfElement<T,N>` — zero SIMD-specific code needed |
| `SimdWdfElement` trait | — | Replaced by `WdfElement<T,N>` |
| `SimdResistor`, `SimdCapacitor`, `SimdDiode` | — | Replaced by generic `Resistor<V>`, `Capacitor<V>`, `Diode<V>` |
| `process_batch_simd()` | — | Not needed — `process_incident(V)` already handles N lanes |

### Phase 4 Checklist

- [ ] Parameterize `WdfElement<T, N>` trait — add `const N: usize` + `type V` with vector bounds
- [ ] Convert `Resistor`, `Capacitor`, `Inductor` to `*<V>` generic structs
- [ ] Convert `Diode`, `OpAmp` to `*<V>` — Newton-Raphson on `V` with N-dependent early exit
- [ ] Update `wdf_element!` macro to emit generic `V`-based structs
- [ ] Update `wdf_compose!` macro (Series/Parallel of 2 elements)
- [ ] Update `wdf_cascade!` macro (N-section cascade with feedback) — MoogLadder, DiodeClipper
- [ ] Convert `SeriesAdapter`, `ParallelAdapter` to `*<V>` generic
- [ ] Delete `rill-core-model/src/simd.rs`
- [ ] Add backward-compatible type alias: `type WdfElement1<T> = WdfElement<T, 1>`
- [ ] Scalar tests pass (N=1, zero regression in 21 WDF tests)
- [ ] SIMD tests: verify element-by-element output matches scalar within 1e-12 for f64
- [ ] Wire SIMD path into `WdfMoogLadder` processor node in `rill-analog-filters`

## Phase 5: I/O Boundary Optimization (Lowest Priority)

**Target crate:** `rill-io`
**Effort:** ~300 LOC

### Opportunities

| Backend | Operation | SIMD gain |
|---|---|---|
| PipeWire | byte→f32 conversion (input) | Low (byte shuffling per sample) |
| PipeWire | f32→byte conversion (output) | Low |
| ALSA | f32↔i16 conversion | **Medium** — 512 samples × 2 channels = 1024 conversions per block |
| All backends | Deinterleave (ring buffer → mono) | Low (non-unit strided stores) |
| All backends | Interleave (mono → output window) | Low (non-unit strided loads) |
| `pre_process()` | Feedback mix-in (port.rs:527) | **Medium** — element-wise add on BUF_SIZE samples |

### Recommendation

Defer I/O SIMD until there is benchmark data showing a bottleneck. DSP algorithm SIMD (Phases 2-3) will have far greater impact than I/O conversion SIMD.

## Benchmarks

### Per-algorithm benchmarks

Each algorithm gets a Criterion benchmark comparing:
1. Scalar (current) — baseline
2. SIMD (new path) — with `simd` feature enabled
3. SIMD fallback (simd feature off but type is SIMD-capable) — regression check

### Benchmarks to add

```
rill-core-dsp/benches/
  oscillator_bench.rs    — Sine, Saw, Square, Triangle, Pulse (all waveforms)
  filter_bench.rs        — Biquad, OnePole, SVF, MoogLadder, Butterworth
  noise_bench.rs         — White, Pink, Brown, Blue, Violet
  reader_bench.rs        — InterpolatedReader (linear, cubic, wrap)
  resampler_bench.rs     — 44.1k→48k conversion

rill-core/benches/
  vector_bench.rs        — bare vector ops (add, mul, sin, exp on 1024-element slices)
```

### Success criteria

| Algorithm | Target speedup on x86_64 (SSE2) | Target speedup on x86_64 (AVX) | Target speedup on AArch64 (NEON) |
|---|---|---|---|
| Sine oscillator | 3.5× | 7× | 3.5× |
| Saw oscillator | 3.2× | 6.5× | 3.2× |
| Triangle oscillator | 3.0× | 6× | 3.0× |
| Biquad | 2.5× | 5× | 2.5× |
| OnePole | 2.5× | 5× | 2.5× |
| InterpolatedReader (linear) | 2.0× | 4× | 2.0× |
| Butterworth (4 sections) | 2.0× | 4× | 2.0× |

Theoretical max for 4-wide f32 SIMD is 4× (assuming no memory bottlenecks, no divergent branches). Realistic targets are 60-90% of theoretical due to:
- Remainder handling overhead
- Gather/scatter overhead for wavetable reads
- Branch divergence for BLEP/conditionals

## Dependency Considerations

**No new external dependencies needed.** The `wide` crate (v0.7) is already an optional dependency behind the `simd` feature flag. CPU detection uses `std::arch` (stable since Rust 1.27).

`Criterion` is needed as a **dev-dependency** for benchmarks only.

## Risk Mitigation

### Risk: SIMD path introduces bugs not caught by scalar tests

**Mitigation:** Add `assert_eq!` checks in tests comparing scalar and SIMD outputs (tolerance 1e-6 for f32, 1e-12 for f64). Run both paths on the same input and verify bit-identical (or within rounding error).

### Risk: SIMD remainder handling is complex and error-prone

**Mitigation:** Standardize the remainder pattern via a helper macro or function:

```rust
/// Process a slice in SIMD chunks with scalar remainder.
fn process_simd<T, F, G>(output: &mut [T], simd_fn: F, scalar_fn: G)
where F: Fn(&mut [T; W]),
      G: Fn(&mut T)
{
    let (chunks, remainder) = output.as_chunks_mut::<W>();
    for chunk in chunks { simd_fn(chunk); }
    for sample in remainder { scalar_fn(sample); }
}
```

Note: `as_chunks_mut` is stabilized in Rust 1.77+.

### Risk: Different CPU features cause different code paths — hard to test

**Mitigation:** Use `#[cfg(target_arch = "x86_64")]`-gated tests with `is_x86_feature_detected!` guards. Run CI on x86_64 (SSE/AVX), AArch64 (NEON via QEMU or Apple Silicon runner), and a `--no-default-features` (scalar) build.

### Risk: Benchmark improvements don't translate to real-world use

**Mitigation:** Measure end-to-end via the PipeWire virtual device test (`rill-io/tests/pipewire_virtual.rs`) — compare xruns and CPU usage with `perf stat` on a representative graph (source → 4 filters → sink).

## Implementation Order Summary

| Phase | Priority | Effort | Impact |
|---|---|---|---|
| 1 — Foundation fixes | **Now** | ~200 LOC | Enables everything else |
| 2 — Trivially vectorizable | **High** | ~500 LOC | Sine/Saw/Triangle/Square — the most-used oscillators |
| 3 — Block state-space | **Medium** | ~800 LOC | Biquad/OnePole — the most-used filters |
| 4 — WDF unified trait | **Low** | ~350 LOC net | Deletes 378 LOC simd.rs, replaces with generic WdfElement<T,N>. All filters get SIMD automatically |
| 5 — I/O SIMD | **Deferred** | ~300 LOC | Wait for benchmarks to justify |

## References

- `rill-core/src/math/vector/` — SIMD vector infrastructure
- `rill-core/src/math/vector/simd/wide.rs` — `wide` crate wrappers (F32x4, F32x8, F64x2, F64x4)
- `rill-core/src/math/vector/traits.rs` — `Vector<T, N>`, `VectorTranscendental<T, N>`, `VectorMask<T, N>`, `VectorReduce<T, N>`
- `rill-core-dsp/src/filters/` — all filter implementations with scalar loops
- `rill-core-dsp/src/generators/` — all generator implementations with scalar loops
- `rill-core-model/src/simd.rs` — existing WDF SIMD leaf elements (**to be deleted** in Phase 4)
- `rill-core-model/src/lib.rs:105` — `WdfElement<T>` trait (**to be unified** with `Vector<T,N>` in Phase 4)
- `rill-core-model/src/elements.rs` — Resistor, Capacitor, Inductor, Diode, OpAmp scalar impls
- `rill-core-model/src/adapters.rs` — SeriesAdapter, ParallelAdapter scalar impls
- `rill-core-model/src/macros/cascade.rs:45` — `wdf_cascade!` macro-generated `process_sample()`
- `rill-core/src/traits/port.rs:569` — `Port::propagate()` — the processing loop (block-granular, no SIMD needed)
- `rill-core/src/traits/port.rs:527` — `pre_process()` — feedback mix (element-wise add, SIMD-able)
- `rill-io/src/backends/alsa.rs` — ALSA f32↔i16 conversion (SIMD-able)
- `rill/AGENTS.md` — RT safety rules (must maintain under SIMD)
