# rill-core architecture

The `rill-core` crate is the foundation of the Rill ecosystem — traits, math,
buffers, queues, time, and error types.

## Core traits

### `Node`

Base trait for all signal graph nodes. No `Send` or `Sync` bounds — nodes live on the
audio thread exclusively.

```rust
pub trait Node<T: Transcendental, const BUF_SIZE: usize> {
    fn metadata(&self) -> NodeMetadata;
    fn init(&mut self, sample_rate: f32);
    fn reset(&mut self);
    fn id(&self) -> NodeId;
    fn set_id(&mut self, id: NodeId);
    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue>;
    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()>;

    // Downcasting helpers (default no-op)
    fn as_io_node_mut(&mut self) -> Option<&mut dyn IoNode<T, BUF_SIZE>> { None }
    fn as_active_node_mut(&mut self) -> Option<&mut dyn ActiveNode<T, BUF_SIZE>> { None }
}
```

### `IoNode` and `ActiveNode`

Two extension traits form a hierarchy for I/O-capable nodes:

```rust
pub trait IoNode<T: Transcendental, const BUF_SIZE: usize>: Node<T, BUF_SIZE> {
    fn resolve_backend(&mut self, backend: Box<dyn IoBackend<T>>);
}

pub trait ActiveNode<T: Transcendental, const BUF_SIZE: usize>: IoNode<T, BUF_SIZE> {
    fn run(
        &mut self,
        tick: Box<dyn FnMut(u64, f32)>,
        running: Arc<AtomicBool>,
    ) -> IoResult<()>;
}
```

- **`IoNode`** — implemented by `Input`, `Output`, `LofiInput`. Receives a backend
  during `GraphBuilder::build()`. Only nodes implementing this trait get backends.
- **`ActiveNode`** — implemented by `Input` and `Output`. The single node in a graph
  that drives the audio callback loop. `Graph::run()` calls `ActiveNode::run()` with
  a tick closure that drains commands, processes the source, and propagates ports.

`GraphBuilder::build()` uses `as_io_node_mut()` / `as_active_node_mut()` to detect
which nodes implement these traits — no name-based matching required.

### `Source`, `Processor`, `Sink`

```rust
pub trait Source<T: Transcendental, const BUF_SIZE: usize>: Node<T, BUF_SIZE> {
    fn generate(&mut self, clock: &ClockTick, ctrl: &[T], clk: &[ClockTick]) -> ProcessResult<()>;
}

pub trait Processor<T: Transcendental, const BUF_SIZE: usize>: Node<T, BUF_SIZE> {
    fn process(&mut self, clock: &ClockTick, signal: &[&[T; BUF_SIZE]], ...) -> ProcessResult<()>;
}

pub trait Sink<T: Transcendental, const BUF_SIZE: usize>: Node<T, BUF_SIZE> {
    fn consume(&mut self, clock: &ClockTick, signal: &[&[T; BUF_SIZE]], ...) -> ProcessResult<()>;
}
```

### `IoBackend` and `IoControl`

Backends are owned by I/O nodes. No `Send + Sync` bounds — they live on
the audio thread exclusively.

```rust
pub trait IoBackend<T: Scalar> {
    fn set_process_callback(&self, cb: Box<dyn Fn(f32)>);
    fn read(&self, channels: &mut [&mut [T]]) -> usize;
    fn write(&self, channels: &[&[T]]) -> usize;
    fn run(&self, running: Arc<AtomicBool>) -> IoResult<()>;
    fn stop(&self) -> IoResult<()>;
    fn as_control(&self) -> Option<&dyn IoControl> { None }
}

pub trait IoControl {
    fn write_data(&self, data: &[u8]) -> usize;
}
```

`IoControl::write_data()` receives raw bytes — interpretation is
backend-specific (e.g. AY-3-8910 register writes, MIDI, proprietary
protocols).  Control actors send bytes via `ParamValue::Bytes` through
the standard command queue.

### `ParamValue`

```rust
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    String(String),
    Choice(String),
    Bytes(Vec<u8>),  // for IoControl::write_data()
}
```

## Queues

Non-blocking SPSC queue for dual-thread communication:

```rust
use rill_core::queues::{MpscQueue, SetParameter};

let cmd_queue = Arc::new(MpscQueue::<SetParameter>::with_capacity(64));

// Control thread
cmd_queue.push(SetParameter::new(port, param, value, SignalOrigin::Manual));

// Audio thread (in tick closure)
while let Some(cmd) = cmd_queue.pop() {
    nodes[cmd.target].set_parameter(&cmd.parameter, cmd.value);
}
```

## `ClockTick`

Sample-accurate timing sent from audio to control thread:

```rust
pub struct ClockTick {
    pub sample_pos: u64,
    pub samples_since_last: u32,
    pub is_new_block: bool,
    pub sample_rate: f32,
    pub tempo: Option<f32>,
}
```

## Module tree

```
rill-core/
├── traits/   — Node, Source, Processor, Sink, ParamValue, Port
├── math/     — Scalar, Transcendental, Vector
├── buffer/   — PipeBuffer, DelayLine, RingBuffer, FixedBuffer
├── queues/   — MpscQueue, SetParameter, Telemetry
├── time/     — ClockTick, SystemClock
├── io/       — IoBackend, IoControl, IoResult
└── macros/   — source_node!, processor_node!, sink_node!
```
