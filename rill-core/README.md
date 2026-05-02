# rill-core

Domain-agnostic foundation library: lock-free queues, generic vector math,
atomic cells, and real-time safe primitives. Powers the Rill audio ecosystem,
but applicable to IoT, robotics, embedded systems, and signal processing.

## The trait hierarchy

- **`Scalar`** — base numeric trait: arithmetic, min/max/clamp, abs.
  Implemented for `f32`, `f64`, `i8`, `i16`, `i32`, `i64`.
- **`Transcendental`** — extends `Scalar` with `sin`, `cos`, `sqrt`, `exp`, `ln`.
  Implemented for `f32`, `f64`.

## Key components

- **traits** — `SignalNode`, `ParameterId`, `PortId`, `Clock`, `Source`/`Processor`/`Sink`
- **math** — `Scalar`, `Transcendental` traits; `lerp`, `db_to_linear`, `midi_to_freq`; **vector** submodule
- **vector** — `Vector<T: Scalar, N>` trait and implementations:
  `ScalarVector1/2/4/8<T>`, SIMD types (`F32x4`, `F64x4`, etc.), slice operations
- **buffer** — `PipeBuffer`, `FanOutBuffer`, `FanInBuffer`, `RingBuffer`, `DelayLine`, `AtomicCell`
- **queues** — lock-free `SpscQueue` and `RingQueue` (no_std, no external deps);
  `MpscQueue` (alloc); `CommandQueue` (std, crossbeam-channel)
- **time** — `ClockTick`, `SystemClock`, tempo and beat tracking
- **macros** — `processor_node!`, `source_node!`, `sink_node!`, `with_parameters!`
- **error** — typed error system

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
