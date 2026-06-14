# Backend Extraction from Graph Nodes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract I/O backends from graph nodes (Input/Output) and make graph processing message-driven via ClockTick with DMA buffer references.

**Architecture:** Backend owns DMA + IoRingBuffer. Graph receives ClockTick with BufferView. Only Source/Sink nodes access BufferView. Graph remains single-threaded on I/O callback. Rack actor orchestrates. Phases: additive → backends → graph → orchestrator → cleanup.

**Tech Stack:** Rust (no new crates). Existing: rill-core, rill-io, rill-graph, rill-adrift, rill-patchbay, rill-core-actor.

---

### Task 1: Create BufferView trait in rill-core

**Files:**
- Create: `rill-core/src/traits/buffer_view.rs`
- Modify: `rill-core/src/traits/mod.rs`

- [ ] **Step 1: Create BufferView trait**

```rust
// rill-core/src/traits/buffer_view.rs

/// BufferView — backend-specific accessor for I/O ring buffers.
///
/// Encapsulates per-backend rules (interleave/deinterleave) for reading
/// input samples from and writing output samples to cross-thread ring buffers.
/// Each backend provides its own implementation.
pub trait BufferView: Send + Sync {
    /// Number of input (capture) channels.
    fn num_input_channels(&self) -> usize;

    /// Number of output (playback) channels.
    fn num_output_channels(&self) -> usize;

    /// Read available input samples for one channel into `dst`.
    ///
    /// Returns the number of samples actually read (may be less than `dst.len()`
    /// if insufficient data is available).
    fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize;

    /// Write output samples for one channel from `src`.
    ///
    /// Returns the number of samples actually written (may be less than `src.len()`
    /// if insufficient space is available).
    fn write_output(&self, channel: usize, src: &[f32]) -> usize;
}
```

- [ ] **Step 2: Register in traits module**

Add to `rill-core/src/traits/mod.rs`:
- Add `pub mod buffer_view;` in the submodules section (after `pub mod port;`)
- Add `pub use buffer_view::*;` in the re-exports section
- Add `BufferView` to the `prelude` module

- [ ] **Step 3: Compile rill-core**

Run: `cargo check -p rill-core`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add rill-core/src/traits/buffer_view.rs rill-core/src/traits/mod.rs
git commit -m 'feat(rill-core): add BufferView trait for backend-specific ring buffer access'
```

---

### Task 2: Update ClockTick — add source and view fields

**Files:**
- Modify: `rill-core/src/time/tick.rs`

- [ ] **Step 1: Add fields and update struct**

Change `ClockTick` in `rill-core/src/time/tick.rs`:

```rust
// Remove Copy from derive, add source and view fields
#[derive(Debug, Clone, PartialEq)]
pub struct ClockTick {
    pub sample_pos: u64,
    pub samples_since_last: u32,
    pub is_new_block: bool,
    pub sample_rate: f32,
    pub tempo: Option<f32>,
    /// Which backend produced this tick (e.g. "alsa:default", "pipewire:0").
    pub source: String,
    /// Backend-specific buffer accessor for reading input / writing output.
    #[doc(hidden)]
    pub view: std::sync::Arc<dyn crate::traits::buffer_view::BufferView>,
}
```

Update `new()` constructor to accept `source` and `view`:

```rust
pub fn new(
    sample_pos: u64,
    samples_since_last: u32,
    sample_rate: f32,
    source: String,
    view: std::sync::Arc<dyn crate::traits::buffer_view::BufferView>,
) -> Self {
    Self {
        sample_pos,
        samples_since_last,
        is_new_block: true,
        sample_rate,
        tempo: None,
        source,
        view,
    }
}
```

Update `with_tempo()` similarly:

```rust
pub fn with_tempo(
    sample_pos: u64,
    samples_since_last: u32,
    sample_rate: f32,
    tempo: f32,
    source: String,
    view: std::sync::Arc<dyn crate::traits::buffer_view::BufferView>,
) -> Self {
    Self {
        sample_pos,
        samples_since_last,
        is_new_block: true,
        sample_rate,
        tempo: Some(tempo),
        source,
        view,
    }
}
```

Update `Default` impl — use a no-op view. Since `Default` can't create an `Arc<dyn BufferView>`, add a `pub fn new_stub()` or similar. But `Default` is used in tests and graph constructor. Solution: add a `NullBufferView` to `rill-core` as a default stub.

Also update `Display` impl to include `source`:

```rust
impl fmt::Display for ClockTick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ClockTick(pos={}, delta={}ms, rate={}Hz, source={}",
            self.sample_pos,
            self.delta_seconds() * 1000.0,
            self.sample_rate,
            self.source,
        )?;
        if let Some(tempo) = self.tempo {
            write!(f, ", tempo={}BPM", tempo)?;
        }
        write!(f, ")")
    }
}
```

- [ ] **Step 2: Add NullBufferView to rill-core**

Add to `rill-core/src/traits/buffer_view.rs` (or a new file `rill-core/src/traits/null_view.rs`):

```rust
/// No-op BufferView for testing and default initialization.
pub struct NullBufferView {
    num_input_channels: usize,
    num_output_channels: usize,
}

impl NullBufferView {
    pub fn new(num_input_channels: usize, num_output_channels: usize) -> Self {
        Self { num_input_channels, num_output_channels }
    }
}

impl BufferView for NullBufferView {
    fn num_input_channels(&self) -> usize { self.num_input_channels }
    fn num_output_channels(&self) -> usize { self.num_output_channels }
    fn read_input(&self, _channel: usize, dst: &mut [f32]) -> usize {
        let n = dst.len();
        dst.fill(0.0);
        n
    }
    fn write_output(&self, _channel: usize, _src: &[f32]) -> usize {
        _src.len()
    }
}
```

Update `Default` for `ClockTick` to use `NullBufferView`:

```rust
impl Default for ClockTick {
    fn default() -> Self {
        Self {
            sample_pos: 0,
            samples_since_last: 0,
            is_new_block: false,
            sample_rate: 44100.0,
            tempo: None,
            source: String::new(),
            view: std::sync::Arc::new(NullBufferView::new(2, 2)),
        }
    }
}
```

- [ ] **Step 3: Update all tests in tick.rs to include new fields**

Update each test call to `ClockTick::new()` and `ClockTick::with_tempo()` and `ClockTick::default()` to include source and view. Example:

```rust
fn null_view() -> std::sync::Arc<dyn BufferView> {
    std::sync::Arc::new(NullBufferView::new(2, 2))
}

let tick = ClockTick::new(44100, 44100, 44100.0, "test".into(), null_view());
```

- [ ] **Step 4: Fix all compilation errors across workspace**

Run: `cargo check --workspace 2>&1 | head -100`

Update all call sites that construct `ClockTick`:
- `rill-graph/src/graph.rs` — the tick closure in `run()`
- `rill-patchbay/src/engine.rs` — any test or default construction
- Any other crate using `ClockTick::new()` or `ClockTick::with_tempo()` or `ClockTick::default()`

- [ ] **Step 5: Compile full workspace**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add rill-core/src/time/tick.rs rill-core/src/traits/buffer_view.rs rill-core/src/traits/mod.rs
git add -u  # staged modifications in other crates
git commit -m 'feat(rill-core): add source and view fields to ClockTick with NullBufferView stub'
```

---

### Task 3: Update IoBackend trait — remove read/write, update callback signature

**Files:**
- Modify: `rill-core/src/io.rs`

- [ ] **Step 1: Update IoBackend trait**

Replace the whole trait definition:

```rust
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use crate::traits::buffer_view::BufferView;

/// Generic real-time signal I/O backend.
///
/// Lifecycle:
/// 1. `create_view()` — obtain the backend's BufferView for graph nodes
/// 2. `set_process_callback(cb)` — register graph processing callback (FnMut(&ClockTick))
/// 3. `run(running)` — enter I/O loop, blocks for poll-driven backends
/// 4. `stop()` — signal shutdown
pub trait IoBackend: Send {
    /// Create a BufferView for this backend.
    fn create_view(&self) -> Arc<dyn BufferView>;

    /// Register the process callback. The callback receives a ClockTick
    /// with source name and BufferView reference. Called once per block.
    fn set_process_callback(&self, cb: Box<dyn FnMut(&crate::time::ClockTick)>);

    /// Enter the I/O lifecycle. Blocks for poll-driven backends,
    /// returns immediately for callback-driven ones.
    fn run(&self, running: Arc<AtomicBool>) -> IoResult<()>;

    /// Signal the backend to shut down.
    fn stop(&self) -> IoResult<()>;

    /// Returns a control interface if supported.
    fn as_control(&self) -> Option<&dyn IoControl> { None }
}
```

Key changes:
- Remove generic parameter `<T: Scalar>` — backends are always f32 for signal I/O
- Remove `read()` and `write()` methods
- `set_process_callback` now takes `FnMut(&ClockTick)` instead of `Fn(f32)`
- Add `create_view()` method
- Remove `Scalar` import, add `BufferView` import

- [ ] **Step 2: Update tests in io.rs**

Update `TestBackend` and `NoControl` test implementations to match new trait:

```rust
struct TestBackend { reg: AtomicU8 }
impl IoBackend for TestBackend {
    fn create_view(&self) -> Arc<dyn BufferView> {
        Arc::new(NullBufferView::new(0, 0))
    }
    fn set_process_callback(&self, _cb: Box<dyn FnMut(&ClockTick)>) {}
    fn run(&self, _: Arc<AtomicBool>) -> IoResult<()> { Ok(()) }
    fn stop(&self) -> IoResult<()> { Ok(()) }
    fn as_control(&self) -> Option<&dyn IoControl> { Some(self) }
}
```

- [ ] **Step 3: Compile rill-core**

Run: `cargo check -p rill-core`
Expected: PASS (tests no longer access read/write)

- [ ] **Step 4: Commit**

```bash
git add rill-core/src/io.rs
git commit -m 'feat(rill-core): rework IoBackend trait — remove read/write, add create_view, update callback signature'
```

---

### Task 4: Update Processable trait — add tick parameter

**Files:**
- Modify: `rill-core/src/traits/processable.rs`

- [ ] **Step 1: Add tick parameter to Processable::process_block**

Change the trait signature and all blanket impls:

```rust
use crate::time::ClockTick;

pub trait Processable<T: Transcendental, const BUF_SIZE: usize> {
    fn process_block(&mut self, ctx: &RenderContext, tick: &ClockTick) -> ProcessResult<()>;
}
```

Update all blanket impls (Source, Processor, Router, Sink) — add `_tick` parameter. Processor and Router ignore it:

```rust
impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Source<T, BUF_SIZE>>
{
    fn process_block(&mut self, ctx: &RenderContext, tick: &ClockTick) -> ProcessResult<()> {
        const { assert!(BUF_SIZE.is_multiple_of(4), "BUF_SIZE must be a multiple of 4 for SIMD") }
        self.as_mut().generate(ctx, &[], &[], tick)
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Processor<T, BUF_SIZE>>
{
    fn process_block(&mut self, ctx: &RenderContext, _tick: &ClockTick) -> ProcessResult<()> {
        self.as_mut().process(ctx, &[], &[], &[], &[])
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Sink<T, BUF_SIZE>>
{
    fn process_block(&mut self, ctx: &RenderContext, tick: &ClockTick) -> ProcessResult<()> {
        self.as_mut().consume(ctx, &[], &[], &[], tick)
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Router<T, BUF_SIZE>>
{
    fn process_block(&mut self, ctx: &RenderContext, _tick: &ClockTick) -> ProcessResult<()> {
        (**self).route(ctx, &[])
    }
}
```

Update NodeVariant impl:

```rust
impl<T: Transcendental, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for NodeVariant<T, BUF_SIZE>
{
    fn process_block(&mut self, ctx: &RenderContext, tick: &ClockTick) -> ProcessResult<()> {
        match self {
            NodeVariant::Source(src) => src.process_block(ctx, tick),
            NodeVariant::Processor(proc) => proc.process_block(ctx, tick),
            NodeVariant::Router(rt) => rt.process_block(ctx, tick),
            NodeVariant::Sink(sink) => sink.process_block(ctx, tick),
        }
    }
}
```

- [ ] **Step 2: Update Source and Sink traits in node.rs**

Update `rill-core/src/traits/node.rs` — add `tick` parameter:

```rust
pub trait Source<T, const BUF_SIZE: usize>: Node<T, BUF_SIZE> {
    fn generate(
        &mut self,
        ctx: &crate::time::RenderContext,
        control_inputs: &[T],
        clock_inputs: &[crate::time::RenderContext],
        tick: &crate::time::ClockTick,
    ) -> ProcessResult<()>;
}

pub trait Sink<T, const BUF_SIZE: usize>: Node<T, BUF_SIZE> {
    fn consume(
        &mut self,
        ctx: &crate::time::RenderContext,
        signal_inputs: &[T],
        control_inputs: &[T],
        clock_inputs: &[crate::time::RenderContext],
        tick: &crate::time::ClockTick,
    ) -> ProcessResult<()>;
}
```

- [ ] **Step 3: Compile rill-core**

Run: `cargo check -p rill-core`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add rill-core/src/traits/processable.rs rill-core/src/traits/node.rs
git commit -m 'feat(rill-core): add tick parameter to Processable::process_block, Source::generate, Sink::consume'
```

---

### Task 5: Fix all Source/Sink implementations across workspace

**Files:** Many — every crate with nodes that implement Source or Sink.

- [ ] **Step 1: Find all implementations**

Run: `cargo check --workspace 2>&1 | grep "error\[E0053\]" | head -50`

Expected: compilation errors for every Source::generate and Sink::consume that doesn't match new signature.

- [ ] **Step 2: Fix each implementation**

Add `tick: &rill_core::time::ClockTick` parameter (as last param) to every `generate()` and `consume()` method. For non-I/O nodes, prefix with `_tick` or `_` to suppress unused warnings.

Key files to modify (update list based on step 1 output):
- `rill-io/src/input.rs` — `Input::generate()`
- `rill-io/src/output.rs` — `Output::consume()`
- `rill-lofi/src/` — Lofi nodes
- `rill-oscillators/src/` — Oscillator nodes
- `rill-digital-filters/src/` — Filter nodes
- `rill-digital-effects/src/` — Effect nodes
- `rill-router/src/` — EQ/Mixer nodes
- `rill-sampler/src/` — Sampler nodes
- `rill-core-dsp/src/` — DSP nodes
- `rill-core-model/src/` — WDF nodes
- `rill-analog-filters/src/` — Analog filter nodes
- `rill-analog-effects/src/` — Analog effect nodes

- [ ] **Step 3: Compile full workspace**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add -u
git commit -m 'feat: add tick parameter to all Source::generate and Sink::consume implementations'
```

---

### Task 6: Create BackendBufferView impls in rill-io

**Files:**
- Create: `rill-io/src/buffer_view.rs`
- Modify: `rill-io/src/lib.rs`
- Modify: `rill-io/src/backends/null.rs`

- [ ] **Step 1: Create BaseDeinterleavedView (shared impl for interleaved backends)**

```rust
// rill-io/src/buffer_view.rs

use std::sync::Arc;
use rill_core::traits::buffer_view::BufferView;

/// BufferView for interleaved backends (PipeWire, PortAudio, JACK, ALSA).
/// Deinterleaves on read, interleaves on write.
pub struct DeinterleavedView {
    input_ring: Arc<rill_core::buffer::RingBuffer>,  // TODO: use IoRingBuffer
    output_ring: Arc<rill_core::buffer::RingBuffer>, // TODO: use IoRingBuffer
    num_input_channels: usize,
    num_output_channels: usize,
    block_size: usize,
}

impl DeinterleavedView {
    pub fn new(
        input_ring: Arc<rill_core::buffer::RingBuffer>,
        output_ring: Arc<rill_core::buffer::RingBuffer>,
        num_input_channels: usize,
        num_output_channels: usize,
        block_size: usize,
    ) -> Self {
        Self { input_ring, output_ring, num_input_channels, num_output_channels, block_size }
    }
}

impl BufferView for DeinterleavedView {
    fn num_input_channels(&self) -> usize { self.num_input_channels }
    fn num_output_channels(&self) -> usize { self.num_output_channels }

    fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize {
        // Read interleaved data from input_ring, deinterleave into dst
        let n = dst.len().min(self.block_size);
        // For interleaved: sample[channel + frame * num_channels]
        let stride = self.num_input_channels;
        let mut buf = vec![0.0f32; n * stride];
        let read = self.input_ring.read(&mut buf);
        for frame in 0..(read / stride).min(n) {
            dst[frame] = buf[frame * stride + channel];
        }
        (read / stride).min(n)
    }

    fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        // Interleave src into output_ring
        let n = src.len().min(self.block_size);
        let stride = self.num_output_channels;
        let mut buf = vec![0.0f32; n * stride];
        for frame in 0..n {
            buf[frame * stride + channel] = src[frame];
        }
        self.output_ring.write(&buf)
    }
}
```

Note: The above uses `rill_core::buffer::RingBuffer` for now. If `IoRingBuffer` from `rill-io/src/buffer.rs` is the target, use that instead since it's lock-free SPSC. The actual ring buffer to use should be `IoRingBuffer` from `rill-io`.

Actually, re-examining: `IoRingBuffer` is defined in `rill-io/src/buffer.rs` and is lock-free SPSC. `rill_core::buffer::RingBuffer` is single-threaded. We should use `IoRingBuffer` for cross-thread communication. But `BufferView` is a `rill-core` trait, so `DeinterleavedView` in `rill-io` wraps `IoRingBuffer` which is also in `rill-io`. No cross-crate issue.

Let's use `IoRingBuffer`:

```rust
use crate::buffer::IoRingBuffer;

pub struct DeinterleavedView {
    input_ring: Arc<IoRingBuffer>,
    output_ring: Arc<IoRingBuffer>,
    num_input_channels: usize,
    num_output_channels: usize,
    block_size: usize,
}
```

- [ ] **Step 2: Update NullBackend to implement new IoBackend trait**

In `rill-io/src/backends/null.rs`, update `NullBackend`:

```rust
use rill_core::traits::buffer_view::BufferView;
use crate::buffer::IoRingBuffer;
use crate::buffer_view::DeinterleavedView;
use std::sync::Mutex;

pub struct NullBackend {
    input_ring: Arc<IoRingBuffer>,
    output_ring: Arc<IoRingBuffer>,
    view: Arc<dyn BufferView>,
    callback: Mutex<Option<Box<dyn FnMut(&rill_core::time::ClockTick)>>>,
    sample_rate: f32,
    block_size: u32,
    num_input_channels: usize,
    num_output_channels: usize,
}

impl NullBackend {
    pub fn new(
        sample_rate: f32,
        block_size: u32,
        num_input_channels: usize,
        num_output_channels: usize,
    ) -> Self {
        let input_ring = Arc::new(IoRingBuffer::new(block_size as usize * num_input_channels.max(1) * 4));
        let output_ring = Arc::new(IoRingBuffer::new(block_size as usize * num_output_channels.max(1) * 4));
        let view = Arc::new(DeinterleavedView::new(
            input_ring.clone(),
            output_ring.clone(),
            num_input_channels,
            num_output_channels,
            block_size as usize,
        ));
        Self {
            input_ring,
            output_ring,
            view,
            callback: Mutex::new(None),
            sample_rate,
            block_size,
            num_input_channels,
            num_output_channels,
        }
    }
}

impl IoBackend for NullBackend {
    fn create_view(&self) -> Arc<dyn BufferView> { self.view.clone() }
    fn set_process_callback(&self, cb: Box<dyn FnMut(&rill_core::time::ClockTick)>) {
        *self.callback.lock().unwrap() = Some(cb);
    }
    fn run(&self, running: Arc<AtomicBool>) -> IoResult<()> {
        // Fill input ring with zeros
        let mut zeros = vec![0.0f32; self.block_size as usize * self.num_input_channels];
        self.input_ring.write(&zeros);
        while running.load(Ordering::Relaxed) {
            let tick = rill_core::time::ClockTick::new(
                0, // sample_pos will be tracked by graph
                self.block_size,
                self.sample_rate,
                "null".to_string(),
                self.view.clone(),
            );
            if let Some(ref mut cb) = *self.callback.lock().unwrap() {
                cb(&tick);
            }
            // Drain output ring
            let mut drain = vec![0.0f32; self.block_size as usize * self.num_output_channels];
            self.output_ring.read(&mut drain);
        }
        Ok(())
    }
    fn stop(&self) -> IoResult<()> { Ok(()) }
}
```

- [ ] **Step 3: Update PipeWire, PortAudio, JACK, ALSA backends similarly**

Each backend:
1. Creates `IoRingBuffer` pair for input/output
2. Creates `DeinterleavedView` wrapping them
3. Implements `create_view()` returning the view
4. Implements `set_process_callback()` storing the callback
5. In `run()`, the I/O loop: writes DMA to input_ring, calls callback, reads output_ring to DMA

- [ ] **Step 4: Register module in rill-io**

Add `pub mod buffer_view;` to `rill-io/src/lib.rs`.

- [ ] **Step 5: Compile rill-io**

Run: `cargo check -p rill-io`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add rill-io/src/buffer_view.rs rill-io/src/lib.rs rill-io/src/backends/null.rs
git add rill-io/src/backends/  # other backends if updated
git commit -m 'feat(rill-io): add DeinterleavedView and update backends for new IoBackend trait'
```

---

### Task 7: Update Graph — process_block receives tick, remove ActiveNode dependency

**Files:**
- Modify: `rill-graph/src/graph.rs`
- Modify: `rill-graph/src/factory.rs` (Input/Output constructors)

- [ ] **Step 1: Update Graph::run() → Graph::process_block()**

Replace `run()` with a public `process_block()` method:

```rust
/// Process one block of signal data.
/// Called from the backend's process callback via the orchestrator.
#[allow(unsafe_code)]
pub fn process_block(&mut self, tick: &ClockTick) -> ProcessResult<()> {
    if let Some(ref mut actor) = self.actor {
        actor.drain();
    }
    let ctx = if let Some(ref clock) = self.system_clock {
        RenderContext::with_tempo(
            tick.sample_pos,
            tick.samples_since_last,
            tick.sample_rate,
            clock.bpm() as f32,
        )
    } else {
        RenderContext::new(tick.sample_pos, tick.samples_since_last, tick.sample_rate)
    };
    self.current_tick = tick.clone();
    unsafe {
        let nv = &mut *self.nodes.get();
        let _ = nv[self.source_idx].process_block(&ctx, tick);
        for po in 0..nv[self.source_idx].num_signal_outputs() {
            if let Some(port) = nv[self.source_idx].output_port(po) {
                let _ = port.propagate(port.buffer(), &ctx);
            }
        }
    }
    if let Some(ref parent) = self.parent_ref {
        parent.send(CommandEnum::ClockTick(tick.clone()));
    }
    Ok(())
}
```

Remove the old `run()` method entirely (lines 594-637).

- [ ] **Step 2: Update GraphBuilder — add buffer_view, remove backend_factory**

In `GraphBuilder`:
- Add field: `buffer_view: Option<Arc<dyn BufferView>>`
- Add method: `pub fn set_buffer_view(&mut self, view: Arc<dyn BufferView>)`
- Remove `backend_factory` field (or keep for backward compat, but don't use for IoNode resolution)
- Update `build()`: instead of creating backends via `backend_factory.create()`, pass `buffer_view` to Input/Output nodes

```rust
// In GraphBuilder
buffer_view: Option<Arc<dyn BufferView>>,

pub fn set_buffer_view(&mut self, view: Arc<dyn BufferView>) {
    self.buffer_view = Some(view);
}
```

Update `new()` to not require `BackendFactory` (or make it optional):

```rust
pub fn new(factory: Arc<NodeFactory<T, BUF_SIZE>>) -> Self {
    Self {
        // ... same fields ...
        backend_factory: Arc::new(BackendFactory::new()), // or remove
        buffer_view: None,
    }
}
```

In `build()`, update IoNode resolution (around line 320-360 in graph.rs):
- Instead of `backend_factory.create(name, &params)` and `io_node.resolve_backend(backend)`,
- Call `io_node.resolve_view(buffer_view.clone())` if available
- Or: IoNode no longer needs a method — the view is obtained via `tick.view` at runtime

Actually, with the design where `tick` carries the view, Input/Output nodes don't need to store the view at all. They receive it via `tick` parameter. So:
- Remove `resolve_backend()` call
- Remove `backend_factory` usage for IoNode resolution
- Input/Output constructors don't need view parameter

- [ ] **Step 3: Update Input to use tick.view**

In `rill-io/src/input.rs`:
- Remove `backend: Option<Box<dyn IoBackend<T>>>` field
- Remove `bufs: Vec<[T; BUF_SIZE]>` field
- Update `generate()`:

```rust
fn generate(
    &mut self,
    ctx: &RenderContext,
    control_inputs: &[T],
    clock_inputs: &[RenderContext],
    tick: &ClockTick,
) -> ProcessResult<()> {
    for (ch, port) in self.outputs.iter_mut().enumerate() {
        let buf = port.buffer_mut();
        tick.view.read_input(ch, buf.as_mut_slice());
    }
    Ok(())
}
```

- Remove `ActiveNode` impl for `Input`
- Remove `IoNode` impl for `Input` (or update `resolve_backend` to a no-op / remove)

- [ ] **Step 4: Update Output to use tick.view**

In `rill-io/src/output.rs`:
- Remove `backend: Option<Box<dyn IoBackend<T>>>` field
- Update `consume()`:

```rust
fn consume(
    &mut self,
    ctx: &RenderContext,
    signal_inputs: &[T],
    control_inputs: &[T],
    clock_inputs: &[RenderContext],
    tick: &ClockTick,
) -> ProcessResult<()> {
    for (ch, port) in self.inputs.iter().enumerate() {
        if port.data_received {
            let buf = port.signal_buffer();
            tick.view.write_output(ch, buf.as_slice());
        }
    }
    Ok(())
}
```

- Remove `ActiveNode` impl for `Output`
- Remove `IoNode` impl for `Output`

- [ ] **Step 5: Update NodeFactory constructors for Input/Output**

In `rill-io/src/lib.rs` or `rill-io/src/registration.rs`:
- Update Input constructor to not take backend parameter
- Update Output constructor to not take backend parameter

- [ ] **Step 6: Update rill-graph/src/backend_factory.rs**

Remove or deprecate (keep for backward compat during transition):

```rust
// BackendFactory can be simplified — no longer used for IoNode injection
// Keep for orchestrator-level backend creation
pub struct BackendFactory {
    factories: HashMap<String, Box<dyn Fn(&HashMap<String, ParamValue>) -> IoResult<Box<dyn IoBackend>>>>,
}
```

Wait — `BackendFactory` is still needed by the orchestrator to create backends. But it's no longer needed in `GraphBuilder`. So:
- Remove `BackendFactory` from `GraphBuilder` fields
- Keep `BackendFactory` type in `rill-graph` for the orchestrator
- Update its `create()` to return `Box<dyn IoBackend>` (new trait without generic)

- [ ] **Step 7: Compile rill-graph and rill-io**

Run: `cargo check -p rill-graph -p rill-io`
Expected: compilation errors to fix, then PASS

- [ ] **Step 8: Commit**

```bash
git add rill-graph/src/graph.rs rill-graph/src/graph_constructor.rs
git add rill-io/src/input.rs rill-io/src/output.rs rill-io/src/lib.rs
git commit -m 'feat: remove ActiveNode, update Input/Output to use tick.view, add Graph::process_block'
```

---

### Task 8: Update ModularSystem orchestrator

**Files:**
- Modify: `rill-adrift/src/modular/mod.rs`
- Modify: `rill-adrift/src/modular/case.rs`

- [ ] **Step 1: Update ModularSystem::launch()**

In `rill-adrift/src/modular/mod.rs`, update `launch()`:
- Create backend before graph: `let backend = backend_factory.create(name, &params)?;`
- Get view from backend: `let view = backend.create_view();`
- Pass view to graph builder: `builder.set_buffer_view(view);`
- After graph is built, register process callback on backend:

```rust
let mut graph = builder.build(&sys)?;
let rack_ref = rack_actor_ref.clone();

backend.set_process_callback(Box::new(move |tick: &ClockTick| {
    graph.process_block(tick);
    // ClockTick is sent to rack inside process_block, no need to duplicate
}));

// Spawn I/O thread
let running_clone = running.clone();
std::thread::spawn(move || {
    backend.run(running_clone);
});
```

- [ ] **Step 2: Update RackCase**

Remove `ActiveNode`-related logic. `RackCase::start()` no longer needs to call `ActiveNode::run()`. Instead, it spawns the I/O thread that calls `backend.run()` and the graph is processed inside the callback.

- [ ] **Step 3: Compile rill-adrift**

Run: `cargo check -p rill-adrift`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add rill-adrift/src/modular/mod.rs rill-adrift/src/modular/case.rs
git commit -m 'feat(rill-adrift): update orchestrator for new backend-outside-graph architecture'
```

---

### Task 9: Cleanup — remove deprecated types

**Files:**
- Modify: `rill-core/src/traits/node.rs` (remove ActiveNode, IoNode)
- Modify: `rill-graph/src/graph.rs` (remove PassiveRef, old imports)
- Modify: `rill-graph/src/graph_constructor.rs` (remove or simplify)

- [ ] **Step 1: Remove ActiveNode and IoNode traits**

In `rill-core/src/traits/node.rs`:
- Remove `ActiveNode` trait (lines 437-451)
- Remove `IoNode` trait (lines 420-423) — or keep IoNode as a marker with `resolve_view()` instead of `resolve_backend()`

Decision: Keep `IoNode` but change `resolve_backend` to `resolve_view` if needed. Since nodes get view from tick, IoNode may not be needed at all. Remove both.

- [ ] **Step 2: Remove PassiveRef**

In `rill-graph/src/graph.rs`, remove `PassiveRef` struct and its `IoBackend` impl (lines 119-140).

- [ ] **Step 3: Remove GraphConstructor**

In `rill-graph/src/graph_constructor.rs`, simplify or remove. The `GraphConstructor` currently wraps graph build + run in a thread. This is now done by the orchestrator directly.

- [ ] **Step 4: Remove old references**

Update `rill-core/src/traits/mod.rs` prelude to remove `ActiveNode` and `IoNode` exports.

- [ ] **Step 5: Compile full workspace**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add -u
git commit -m 'chore: remove ActiveNode, IoNode, PassiveRef, GraphConstructor — deprecated by backend extraction'
```

---

### Task 10: Run tests and fix any failures

**Files:** All modified

- [ ] **Step 1: Run full test suite**

```bash
cargo test --workspace 2>&1 | tail -50
```

- [ ] **Step 2: Fix failing tests**

Update any tests that:
- Construct `ClockTick` without new fields
- Create `Input`/`Output` with `backend` parameter
- Call `ActiveNode::run()` 
- Reference old `IoBackend` trait methods

- [ ] **Step 3: Run clippy**

```bash
cargo clippy --workspace 2>&1 | tail -30
```

- [ ] **Step 4: Fix clippy warnings**

- [ ] **Step 5: Final commit**

```bash
git add -u
git commit -m 'test: fix all tests and clippy warnings for backend extraction'
```

---

### Task 11: Update examples and docs

**Files:** Various examples/ and docs/

- [ ] **Step 1: Find affected examples**

```bash
grep -r "ActiveNode\|resolve_backend\|IoBackend.*read\|IoBackend.*write" rill-io/examples/ rill-adrift/examples/ rill-graph/examples/ 2>/dev/null
```

- [ ] **Step 2: Update examples to new API**

Update any example that uses old backend-in-node pattern.

- [ ] **Step 3: Commit**

```bash
git add examples/
git commit -m 'docs: update examples for new backend extraction architecture'
```
