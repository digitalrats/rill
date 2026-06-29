# rill-core architecture

The `rill-core` crate is the foundation of the Rill ecosystem — traits, math,
buffers, queues, time, and error types.

## Core traits

### `Node`

Base trait for all signal graph nodes. No `Send` or `Sync` bounds — nodes live on the
signal thread exclusively.

```rust
pub trait Node<T: Transcendental, const BUF_SIZE: usize> {
    fn metadata(&self) -> NodeMetadata;
    fn init(&mut self, sample_rate: f32);
    fn reset(&mut self);
    fn id(&self) -> NodeId;
    fn set_id(&mut self, id: NodeId);
    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue>;
    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()>;
}
```

### ParameterWrite

`ParameterWrite` is the polymorphic control interface for DSP engines.
It decouples parameter dispatch from the concrete engine type:

```rust
pub trait ParameterWrite {
    fn write_parameter(&mut self, name: &str, value: ParamValue) -> ProcessResult<()>;
    fn read_parameter(&self, name: &str) -> Option<ParamValue> { None }
}
```

Implemented by `BasicOscillator<f32>`, `Ay38910Chip`, and any engine
that accepts named parameter writes.

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

### `IoDriver`, `IoCapture`, `IoPlayback`

Backends are created **externally** by the orchestrator. Three orthogonal
traits separate concerns:

```rust
pub trait IoDriver: Send + Sync {
    fn set_process_callback(&self, cb: Box<dyn FnMut(&ClockTick)>);
    fn run(&self, running: Arc<AtomicBool>) -> IoResult<()>;
    fn stop(&self) -> IoResult<()>;
}

pub trait IoCapture: Send + Sync {
    fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize;
    fn num_input_channels(&self) -> usize;
}

pub trait IoPlayback: Send + Sync {
    fn write_output(&self, channel: usize, src: &[f32]) -> usize;
    fn num_output_channels(&self) -> usize;
}
```

A single backend struct (e.g. `PipewireBackend`) implements `IoDriver`
and optionally `IoCapture` / `IoPlayback`.  The driver owns the timing loop;
capture and playback provide data access.

`IoBackend` exists as a backward-compatible alias: `pub trait IoBackend: IoDriver {}`.


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

Sample-accurate timing sent from driver to control thread.
Carries only timing metadata — I/O access is through `IoCapture`/`IoPlayback`
traits held by graph nodes.

```rust
pub struct ClockTick {
    pub sample_pos: u64,
    pub samples_since_last: u32,
    pub is_new_block: bool,
    pub sample_rate: f32,
    pub tempo: Option<f32>,
    pub source: String,
    pub speed_ratio: f64,
    pub is_final: bool,
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
├── io/       — IoDriver, IoCapture, IoPlayback, IoControl, IoResult
└── macros/   — source_node!, processor_node!, sink_node!
```
