# dasp Bridge — Design Spec

**Status:** deferred (spec exists for future implementation)
**Date:** 2026-05-10
**Target:** `rill-core` crate, behind feature flag `dasp-bridge`

## Motivation

Rill lives in its own type ecosystem (`Transcendental`, `ScalarVector`, `Algorithm`).
Developers using crates from the `rustaudio/dasp` ecosystem (`dasp`, `cpal`, `hound`,
`rodio`, `fundsp`) face manual type conversion at the boundary when they want to use
rill for filtering, effect processing, or analysis.

The goal of the dasp bridge is **not** to replace rill internals with dasp types,
but to provide a zero-friction entry point: load audio via dasp/cpal/hound, process
through rill graph, output back to dasp-compatible types — without manual per-sample
conversion loops in user code.

The bridge is intentionally kept behind a feature flag so that dasp is never pulled
transitively by rill users who don't need it.

## Non-goals

- Do NOT replace `Transcendental` with `dasp::Sample`.
- Do NOT modify existing rill algorithms to accept dasp types natively.
- Do NOT pull dasp into the default dependency tree.
- Do NOT introduce `unsafe` in rill code (the bridge wraps dasp's unsafe internally).

## Architecture

```
┌────────────────────────── rill graph (RT-safe) ──────────────────────────────┐
│                                                                               │
│  [Node A] ──ptr──→ [Node B] ──ptr──→ [Node C] ──ptr──→ [DaspSink]           │
│                                                              │               │
│                                                         type conversion      │
│                                                              │               │
│                                                         dasp Frame output    │
└──────────────────────────────────────────────────────────────────────────────┘
                                                               │
                                                    ┌──────────┴──────────┐
                                                    │   external dasp     │
                                                    │   consumer / cpal   │
                                                    └─────────────────────┘


┌─────────────────── external dasp source / hound ───────────────────────────┐
│                                                                             │
│  [dasp Signal] ──→ [DaspSource]                                            │
│                         │                                                   │
│                    type conversion                                          │
│                         │                                                   │
│                         ▼                                                   │
└────────────────────────────────────────────────────────────────────────────┘
                         │
┌────────────────────────┴────────────────────────────────────────────────────┐
│  [Node D] ──ptr──→ [Node E] ──ptr──→ [Node F]                               │
│                                                                              │
│                            rill graph (RT-safe)                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

The bridge sits **strictly at the graph boundary**. dasp types never penetrate
into rill DSP algorithms. Conversion happens once per block at the edge.

## Types provided

### `DaspSource<T, S>` — dasp Signal → rill Algorithm

Located in `rill-core` (feature-gated), NOT in `rill-core-dsp` (to avoid
dasp dependency in the DSP crate).

```rust
/// Wraps a dasp `Signal` as a rill `Algorithm`, converting frames
/// to interleaved samples at the graph boundary.
///
/// # RT safety
///
/// - `S` (the signal) must be constructed before entering the RT path.
/// - All heap allocation happens at construction time.
/// - `process()` calls `S::next()` per output sample — O(N) per block.
pub struct DaspSource<T, S>
where
    T: Transcendental,
    S: dasp::Signal<Frame = FrameOf<T>>,
{
    signal: S,
    sample_rate: f32,
}

impl<T: Transcendental, S: dasp::Signal<Frame = FrameOf<T>>> Algorithm<T>
    for DaspSource<T, S>
{
    fn init(&mut self, sample_rate: f32) { self.sample_rate = sample_rate; }
    fn process(&mut self, _input: Option<&[T]>, output: &mut [T], _ctx: &ActionContext) -> ProcessResult<()> {
        for out in output.iter_mut() {
            let frame = self.signal.next();
            *out = frame.extract_channel(0); // mono only; stereo via paired DaspSource
        }
        Ok(())
    }
}
```

**Design decision — mono only:** `Algorithm<T>::process` operates on `&[T]`
(mono interleaved). For stereo, the user instantiates two `DaspSource` nodes
and extracts channels separately, following rill's existing pattern where
stereo is two mono ports, not interleaved `[f32; 2]` frames.

### `DaspSink<T, S>` — rill Algorithm → dasp Signal

Reverse direction: a rill output port feeds samples into a dasp-compatible
ring buffer that can be consumed as a `dasp::Signal`.

```rust
/// Collects rill output into a dasp-compatible buffer consumable
/// as a `dasp::Signal`.
///
/// Internally uses `rill_core::buffer::RingBuffer` (RT-safe, no allocation).
pub struct DaspSink<T, const N: usize>
where
    T: Transcendental,
{
    buffer: RingBuffer<T, N>,
    read_pos: usize,
}

impl<T: Transcendental, const N: usize> Algorithm<T> for DaspSink<T, N> { ... }

// DaspSink also implements dasp::Signal for consumption outside rill
impl<T: Transcendental, const N: usize> dasp::Signal for DaspSink<T, N> { ... }
```

### `FrameOf<T>` — bridge type

A minimal dasp `Frame` implementation that maps directly to rill's
`ScalarVector1<T>`, keeping conversion cheap.

```rust
/// A single-channel dasp Frame compatible with rill's ScalarVector1.
pub struct FrameOf<T: Transcendental>(pub ScalarVector1<T>);

impl<T: Transcendental> dasp::Frame for FrameOf<T> { ... }
```

### `From`/`Into` impls

```rust
// Scalar → dasp Sample
impl From<f32> for dasp_sample::types::F32 { ... }
impl From<dasp_sample::types::F32> for f32 { ... }

// Stereo pair: two rill ports ↔ one dasp stereo frame
impl<T: Transcendental> From<(T, T)> for FrameOf<T> { ... }
impl<T: Transcendental> From<FrameOf<T>> for (T, T) { ... }
```

## RT-safety analysis

| Component | RT concern | Resolution |
|---|---|---|
| `DaspSource` | `S::next()` may allocate | Document that `S` must be pre-constructed. Typical dasp combinators (`.map()`, `.scale_amp()`) are allocation-free. Forbidden: `.buffered()`, `.fork()` with `Vec`-backed ring buffers. |
| `DaspSink` | Internal ring buffer | Uses `rill_core::buffer::RingBuffer` — RT-safe, stack-allocated (const generic `N`). |
| `FrameOf<T>` | `unsafe` in dasp `Frame` impls | dasp's `Frame` for `[S; N]` uses `unsafe` internally (compile-time bounds elimination). `FrameOf<T>` wraps `ScalarVector1<T>` which has no dasp-level unsafe — we implement `Frame` methods manually without unsafe. |
| `From`/`Into` | Conversion cost | O(1) per value. No allocation. |

### dasp combinators — RT-safe vs forbidden

| Safe for DaspSource | Forbidden in DaspSource |
|---|---|
| `.map()` `.zip_map()` | `.buffered()` — Vec-backed |
| `.scale_amp()` `.add_amp()` | `.fork()` — ring buffer with alloc |
| `.clip_amp()` `.offset_amp()` | `.bus()` — dynamic dispatch |
| `.delay()` `.take()` | `.into_interleaved_samples()` — internal allocation |
| `.inspect()` `.mul_hz()` | `Box<dyn Signal>` — vtable in hot path |
| `Converter` with `Linear`/`Floor` | `Converter` with `Sinc` — allocates ring buffer internally |

This whitelist is documented as a table in the `DaspSource` doc-comment.
Violations are caught by code review, not compile-time (Rust cannot enforce
"no allocation" at the trait level).

## Feature flag design

```toml
# rill-core/Cargo.toml
[features]
dasp-bridge = ["dep:dasp_frame", "dep:dasp_sample", "dep:dasp_signal"]

[dependencies]
dasp_frame = { version = "0.11", optional = true }
dasp_sample = { version = "0.11", optional = true }
dasp_signal = { version = "0.11", optional = true }
```

Bridge code is gated:
```rust
#[cfg(feature = "dasp-bridge")]
mod dasp_bridge;

#[cfg(feature = "dasp-bridge")]
pub use dasp_bridge::*;
```

## Performance budget

| Metric | Target | Measured via |
|---|---|---|
| Per-block conversion overhead | < 5% of block processing time | Criterion bench: native `Algorithm::process()` vs `DaspSource::process()` with identical signal |
| Latency (DaspSink) | ≤ 1 block | Ring buffer read-after-write delay |
| Memory (DaspSource) | sizeof(S) + 4 bytes | Stack size of the wrapper |
| No regression in non-bridge code | 0% | Workspace tests with and without feature flag |

Benchmarks to add (in `rill-core/benches/`):
1. `bench_dasp_source_vs_native` — identically configured sine generator, compare native `Algorithm` vs `DaspSource` wrapping dasp `signal::rate(..).sine()`
2. `bench_dasp_sink_rtt` — measure ring buffer write/read throughput

## Implementation plan

### Phase 1: Types and conversions (1 module, ~150 LOC)

1. Create `rill-core/src/dasp_bridge/types.rs`
   - `FrameOf<T>` struct + `dasp::Frame` impl (safe, no unsafe)
   - `From<(T,)> for FrameOf<T>` and reverse
   - `From<f32> for dasp_sample::types::F32` bridge impls

2. Create `rill-core/src/dasp_bridge/source.rs`
   - `DaspSource<T, S>` struct
   - `impl Algorithm<T> for DaspSource<T, S>`
   - RT-safety doc table

3. Create `rill-core/src/dasp_bridge/sink.rs`
   - `DaspSink<T, const N>` struct
   - `impl Algorithm<T> for DaspSink<T, N>`
   - `impl dasp::Signal for DaspSink<T, N>`

4. Wire up in `rill-core/src/lib.rs` behind `#[cfg(feature = "dasp-bridge")]`

### Phase 2: Feature flag and deps (~30 LOC)

1. Add optional deps to `rill-core/Cargo.toml`
2. Add `dasp-bridge` feature flag
3. Gate module behind feature flag

### Phase 3: Tests (~200 LOC)

1. Unit test: `FrameOf<f32>` ↔ `f32` roundtrip
2. Unit test: `DaspSource` with static sine signal → output comparison
3. Unit test: `DaspSink` write-read cycle
4. Unit test: empty signal produces zero output
5. Doc-test: complete example showing dasp signal → rill filter → dasp output

### Phase 4: Documentation (~100 LOC)

1. Module-level doc: "dasp Bridge" with architecture diagram (ASCII)
2. RT-safety section listing safe/forbidden combinators
3. Usage example: `hound` WAV load → `DaspSource` → rill filter → `DaspSink` → `cpal` output

### Phase 5: Benchmarks (deferred until SIMD activated)

Benchmarks are deferred because the per-sample `next()` overhead is meaningless
to measure until rill's SIMD path is activated — at that point the bridge becomes
the bottleneck worth profiling.

## Open questions

1. **Stereo support?** Current design is mono-only with paired nodes. Alternative:
   `DaspSource2<T, S>` for stereo interleaved frames. Decision deferred until
   real-world usage patterns emerge.

2. **Sample type mapping?** dasp supports `i8`–`i64`, `u8`–`u64`, `I24`, `U48` etc.
   rill only supports `f32`/`f64` via `Transcendental`. Integer sample types from
   dasp require runtime conversion to float in the bridge — not zero-cost.
   Acceptable for I/O boundaries, but documented limitation.

3. **Dynamic vs static Signal?** `dasp::Signal` is trait-based — the concrete type
   is known at compile time for combinator chains. Boxing (`Box<dyn Signal>`) enables
   dynamic graphs but hits vtable overhead in the audio path. Design choice:
   `DaspSource` is generic over `S` (static dispatch, preferred) — boxing left
   to the user if needed.

4. **`rill-adrift` umbrella re-export?** The bridge types should be re-exported
   through `rill-adrift` behind a passthrough `dasp-bridge` feature flag,
   matching the pattern used for `io`, `lofi`, and `sampler` feature flags.

## References

- dasp docs: <https://docs.rs/dasp/0.11.0/dasp/>
- dasp_signal Converter: <https://docs.rs/dasp_signal/0.11.0/dasp_signal/interpolate/struct.Converter.html>
- rill RT-safety: `rill/AGENTS.md` § "Real-time safety"
- rill architecture: `rill/docs/architecture.md`
- rill two-thread model: `rill/docs/plans/two_thread_architecture.md`
