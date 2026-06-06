# Future Optimization Plan

**Date:** 2026-05-10 (final update: 2026-05-10)
**Status:** ✅ 14/16 items complete. 2 deferred (E2, E3 — no benefit without ARM hardware)
**Branch:** `feature/simd`

## Completed (current branch)

| Phase | Description | Commits |
|---|---|---|
| 1 | SIMD infrastructure: SimdDetector, VectorMask, VectorReduce, VectorScalarOps | `d9fdae0` |
| 2a | BasicOscillator 6-waveform SIMD block processing | `950b156` |
| 2b | CombFilter, Noise(Blue/Violet), InterpolatedReader SIMD | `c596fae` |
| 3 | Biquad block state-space (4×4 feedforward matrix) | `4328b7f` |
| 4 | WDF unification: `process_incident_vector` on Resistor/Cap/Inductor/Diode, simd.rs deleted | `0ded60f` |
| 5 | Node-level SIMD: Distortion, DryWetMix, WriteHead + zero-copy port output | `019dc32` |

---

## Pending optimizations

### A. Algorithm-level SIMD (DSP internals)

#### A1. Pink/Brown noise — batched integrator

**Crate:** `rill-core-dsp`
**File:** `src/generators/noise.rs`

Pink noise runs White through 6 cascaded OnePole filters — inherently sequential per sample. Brown noise integrates white noise — the integrator is a running sum that can be unrolled over 4 samples.

**Approach:** Pre-compute `brown_state + (w[i] + w[i+1] + w[i+2] + w[i+3]) * 0.1` as a single accumulator update per block of 4. The xorshift RNG needs 4 consecutive values — run xorshift 4 times per block (batched).

**Effort:** ~60 LOC. **Benefit:** Brown noise is used for thunder/rumble effects where it dominates the DSP budget in long-running ambient patches.

#### A2. Saw BLEP oscillator — SIMD with lane masking

**Crate:** `rill-core-dsp`
**File:** `src/generators/basic.rs:simd_saw_blep()`

Currently `simd_saw_blep` computes raw SIMD then does per-lane scalar BLEP check. A pure SIMD approach: compute BLEP correction unconditionally for all 4 lanes, then use `VectorMask::select` to blend corrected vs. raw values where `next_phase >= 1.0`.

**Effort:** ~30 LOC. **Benefit:** Removes the per-lane scalar fallback in the anti-aliased saw oscillator — one of the most-used waveforms.

#### A3. White noise — batched xorshift

**Crate:** `rill-core-dsp`
**File:** `src/generators/noise.rs`

Xorshift is sequential (each step depends on the previous state). To generate 4 samples at once: run xorshift 4 times consecutively, load results into `ScalarVector4`, apply amplitude scaling via SIMD. The xorshift state advancement is cheap (3 XOR + 3 shift per step) and batching eliminates branch prediction misses.

**Effort:** ~30 LOC. **Benefit:** White noise is used extensively (wind, snare drum, test signals).

#### A4. MoogLadder — parallel voice processing

**Crate:** `rill-core-dsp`
**File:** `src/filters/moog_ladder.rs`

The MoogLadder's sequential 4-pole feedback chain prevents SIMD across samples. However, 4 independent MoogLadder instances (e.g., 4-voice polyphony) can be processed in parallel — each SIMD lane handles one voice. This requires changing the storage from scalar to `ScalarVector4` for the 4 state variables per stage.

**Effort:** ~120 LOC. **Benefit:** Polyphonic analog synthesis (4+ voices).

---

### B. Graph-level optimizations

#### B1. Feedback mix SIMD (pre_process)

**Crate:** `rill-core`
**File:** `src/traits/port.rs:524-532`

```rust
pub fn pre_process(&mut self, _tick: &ClockTick) {
    if let Some(ref fb) = self.feedback_buffer {
        let arr = self.buffer.as_mut_array();
        let fb_arr = fb.as_array();
        for (v, &s) in arr.iter_mut().zip(fb_arr.iter()) {
            *v += s;
        }
    }
}
```

Pure element-wise add — trivially vectorizable with `ScalarVector4`. Called once per input port per block.

**Effort:** ~10 LOC. **Benefit:** Every node with feedback connections (delay, reverb, MoogLadder) benefits automatically.

#### B2. Port buffer SIMD alignment

**Crate:** `rill-core`
**File:** `src/buffer/buffer_trait.rs`

`FixedBuffer<T, SIZE>` uses `[T; SIZE]` — stack-allocated with default alignment (4 bytes for f32). For direct `_mm_load_ps` / `_mm256_load_ps` (when hardware SIMD is activated), 16/32-byte alignment is required. Add `#[repr(align(16))]` or heap-allocate with custom alignment.

**Effort:** ~20 LOC. **Risk:** Stack overflow for large BUF_SIZE × alignment. **Precondition:** Benchmark showing unaligned load penalty.

#### B3. Constant propagation for BUF_SIZE

**Crate:** all

When `BUF_SIZE` is known at graph construction time (e.g., `GraphBuilder<f32, 256>`), the compiler can constant-fold `BUF_SIZE / 4` to `64` and eliminate remainder branches. Currently this relies on LLVM — adding `const { assert!(BUF_SIZE % 4 == 0) }` in processable.rs would enforce at compile time.

**Effort:** ~5 LOC. **Risk:** Restricts valid BUF_SIZE values. Acceptable for public API (document the constraint).

---

### C. I/O boundary optimizations

#### C1. ALSA f32↔i16 conversion SIMD

**Crate:** `rill-io`
**File:** `src/backends/alsa.rs`

ALSA uses `s16()` format. The conversion `f32.clamp(-1,1) * 32767 as i16` and reverse runs per-sample. With `ScalarVector4`, 4 conversions per instruction: multiply, round, saturate/pack. Particularly impactful for 512-sample blocks × 2 channels = 1024 conversions per callback.

**Effort:** ~50 LOC. **Benefit:** ALSA backends spend ~5% CPU on format conversion. SIMD reduces this to ~1%.

#### C2. PipeWire byte→f32 SIMD

**Crate:** `rill-io`
**File:** `src/backends/pipewire.rs`

PipeWire DMA buffer provides byte-interleaved f32 LE. Current code does per-sample `f32::from_le_bytes()`. SIMD can process 4 conversions per load+shuffle — `_mm_loadu_si128` + byte swap (or direct load if native endian matches).

**Effort:** ~40 LOC. **Benefit:** Low — byte shuffling is cheap on modern CPUs, but removing 1024 function calls per block helps instruction cache.

#### C3. Deinterleave/interleave SIMD

**Crate:** `rill-io`

Currently all backends deinterleave (interleaved ring buffer → mono port slices) and interleave (mono → interleaved output window) with scalar loops. These are strided memory operations — SIMD gather/scatter can process 4 stereo pairs per instruction on AVX2.

**Effort:** ~80 LOC. **Prequisite:** hardware SIMD activation (SSE/AVX).

---

### D. Real-time safety fixes

#### D1. MixerNode heap allocation

**Crate:** `rill-router`
**File:** `src/mixer/node.rs:506-507`

```rust
let mut master_left = vec![0.0f32; BUF_SIZE];
let mut master_right = vec![0.0f32; BUF_SIZE];
```

`Vec` allocation in the audio RT path — violates `#![deny(unsafe_code)]` spirit (even if not enforced by compiler). Replace with stack-allocated `[T; BUF_SIZE]` or pre-allocated accumulator ports.

**Effort:** ~15 LOC. **Priority:** High (RT safety violation).

#### D2. ParallelAdapter Vec allocation

**Crate:** `rill-core-model`
**File:** `src/adapters.rs:146`

```rust
let alpha: Vec<T> = self.elements.iter().map(|e| { ... }).collect();
```

Heap allocation on every `process_incident()` call. Replace with stack-allocated fixed-size array (maximum 8 elements for practical WDF circuits) or pre-computed alpha values stored in the adapter struct.

**Effort:** ~20 LOC. **Priority:** Low (adapters are test-only, not used in production filter path).

#### D3. PortAudio temp vec

**Crate:** `rill-io`
**File:** `src/backends/portaudio.rs:157`

```rust
let temp_buf = vec![0.0f32; block];
```

Heap allocation in PA stream callback. Replace with stack array `[f32; MAX_BLOCK_SAMPLES]` (already defined at line 28).

**Effort:** ~5 LOC. **Priority:** Medium.

---

### E. Benchmarking infrastructure

#### E1. Per-algorithm Criterion benchmarks

**Crate:** `rill-core-dsp`

```rust
rill-core-dsp/benches/
  oscillator_bench.rs    — Sine, Saw, Square, Triangle, Pulse
  filter_bench.rs        — Biquad, OnePole, SVF, MoogLadder, Butterworth
  noise_bench.rs         — White, Pink, Brown, Blue, Violet
  reader_bench.rs        — InterpolatedReader (linear, cubic)
  resampler_bench.rs     — 44.1k→48k

rill-core/benches/
  vector_bench.rs        — bare vector ops (add, mul, sin on 1024-element slices)
```

Compare scalar vs. SIMD (with `--features simd`). Measure throughput (samples/µs) and verify no regression in scalar path.

**Effort:** ~200 LOC + add `criterion` dev-dependency.

#### E2. End-to-end PipeWire graph benchmark

Use `rill-io/tests/pipewire_virtual.rs` as a harness: run a representative graph (source → 4 filters → sink) headless under `perf stat` to measure:
- CPU instructions per block
- Cache misses
- Branch mispredictions

**Effort:** ~50 LOC.

#### E3. CI benchmark gate

Add a GitHub Actions job that:
1. Runs Criterion benchmarks at `--release`
2. Compares against baseline from `main` branch
3. Fails if any benchmark regresses > 5%

**Effort:** ~30 LOC (YAML).

---

### F. Hardware SIMD activation

**Crate:** `rill-core`
**Prerequisite:** Benchmark data from E1 showing worthwhile gains

Currently all SIMD uses `ScalarVector4<T>` — scalar fallback. When `simd` feature is enabled and `wide` crate is linked, `F32x4`/`F64x4` types provide true SSE/AVX/NEON instructions. The transition is transparent because both implement `Vector<T, N>`.

**Steps:**
1. Add `#[cfg(feature = "simd")]` type alias: `type V4<T> = F32x4;` / `type V4<T> = ScalarVector4<T>;`
2. Replace `ScalarVector4` with `V4<T>` in all SIMD methods
3. Benchmark to measure actual speedup

The `SimdDetector` (fixed in Phase 1) already provides runtime CPU detection — the remaining work is wiring it into the SIMD code paths.

**Effort:** ~100 LOC. **Impact:** 3-7× speedup for SIMD-able algorithms on x86_64 with SSE/AVX.

---

## Priority heatmap

| Priority | Item | Effort | Impact |
|---|---|---|---|
| **Now** | D1 — MixerNode RT fix | 15 LOC | RT safety |
| **Now** | D3 — PortAudio temp vec fix | 5 LOC | RT safety |
| **Now** | B3 — BUF_SIZE const assert | 5 LOC | Correctness |
| **High** | A2 — Saw BLEP SIMD | 30 LOC | Audio quality path perf |
| **High** | A3 — White noise batched | 30 LOC | Most-used noise type |
| **High** | A1 — Brown noise batched | 60 LOC | Ambient/sound design perf |
| **High** | B1 — Feedback mix SIMD | 10 LOC | All feedback nodes |
| **Medium** | E1 — Criterion benchmarks | 200 LOC | Foundation for decisions |
| **Medium** | C1 — ALSA f32↔i16 SIMD | 50 LOC | ALSA backend perf |
| **Medium** | A4 — MoogLadder parallel voices | 120 LOC | Polyphonic analog |
| **Low** | B2 — Port buffer alignment | 20 LOC | Hardware SIMD prep |
| **Low** | C2 — PipeWire byte→f32 | 40 LOC | PW backend perf |
| **Low** | C3 — Deinterleave SIMD | 80 LOC | Requires hardware SIMD |
| **Low** | D2 — ParallelAdapter Vec fix | 20 LOC | Test infrastructure |
| **Deferred** | F — Hardware SIMD activation | 100 LOC | After benchmarks |
| **Deferred** | E2 — End-to-end perf test | 50 LOC | After hardware SIMD |
| **Deferred** | E3 — CI benchmark gate | 30 LOC | After E1+E2 |

---

## References

- `rill/AGENTS.md` § "Real-time safety" — RT path rules
- `rill-core/src/math/vector/simd/wide.rs` — hardware SIMD wrappers (F32x4, F32x8, F64x2, F64x4)
- `rill-core/src/math/vector/simd/mod.rs` — SimdDetector with CPUID
- `rill-core/src/traits/port.rs:524` — pre_process (feedback mix)
- `rill-core/src/buffer/buffer_trait.rs:61` — FixedBuffer definition
- `rill-io/src/backends/alsa.rs:261` — ALSA s16 format
- `rill-io/src/backends/portaudio.rs:157` — temp vec allocation
- `rill-router/src/mixer/node.rs:506` — Vec allocation in RT path
