# rill-core

Domain-agnostic foundation library: lock-free queues, generic vector math,
atomic cells, and real-time safe primitives. Powers the Rill ecosystem for
IoT, robotics, embedded systems, signal processing, and audio.

## The trait hierarchy

- **`Scalar`** — base numeric trait: arithmetic, min/max/clamp, abs.
  Implemented for `f32`, `f64`, `i8`, `i16`, `i32`, `i64`.
- **`Transcendental`** — extends `Scalar` with `sin`, `cos`, `sqrt`, `exp`, `ln`.
  Implemented for `f32`, `f64`.

## Key components

- **traits** — `Node`, `ParameterId`, `PortId`, `Clock`, `Source`/`Processor`/`Sink`, `Algorithm`, `ParameterWrite`, `MultichannelAlgorithm`, `BridgeAlgorithm`
- **math** — `Scalar`, `Transcendental` traits; `lerp`, `db_to_linear`, `seconds_to_samples`; **vector** submodule
- **vector** — `Vector<T: Scalar, N>` trait and implementations:
  `ScalarVector1/2/4/8<T>`, SIMD types (`F32x4`, `F64x4`, etc.), slice operations
- **buffer** — `PipeBuffer`, `FanOutBuffer`, `FanInBuffer`, `RingBuffer`, `DelayLine`, `AtomicCell`
- **queues** — lock-free `SpscQueue` and `RingQueue` (no_std, no external deps);
  `MpscQueue` (alloc); signal/command types (`SetParameter`, `CommandEnum`, etc.)
- **time** — `ClockTick`, `SystemClock`, `RenderContext`, tempo and beat tracking
- **macros** — `processor_node!`, `source_node!`, `sink_node!`, `with_parameters!`
- **io** — `IoDriver`, `IoCapture`, `IoPlayback` traits for backend abstraction
- **builtin** — `Registry<T>`, `BuiltinSig`, `BlockBuiltin<T>`, `SampleBuiltin<T>`
- **interpolate** — fractional-index interpolation trait
- **prelude** — convenience re-exports for common imports

## Domain-agnostic primitives

| Component | no_std | Alloc | Description |
|-----------|--------|-------|-------------|
| `Scalar` (i8/i16/i32/i64) | ✅ | — | Integer arithmetic, min/max/clamp |
| `Scalar` (f32/f64) | ✅ | — | Float arithmetic |
| `Transcendental` (f32/f64) | ✅ | — | Float + sin/cos/sqrt/exp/ln |
| `SpscQueue<T, CAP>` | ✅ | — | Lock-free SPSC ring buffer |
| `RingQueue<T, CAP>` | ✅ | — | Lock-free delay-line buffer |
| `MpscQueue<T>` | ✅ | ✅ | Lock-free MPSC (Michael-Scott) |
| `AtomicCell<T>` | ✅ | — | Atomic wrapper for Copy types |
| `Vector<T, N>` + `ScalarVectorN<T>` | ✅ | — | Generic vector math |

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-core>
