# Two-Thread Architecture — Audio Engine + Control World

## Overview

Реализация двухпоточной модели поверх иммутабельного `AudioGraph`.
Звуковой поток (hard RT) и поток мира автоматов (soft RT) общаются
через неблокирующие очереди.

```
[Control Thread (soft RT)]             [Audio Thread (hard RT)]
─────────────────────────────           ─────────────────────────
  Automata (LFO, Env)                        AudioEngine
  Sensors (анализаторы)                        │
  Servos (приводы)                      Processing Loop:
       │                                 for idx in topo_order:
       │                                     pre_process
       │ CommandQueue.send()                 process_block
       ├─────────── неблокирующая ─────────►  snapshot_feedback
       │              очередь                 propagate
       │                                    │
       ◄─────────── неблокирующая ─────────┤
       │    TelemetryQueue.recv()           TelemetryQueue.send()
       │
  Sensor.update(value)
```

## Status

Queue infrastructure in `rill-core` is **partially implemented** (`SpscQueue`,
`RtQueue`, `CommandEnum`, `Telemetry`, `MicroControlObserver` exist).
Audio engine and integration layer are **not yet built**.

## Phase 1 — Complete Queue Infrastructure (`rill-core`)

### 1.1 Define `CommandQueue<T>` wrapper

A real-time-safe wrapper around `RtQueue<T>` with typed send/recv API.
Already partially present (type alias in `telemetry.rs`), but needs a proper
struct in `command.rs`.

```rust
// rill-core/src/queues/command.rs
pub struct CommandQueue<T: Command> {
    inner: Arc<RtQueue<T>>,
    name: String,
    stats: Arc<AtomicQueueStats>,
}
```

Methods:
- `CommandQueue::new(name, capacity)` — create with fixed capacity
- `send(&self, cmd: T) -> QueueResult<()>` — non-blocking push
- `try_recv(&self) -> Result<T, QueueError>` — non-blocking pop
- `receiver(&self) -> CommandReceiver<T>` — consumer half (for audio thread)
- `sender(&self) -> CommandSender<T>` — producer half (for control thread)

### 1.2 Fix `RtQueue` compilation issues

- `rt_queue.rs` imports `QueueStats` (doesn't exist) — change to `QueueStatsSnapshot`
- `SpscQueue::stats()` returns `QueueStatsSnapshot` directly, but `RtQueue::stats()` calls `.snapshot()` on it — fix to match
- Ensure `T: Copy + Send + 'static` bounds work with `CommandEnum` and `Telemetry`

### 1.3 Fix `MicroControlObserver` test import

- Test imports `crate::queue::TelemetryQueue` but module is `crate::queues` — fix path
- Observer should use `Arc<RtQueue<Telemetry>>` internally instead of `crossbeam_channel::Sender`

### 1.4 Consolidate error types

There are two `QueueError` definitions:
- `queues/mod.rs` — simple enum with `Empty`/`Full`/etc
- `queues/error.rs` — `thiserror`-based with different variants

Consolidate into one (prefer `thiserror` version for better integration).

### 1.5 Add `TelemetryQueue` as proper struct

Currently a type alias (`CommandQueue<Telemetry>`). Promote to struct with
convenience methods already defined in `TelemetryQueueExt` trait.

---

## Phase 2 — AudioEngine (`rill-graph`)

### 2.1 New module: `rill-graph/src/engine.rs`

```rust
pub struct AudioEngine<T: AudioNum, const BUF_SIZE: usize> {
    /// Immutable graph (built once)
    graph: AudioGraph<T, BUF_SIZE>,
    /// Nodes (mutable state, indexed by same indices as graph)
    nodes: Vec<NodeEntry<T, BUF_SIZE>>,
    /// Command queue receiver (control → audio)
    cmd_rx: Option<CommandReceiver<CommandEnum>>,
    /// Telemetry queue sender (audio → control)
    tel_tx: Option<TelemetrySender>,
    /// Micro-control observer
    observer: Option<MicroControlObserver>,
    /// Running flag
    running: Arc<AtomicBool>,
    /// Stats
    stats: Arc<AtomicGraphStats>,
}
```

### 2.2 Engine API

```rust
impl<T: AudioNum, const BUF_SIZE: usize> AudioEngine<T, BUF_SIZE> {
    /// Create engine from a built AudioGraph
    pub fn new(graph: AudioGraph<T, BUF_SIZE>) -> Self;

    /// Attach command queue (control → audio)
    pub fn attach_command_queue(&mut self, rx: CommandReceiver<CommandEnum>);

    /// Attach telemetry queue (audio → control)
    pub fn attach_telemetry(&mut self, tx: TelemetrySender);

    /// Attach micro-control observer
    pub fn attach_observer(&mut self, observer: MicroControlObserver);

    /// Process one block — called from audio thread
    pub fn process_block(&mut self) -> ProcessResult<()>;

    /// Run the processing loop in a dedicated audio thread
    pub fn start(&self) -> ProcessResult<()>;

    /// Signal the audio thread to stop
    pub fn stop(&self);
}
```

### 2.3 Processing Loop (`process_block`)

```rust
pub fn process_block(&mut self) -> ProcessResult<()> {
    let tick = ClockTick::new(...);  // advance clock

    // 1. Drain command queue
    if let Some(ref rx) = self.cmd_rx {
        while let Ok(cmd) = rx.try_recv() {
            self.apply_command(cmd);
        }
    }

    // 2. Process nodes in topological order
    for &idx in self.graph.topo_order() {
        let node = &mut self.nodes[idx];

        // 2a. pre_process — mix feedback
        for port_idx in 0..node.num_audio_inputs() {
            node.input_port_mut(port_idx).pre_process(&tick);
        }

        // 2b. Process DSP
        let node_variant = &mut node.node;
        node_variant.process_block(&tick)?;

        // 2c. snapshot_feedback
        for port_idx in 0..node.num_audio_outputs() {
            node.output_port_mut(port_idx).snapshot_feedback();
        }

        // 2d. propagate
        for port_idx in 0..node.num_audio_outputs() {
            node.output_port(port_idx).propagate(&tick, &mut self.nodes);
        }
    }

    // 3. Send telemetry
    if let Some(ref tx) = self.tel_tx {
        // Send per-node telemetry
        for &idx in self.graph.topo_order() {
            let node = &self.nodes[idx];
            // e.g., send peak values
        }
    }

    Ok(())
}
```

### 2.4 Command Application

```rust
fn apply_command(&mut self, cmd: CommandEnum) {
    match cmd {
        CommandEnum::SetParameter(sp) => {
            let node_idx = sp.port.node_id().inner();
            if let Some(node) = self.nodes.get_mut(node_idx) {
                let _ = node.node.set_parameter(&sp.parameter, ParamValue::Float(sp.value));
            }
        }
        _ => {}  // other commands handled by higher layers
    }
}
```

### 2.5 Thread Management (`start` / `stop`)

```rust
pub fn start(&self) -> ProcessResult<()> {
    self.running.store(true, Ordering::Relaxed);
    let running = self.running.clone();
    // Spawn high-priority audio thread (platform-specific)
    std::thread::Builder::new()
        .name("rill-audio".into())
        .spawn(move || {
            while running.load(Ordering::Relaxed) {
                self.process_block()?;
            }
            Ok(())
        })?;
    Ok(())
}

pub fn stop(&self) {
    self.running.store(false, Ordering::Relaxed);
}
```

### 2.6 Dependencies to activate in `rill-graph/Cargo.toml`

- `crossbeam-channel` — already declared, currently unused. Wire for telemetry/command.
- `parking_lot` — already declared, currently unused. Use for stats `RwLock`.

---

## Phase 3 — Control Thread & Automaton World (`rill-patchbay`)

### 3.1 Reactivate `rill-patchbay` crate

The patchbay was described as "мир автоматов" — it should own:

- **Automata** — LFO, envelope generators, sequencers
- **Sensors** — envelope followers, pitch detectors, peak analyzers
- **Servos** — bind automata/sensors to graph parameters

### 3.2 Automaton trait

```rust
pub trait Automaton: Send + 'static {
    fn tick(&mut self, tick: &ClockTick) -> Vec<CommandEnum>;
    fn name(&self) -> &str;
    fn reset(&mut self);
}
```

Examples: `LfoAutomaton`, `EnvelopeAutomaton`

### 3.3 Sensor trait

```rust
pub trait Sensor: Send + 'static {
    fn feed(&mut self, telemetry: &Telemetry);
    fn value(&self) -> f32;
    fn name(&self) -> &str;
}
```

Examples: `EnvelopeFollower`, `PitchDetector`

### 3.4 Servo

```rust
pub struct Servo {
    automaton: Box<dyn Automaton>,
    target: ParameterTarget,
    cmd_tx: CommandSender<CommandEnum>,
}
```

### 3.5 Control Thread Loop

```rust
pub struct ControlWorld {
    automata: Vec<Box<dyn Automaton>>,
    sensors: Vec<Box<dyn Sensor>>,
    servos: Vec<Servo>,
    cmd_tx: CommandSender<CommandEnum>,
    tel_rx: Receiver<Telemetry>,
}

impl ControlWorld {
    pub fn tick(&mut self, tick: &ClockTick) {
        // 1. Feed telemetry to sensors
        while let Ok(tel) = self.tel_rx.try_recv() {
            for sensor in &mut self.sensors {
                sensor.feed(&tel);
            }
        }

        // 2. Tick automata → collect commands → send to graph
        for servo in &mut self.servos {
            let cmds = servo.automaton.tick(tick);
            for cmd in cmds {
                self.cmd_tx.send(cmd);
            }
        }
    }
}
```

---

## Phase 4 — Integration & Wiring

### 4.1 Putting It Together

```rust
use rill_graph::prelude::*;
use rill_core::queues::*;
use rill_patchbay::prelude::*;  // future

const BUF_SIZE: usize = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build immutable graph
    let mut builder = GraphBuilder::<f32, BUF_SIZE>::new();
    let osc = builder.add_source(Box::new(SineOsc::new(440.0, 44100.0)));
    let filter = builder.add_processor(Box::new(LowPassFilter::new(1000.0, 44100.0)));
    let sink = builder.add_sink(Box::new(NullSink::new(44100.0)));
    builder.connect_audio(osc, 0, filter, 0);
    builder.connect_audio(filter, 0, sink, 0);
    let graph = builder.build(Box::new(SystemClock::with_sample_rate(44100.0)))?;

    // 2. Create queues
    let cmd_queue = CommandQueue::<CommandEnum>::new("audio-control", 1024);
    let tel_queue = TelemetryQueue::new("audio-telemetry", 256);

    // 3. Create engine
    let mut engine = AudioEngine::new(graph);
    engine.attach_command_queue(cmd_queue.receiver());
    engine.attach_telemetry(tel_queue.sender());
    engine.attach_observer(MicroControlObserver::new(tel_queue.clone()));

    // 4. Create control world
    let mut world = ControlWorld::new();
    world.add_automaton(LfoAutomaton::new(5.0).with_target(filter_port, "cutoff"));
    world.attach_command_queue(cmd_queue.sender());
    world.attach_telemetry(tel_queue.receiver());

    // 5. Start audio thread
    engine.start()?;

    // 6. Control loop (on main thread or dedicated)
    let mut tick = ClockTick::new(0, BUF_SIZE as u32, 44100.0);
    loop {
        world.tick(&tick);
        tick = tick.advance(BUF_SIZE as u32);
        std::thread::sleep(Duration::from_micros(
            (BUF_SIZE as f32 / 44100.0 * 1_000_000.0) as u64
        ));
    }
}
```

### 4.2 Thread Safety Summary

| Component | Thread | Requirements |
|-----------|--------|--------------|
| `AudioEngine::process_block` | Audio (hard RT) | No allocations, no blocking, no locks |
| `AudioEngine::start/stop` | Any | Atomic flags |
| `CommandQueue::send` | Control (soft RT) | Lock-free push |
| `CommandQueue::try_recv` | Audio (hard RT) | Lock-free pop |
| `TelemetryQueue::send` | Audio (hard RT) | Lock-free push |
| `TelemetryQueue::try_recv` | Control (soft RT) | Lock-free pop |
| `ControlWorld::tick` | Control (soft RT) | May block, may allocate |

---

## Phase 5 — Micro-Control & Observability

### 5.1 OperationGuard Integration

Wrap sensitive engine operations with `MicroControlObserver`:

```rust
pub fn process_block(&mut self) -> ProcessResult<()> {
    let _guard = self.observer
        .as_ref()
        .map(|o| o.observe_start("engine::process_block"));

    // ... processing loop ...

    Ok(())
}
```

### 5.2 Telemetry Emission Points

- Per-node peak values (after propagation)
- Parameter changes (when command is applied)
- Processing time violations (via `OperationGuard`)
- Block processing stats (avg/max time)

### 5.3 Stats Collection

```rust
pub struct GraphStats {
    pub blocks_processed: AtomicU64,
    pub max_process_time_ns: AtomicU64,
    pub avg_process_time_ns: AtomicF64,  // or use RwLock
}
```

Already declared in `graph.rs` as `GraphStats` — connect to `AudioEngine`.

---

## Summary

| Phase | What | Where | Depends On |
|-------|------|-------|------------|
| 1 | Complete queue infra | `rill-core::queues` | — |
| 2 | AudioEngine | `rill-graph::engine` | Phase 1 |
| 3 | Control world | `rill-patchbay` | Phase 1 |
| 4 | Integration wiring | app-level | Phases 1–3 |
| 5 | Observability | `rill-core` + `rill-graph` | Phase 2 |

## Current codebase state

Files already existing that support this plan:

| File | Purpose | Status |
|------|---------|--------|
| `rill-core/src/queues/spsc.rs` | Lock-free SPSC queue | Implemented, tested |
| `rill-core/src/queues/rt_queue.rs` | RT queue facade | Implemented, tested |
| `rill-core/src/queues/signal.rs` | CommandEnum, SetParameter, etc. | Implemented |
| `rill-core/src/queues/telemetry.rs` | Telemetry enum + queue | Implemented |
| `rill-core/src/queues/observer.rs` | MicroControlObserver | Implemented |
| `rill-core/src/queues/command.rs` | Command trait (minimal) | Stub |
| `rill-core/src/queues/error.rs` | QueueError (thiserror) | Implemented |
| `rill-core/src/queues/mpsc.rs` | MPSC queue | Partial |
| `rill-core/src/queues/ring.rs` | Ring queue | Partial |
| `rill-graph/src/graph.rs` | GraphStats struct | Declared, unused |
| `rill-graph/Cargo.toml` | crossbeam-channel, parking_lot | Declared, unused |
