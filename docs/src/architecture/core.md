# rill-core architecture

The `rill-core` crate is the foundation of the Rill ecosystem — traits, math,
buffers, queues, time, macros, and error types. It has **no audio-specific
dependencies** and can be used in embedded, IoT, robotics, and signal
processing contexts.

## Module tree

```
rill-core/
├── lib.rs                 # Root module, re-exports
├── prelude.rs             # Convenience prelude
├── config.rs              # Configuration
├── error.rs               # Error system
├── utils.rs               # Utilities
├── interpolate.rs         # Fractional-index interpolation
├── traits/
│   ├── mod.rs             # Node, Source, Processor, Sink
│   ├── node.rs            # Nodes and identifiers
│   ├── port.rs            # Ports
│   ├── param.rs           # Parameters
│   ├── processable.rs     # Processing interface
│   ├── error.rs           # Trait errors
│   ├── action.rs          # Node actions
│   └── algorithm.rs       # Algorithm trait
├── math/
│   ├── mod.rs             # Numeric type abstractions
│   ├── num.rs             # Scalar + Transcendental traits
│   ├── vector/            # Vector<T, N>, ScalarVector, SIMD
│   ├── conversions.rs     # Conversions
│   └── functions.rs       # Functions
├── buffer/
│   ├── mod.rs             # PipeBuffer, FanOutBuffer, etc.
│   ├── pipe.rs            # Point-to-point connections
│   ├── fan.rs             # Fan-out and fan-in
│   ├── delay.rs           # Delay line
│   ├── ring.rs            # Ring buffer
│   ├── storage.rs         # AtomicCell
│   ├── pool.rs            # Buffer pool
│   └── port_buffer.rs     # Port-owned buffer
├── queues/
│   ├── mod.rs             # Command and telemetry queues
│   ├── rt_queue.rs        # Real-time queue
│   ├── spsc.rs            # Single-producer single-consumer
│   ├── mpsc.rs            # Multi-producer single-consumer
│   ├── ring.rs            # Ring queue
│   ├── command.rs         # Commands
│   ├── telemetry.rs       # Telemetry
│   ├── signal.rs          # Signals
│   ├── observer.rs        # Observers
│   ├── atomic.rs          # Atomic operations
│   └── error.rs           # Queue errors
├── time/
│   ├── mod.rs             # Time and clock signals
│   ├── clock.rs           # Clock and ClockSource traits
│   ├── source.rs          # Clock implementations
│   ├── tick.rs            # ClockTick
│   └── error.rs           # Time errors
└── macros/
    ├── mod.rs             # Macros
    ├── source.rs          # source_node!
    ├── processor.rs       # processor_node!
    ├── sink.rs            # sink_node!
    ├── params.rs          # Parameters
    ├── ports.rs           # Ports
    └── tests.rs           # Macro tests
```

## Key components

### buffer

Real-time safe buffers for single-threaded signal processing:

- `PipeBuffer` — single-producer single-consumer
- `FanOutBuffer` — broadcast to multiple consumers
- `FanInBuffer` — mix multiple producers
- `DelayLine` — circular buffer with configurable delay
- `TapeLoop` — heap-allocated circular buffer for large delays
- `RingBuffer` — lock-free ring buffer for I/O

```rust
use rill_core::buffer::{PipeBuffer, FanOutBuffer, FanInBuffer, DelayLine};

let mut pipe = PipeBuffer::new(1024);
pipe.write(&[1.0, 2.0, 3.0]);
let read = pipe.read(3);
```

### macros

Convenience macros for creating nodes without boilerplate:

```rust
use rill_core::macros::{processor, sink, source};

processor!(Gain, |sample, _| sample * 0.5);
sink!(Logger, |sample, _| println!("{}", sample));
source!(Silence, || 0.0);
```

### math

Numeric trait hierarchy:

- **`Scalar`** — arithmetic, `min`/`max`/`clamp`/`abs`. Implemented for `f32`, `f64`, `i8`, `i16`, `i32`, `i64`.
- **`Transcendental`** — extends `Scalar` with `sin`, `cos`, `sqrt`, `exp`, `ln`, `PI`. Only `f32`, `f64`.
- **`Vector<T: Scalar, N>`** — SIMD-ready vector operations for any `Scalar`.

```rust
use rill_core::math::Scalar;

fn scale<T: Scalar>(v: ScalarVector4<T>) -> ScalarVector4<T> {
    v * T::from_f32(0.5)
}
```

### queues

Non-blocking queues for dual-thread communication (control ↔ audio):

```rust
use rill_core::queues::{CommandQueue, CommandEnum, SetParameter};

let mut queue = CommandQueue::new();
queue.send(CommandEnum::SetParameter(SetParameter {
    node_id: 1,
    param_id: "cutoff".to_string(),
    value: 1000.0,
}));
```

| Queue | Atomic | Alloc | Use case |
|-------|--------|-------|----------|
| `SpscQueue<T, CAP>` | yes | no | High-throughput SPSC |
| `MpscQueue<T>` | yes | yes | Multi-producer to audio thread |
| `RingQueue<T, CAP>` | yes | no | Lock-free delay line |

### time

Clock and timing abstractions:

```rust
use rill_core::time::{Clock, SystemClock};

let clock = SystemClock::new(44100.0);
let pos = clock.position_samples();
clock.advance(64);
```

### error

Typed error system with category codes:

```rust
use rill_core::{SignalError, SignalResult};

fn safe_process() -> SignalResult<()> {
    Ok(())
}
```

### prelude

Convenience re-export of common types:

```rust
use rill_core::prelude::*;
// Node, Scalar, Transcendental, PipeBuffer, CommandQueue, Clock, etc.
```
