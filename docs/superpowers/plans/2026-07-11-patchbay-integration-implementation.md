# Patchbay Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add lifecycle logging to ModularSystem, create PatchbayInspector for control-path state, extend AnalyzerCommand/Response with automaton/sensor/queue variants, and integrate everything into ModularSystem::launch().

**Architecture:** `rill-lang` engine shares `Arc<ProbeSlot>` between signal and collector threads. `PatchbayInspector` in `rill-patchbay` collects automaton/sensor snapshots. `ModuleFactory::construct()` registers inspectors during construction. `ModularSystem::launch()` wires CollectorThread with shmem + inspector + engine debug state.

**Tech Stack:** rill-core (SpscQueue, CommandEnum), rill-lang (RillGraphEngine, debug), rill-telemetry (CollectorThread, protocol), rill-patchbay (Servo, Sensor), rill-adrift (ModularSystem), log, parking_lot, dashmap, serde.

---

## File Map

| File | Action | Purpose |
|---|---|---|
| `rill-lang/src/graph_engine.rs` | Modify | `probe_slots: Vec<Arc<ProbeSlot>>`, `clone_debug_state()` |
| `rill-adrift/src/modular/mod.rs` | Modify | Lifecycle logging, debug wiring |
| `rill-adrift/Cargo.toml` | Modify | Ensure debug feature enables dependencies |
| `rill-telemetry/src/debug/protocol.rs` | Modify | New AnalyzerCommand/Response variants, QueueStats |
| `rill-patchbay/Cargo.toml` | Modify | `debug` feature, `serde` dep |
| `rill-patchbay/src/debug.rs` | Create | PatchbayInspector, AutomatonInspector, SensorInspector, snapshots |
| `rill-patchbay/src/engine.rs` | Modify | `Servo::inspector()` |
| `rill-patchbay/src/osc.rs` | Modify | `OscSensor::inspect()` |
| `rill-patchbay/src/midi.rs` | Modify | `MidiHub::inspect()` |
| `rill-patchbay/src/module_factory.rs` | Modify | Add `inspector` param to `construct()` |
| `rill-patchbay/src/servo_constructor.rs` | Modify | Pass inspector through Servo construction |
| `rill-patchbay/src/lib.rs` | Modify | Wire `debug` module |
| `rill-telemetry/src/debug/collector_thread.rs` | Modify | Handle new command variants |

---

## Phase 1: Arc<ProbeSlot> + Engine Changes

### Task 1: Arc<ProbeSlot> and clone_debug_state

**Files:**
- Modify: `rill-lang/src/graph_engine.rs`

- [ ] **Step 1: Change probe_slots to Vec<Arc<ProbeSlot>>**

Find the struct field (currently `probe_slots: Vec<ProbeSlot>`). Change to:

```rust
    #[cfg(feature = "debug")]
    pub(crate) probe_slots: Vec<std::sync::Arc<ProbeSlot>>,
```

- [ ] **Step 2: Update probe_slots initialization**

In both `new()` and `new_duplex()` constructors, change `probe_slots: Vec::new(),` — no change needed, Vec::new() works for both.

But `allocate_probe_slots` needs updating:

```rust
    #[cfg(feature = "debug")]
    pub fn allocate_probe_slots(&mut self, count: usize) {
        self.probe_slots = (0..count)
            .map(|_| std::sync::Arc::new(ProbeSlot::default()))
            .collect();
    }
```

- [ ] **Step 3: Add clone_debug_state method**

After `debug_state()`, add:

```rust
    #[cfg(feature = "debug")]
    pub fn clone_debug_state(&self) -> (
        Vec<std::sync::Arc<ProbeSlot>>,
        DebugControl,
        std::sync::Arc<SpscQueue<CommandFrame, 256>>,
    ) {
        (
            self.probe_slots.clone(),
            self.debug_control.clone(),
            self.command_queue.clone(),
        )
    }
```

- [ ] **Step 4: Update all probe capture code**

In `execute_sub_schedule` and `execute_step`, the probe capture code accesses `probe_slots[id as usize]`. With `Arc<ProbeSlot>`, the Deref trait handles this automatically — no code changes needed. But verify `cargo check` passes.

- [ ] **Step 5: Verify**

```bash
cargo check -p rill-lang --features debug
cargo check -p rill-lang
```

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/graph_engine.rs && git commit -m 'feat(rill-lang): use Arc<ProbeSlot> for shared probe access, add clone_debug_state'
```

---

## Phase 2: Logging + Protocol + PatchbayInspector

### Task 2: Lifecycle Logging in ModularSystem

**Files:**
- Modify: `rill-adrift/src/modular/mod.rs`

- [ ] **Step 1: Add lifecycle log calls**

In `launch()`, add:

```rust
pub fn launch(mut self, def: &ModularSystemDef) -> Result<Self, ModularError> {
    // ... after tokio_rt creation ...
    for rd in &def.racks {
        log::info!(
            "rill-adrift: launching rack '{}' — {} nodes, {} modules",
            rd.name, rd.graph.nodes.len(), rd.modules.len()
        );
        // ... existing code ...
        // After graph built:
        if let Some(case) = self.cases.get_mut(&rd.name) {
            case.start(move |running| {
                // ... existing engine build ...
                log::info!(
                    "rill-adrift: rack '{}' engine built — {} programs, {} steps",
                    rd.name, programs.len(), scheduled.steps.len()
                );
                // ... runner setup ...
                if let Some((ref name, ref params)) = backend_name {
                    match bf.create_any(name, params) {
                        Ok((driver, capture, playback)) => {
                            runner.wire_backends(capture, playback);
                            log::info!("rill-adrift: rack '{}' backend '{}' started", rd.name, name);
                            let _ = runner.run_with_driver(driver, running);
                        }
                        Err(e) => log::error!("rill-adrift: rack '{}' backend create '{}': {e}", rd.name, name),
                    }
                }
            });
        }
        // ... module construction ...
    }
    log::info!("rill-adrift: system launched with {} rack(s)", def.racks.len());
    // ...
}
```

In `stop()`:
```rust
pub fn stop(&mut self) {
    log::info!("rill-adrift: stopping system");
    for case in self.cases.values_mut() {
        log::info!("rill-adrift: stopping rack '{}'", case.name());
        case.stop();
    }
    #[cfg(feature = "serialization")]
    { self.tokio_rt = None; }
    log::info!("rill-adrift: system stopped");
}
```

- [ ] **Step 2: Verify**

```bash
cargo check -p rill-adrift
```

- [ ] **Step 3: Commit**

```bash
git add rill-adrift/src/modular/mod.rs && git commit -m 'feat(rill-adrift): add lifecycle logging to ModularSystem'
```

---

### Task 3: Protocol Extension — New Command/Response Variants

**Files:**
- Modify: `rill-telemetry/src/debug/protocol.rs`

- [ ] **Step 1: Add new AnalyzerCommand variants**

Add to the `AnalyzerCommand` enum (before `Quit`):

```rust
    /// List all registered automata.
    ListAutomata,
    /// Get automaton state snapshot.
    GetAutomatonState {
        /// Automaton name.
        name: String,
    },
    /// List all registered sensors.
    ListSensors,
    /// Get sensor status snapshot.
    GetSensorStatus {
        /// Sensor name.
        name: String,
    },
    /// List queue statistics.
    ListQueues,
```

- [ ] **Step 2: Add new AnalyzerResponse variants**

Add to the `AnalyzerResponse` enum (before `Paused`):

```rust
    /// List of automaton names.
    AutomataList(Vec<String>),
    /// Automaton state snapshot (serialized as JSON for transport).
    AutomatonState(String),
    /// List of sensor names.
    SensorList(Vec<String>),
    /// Sensor status snapshot (serialized as JSON for transport).
    SensorStatus(String),
    /// Queue statistics.
    QueueList(Vec<QueueStats>),
```

- [ ] **Step 3: Add QueueStats struct**

At the end of the file:

```rust
/// Statistics about a signal/command queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    /// Queue identifier.
    pub name: String,
    /// Maximum capacity.
    pub capacity: usize,
    /// Current number of elements.
    pub len: usize,
    /// Whether the queue is at capacity.
    pub is_full: bool,
}
```

- [ ] **Step 4: Verify**

```bash
cargo check -p rill-telemetry --features debug
```

- [ ] **Step 5: Commit**

```bash
git add rill-telemetry/src/debug/protocol.rs && git commit -m 'feat(rill-telemetry): add automaton/sensor/queue command variants'
```

---

### Task 4: PatchbayInspector Module

**Files:**
- Create: `rill-patchbay/src/debug.rs`
- Modify: `rill-patchbay/Cargo.toml`
- Modify: `rill-patchbay/src/lib.rs`

- [ ] **Step 1: Add debug feature and serde dep to Cargo.toml**

In `rill-patchbay/Cargo.toml` [features] section:
```toml
debug = ["serde"]
```

Check that `serde` is available. If not, add:
```toml
serde = { workspace = true, optional = true, features = ["derive"] }
```

- [ ] **Step 2: Create rill-patchbay/src/debug.rs**

```rust
//! Control-path inspection for the debugger.
//! Gated behind the `debug` feature.

use std::collections::HashMap;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// Snapshot of an automaton's current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomatonSnapshot {
    /// Automaton identifier (servo name).
    pub name: String,
    /// Whether the automaton is enabled.
    pub enabled: bool,
    /// Current output value.
    pub value: f64,
    /// Additional state fields (time, base, frozen, phase, etc.).
    pub extra: HashMap<String, f64>,
}

/// Snapshot of a sensor's current status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorSnapshot {
    /// Sensor identifier.
    pub name: String,
    /// Sensor type ("osc", "midi").
    pub kind: String,
    /// Whether the sensor is connected and polling.
    pub connected: bool,
    /// Total events received.
    pub event_count: u64,
    /// Last event description, if any.
    pub last_event: Option<String>,
    /// Whether a MIDI clock tracker is active.
    pub tracker_active: bool,
}

/// Trait for types that can produce an automaton state snapshot.
pub trait AutomatonInspector: Send + Sync {
    fn snapshot(&self) -> AutomatonSnapshot;
}

/// Trait for types that can produce a sensor state snapshot.
pub trait SensorInspector: Send + Sync {
    fn snapshot(&self) -> SensorSnapshot;
}

/// Collects control-path state for debugger inspection.
pub struct PatchbayInspector {
    automata: DashMap<String, Box<dyn AutomatonInspector>>,
    sensors: DashMap<String, Box<dyn SensorInspector>>,
}

impl PatchbayInspector {
    pub fn new() -> Self {
        Self {
            automata: DashMap::new(),
            sensors: DashMap::new(),
        }
    }

    pub fn register_automaton(&self, name: String, inspector: Box<dyn AutomatonInspector>) {
        self.automata.insert(name, inspector);
    }

    pub fn register_sensor(&self, name: String, inspector: Box<dyn SensorInspector>) {
        self.sensors.insert(name, inspector);
    }

    pub fn list_automata(&self) -> Vec<String> {
        self.automata.iter().map(|e| e.key().clone()).collect()
    }

    pub fn get_automaton_snapshot(&self, name: &str) -> Option<AutomatonSnapshot> {
        self.automata.get(name).map(|a| a.snapshot())
    }

    pub fn list_sensors(&self) -> Vec<String> {
        self.sensors.iter().map(|e| e.key().clone()).collect()
    }

    pub fn get_sensor_snapshot(&self, name: &str) -> Option<SensorSnapshot> {
        self.sensors.get(name).map(|s| s.snapshot())
    }
}
```

- [ ] **Step 3: Wire module in lib.rs**

Add to `rill-patchbay/src/lib.rs` (after existing modules):

```rust
#[cfg(feature = "debug")]
pub mod debug;
```

- [ ] **Step 4: Verify**

```bash
cargo check -p rill-patchbay --features debug
cargo check -p rill-patchbay
```

- [ ] **Step 5: Commit**

```bash
git add rill-patchbay/Cargo.toml rill-patchbay/src/debug.rs rill-patchbay/src/lib.rs
git commit -m 'feat(rill-patchbay): add PatchbayInspector for control-path inspection'
```

---

### Task 5: Servo + Sensor Inspection Methods

**Files:**
- Modify: `rill-patchbay/src/engine.rs`
- Modify: `rill-patchbay/src/osc.rs`
- Modify: `rill-patchbay/src/midi.rs`
- Modify: `rill-patchbay/src/debug.rs`

- [ ] **Step 1: Add Servo::inspector() in engine.rs**

After the existing `Servo<A>` impl block, add (behind feature gate):

```rust
#[cfg(feature = "debug")]
impl<A: crate::engine::Automaton + 'static> Servo<A> {
    pub fn inspector(&self) -> Box<dyn crate::debug::AutomatonInspector> {
        Box::new(crate::debug::ServoInspector {
            name: self.id.clone(),
            state: self.state.clone(),
        })
    }
}
```

- [ ] **Step 2: Add ServoInspector in debug.rs**

Add to `rill-patchbay/src/debug.rs`:

```rust
use std::sync::Arc;
use parking_lot::Mutex;

/// Inspects a Servo's internal state.
pub(crate) struct ServoInspector<A: crate::engine::Automaton> {
    pub(crate) name: String,
    pub(crate) state: Arc<Mutex<crate::engine::ServoState<A>>>,
}

impl<A: crate::engine::Automaton> AutomatonInspector for ServoInspector<A> {
    fn snapshot(&self) -> AutomatonSnapshot {
        let s = self.state.lock();
        let mut extra = HashMap::new();
        extra.insert("time".into(), s.time.seconds as f64);
        extra.insert("base".into(), s.base);
        extra.insert("frozen".into(), if s.frozen { 1.0 } else { 0.0 });
        AutomatonSnapshot {
            name: self.name.clone(),
            enabled: s.enabled,
            value: s.value.to_f64().unwrap_or(s.base),
            extra,
        }
    }
}
```

Note: `ServoState` and `Automaton` are `pub(crate)` — accessible from within the same crate.

- [ ] **Step 3: Add OscSensor::inspect() in osc.rs**

```rust
#[cfg(feature = "debug")]
impl OscSensor {
    pub fn inspect(&self) -> crate::debug::SensorSnapshot {
        crate::debug::SensorSnapshot {
            name: self.id.clone(),
            kind: "osc".into(),
            connected: self.thread.is_some(),
            event_count: 0,
            last_event: None,
            tracker_active: false,
        }
    }
}
```

- [ ] **Step 4: Add MidiHub::inspect() in midi.rs**

```rust
#[cfg(feature = "debug")]
impl MidiHub {
    pub fn inspect(&self) -> crate::debug::SensorSnapshot {
        crate::debug::SensorSnapshot {
            name: self.id.clone(),
            kind: "midi".into(),
            connected: self.thread.is_some(),
            event_count: 0,
            last_event: None,
            tracker_active: self.tracker.is_some(),
        }
    }
}
```

- [ ] **Step 5: Verify**

```bash
cargo check -p rill-patchbay --features debug
```

- [ ] **Step 6: Commit**

```bash
git add rill-patchbay/src/engine.rs rill-patchbay/src/osc.rs rill-patchbay/src/midi.rs rill-patchbay/src/debug.rs
git commit -m 'feat(rill-patchbay): add Servo::inspector(), OscSensor::inspect(), MidiHub::inspect()'
```

---

### Task 6: ModuleFactory Inspector Registration

**Files:**
- Modify: `rill-patchbay/src/module_factory.rs`
- Modify: `rill-patchbay/src/servo_constructor.rs`

- [ ] **Step 1: Add inspector param to ModuleFactory::construct()**

```rust
#[cfg(feature = "debug")]
use crate::debug::PatchbayInspector;

pub fn construct(
    &self,
    module: &ModuleDef,
    automaton_defs: &[AutomatonDef],
    system: &Arc<ActorSystem>,
    graph_ref: &ActorRef<CommandEnum>,
    #[cfg(feature = "debug")] inspector: Option<&PatchbayInspector>,
) -> Result<ActorRef<CommandEnum>, ModuleError> {
    // ... existing logic ...
    // Pass inspector through to the constructor callback
}
```

The `ModuleConstructor::construct()` trait method needs the same optional param.

- [ ] **Step 2: Update ModuleConstructor trait**

```rust
pub trait ModuleConstructor: Send + Sync {
    fn type_name(&self) -> &'static str;
    fn construct(
        &self,
        module: &ModuleDef,
        automaton_defs: &[AutomatonDef],
        system: &Arc<ActorSystem>,
        graph_ref: &ActorRef<CommandEnum>,
        #[cfg(feature = "debug")] inspector: Option<&PatchbayInspector>,
    ) -> Result<ActorRef<CommandEnum>, ModuleError>;
    fn clone_box(&self) -> Box<dyn ModuleConstructor>;
}
```

- [ ] **Step 3: Update ServoConstructor to register inspector**

In `rill-patchbay/src/servo_constructor.rs`, find the `construct()` method. After creating the Servo, register with inspector:

```rust
#[cfg(feature = "debug")]
if let Some(inspector) = inspector {
    let automaton_inspector = servo.inspector();
    inspector.register_automaton(module.type_name().to_string(), automaton_inspector);
}
```

- [ ] **Step 4: Update all call sites of construct()**

Search for `.construct(` in the codebase. Update each call to add `None` for the inspector param or `Some(&inspector)` when available.

- [ ] **Step 5: Verify**

```bash
cargo check -p rill-patchbay --features debug
cargo check -p rill-patchbay
```

- [ ] **Step 6: Commit**

```bash
git add rill-patchbay/src/
git commit -m 'feat(rill-patchbay): add inspector registration to ModuleFactory and ServoConstructor'
```

---

## Phase 3: CollectorThread + ModularSystem Integration

### Task 7: CollectorThread Inspector Handling

**Files:**
- Modify: `rill-telemetry/src/debug/collector_thread.rs`
- Modify: `rill-telemetry/src/debug/state.rs`

- [ ] **Step 1: Add PatchbayInspector to CollectorThread::spawn()**

Add an optional `inspector: Option<std::sync::Arc<rill_patchbay::debug::PatchbayInspector>>` parameter.

Wait — `rill-telemetry` doesn't depend on `rill-patchbay`. Use a trait object or generic approach instead.

**Better approach:** Pass inspector through a closure or trait. The simplest: add an `inspect_handler: Option<Box<dyn Fn(AnalyzerCommand) -> Option<AnalyzerResponse> + Send>>` parameter to `CollectorThread::spawn()`.

In the thread loop, after `state_manager.handle_command(cmd)` returns the catch-all error, check the inspect handler:

```rust
let resp = state_manager.handle_command(cmd);
if matches!(resp, AnalyzerResponse::Error(_)) {
    if let Some(ref handler) = inspect_handler {
        if let Some(custom_resp) = handler(cmd) {
            // in shmem mode: shmem.write_response(&custom_resp);
            // in mpsc mode: resp_tx.send(custom_resp);
        }
    }
}
```

- [ ] **Step 2: Add inspect_handler param**

```rust
pub fn spawn(
    config: AnalyzerConfig,
    probe_states: Arc<DashMap<ProbeId, ProbeState>>,
    probe_queues: Vec<Arc<SpscQueue<ProbeFrame, 64>>>,
    command_queue: Arc<SpscQueue<CommandFrame, 256>>,
    probe_slots: Vec<Arc<ProbeSlot>>,
    debug_control: DebugControl,
    resp_tx: mpsc::Sender<AnalyzerResponse>,
    shmem: Option<ShmemRegion>,
    inspect_handler: Option<Box<dyn Fn(&AnalyzerCommand) -> Option<AnalyzerResponse> + Send>>,
) -> (Self, mpsc::Sender<AnalyzerCommand>)
```

- [ ] **Step 3: Wire handler in the main loop**

In the command processing section, after `state_manager.handle_command(cmd)`:

```rust
let mut resp = state_manager.handle_command(cmd);
if matches!(resp, AnalyzerResponse::Error(_)) {
    if let Some(ref handler) = inspect_handler {
        if let Some(custom) = handler(&cmd) {
            resp = custom;
        }
    }
}
// Send/write resp
```

- [ ] **Step 4: Verify**

```bash
cargo check -p rill-telemetry --features debug
```

- [ ] **Step 5: Commit**

```bash
git add rill-telemetry/src/debug/collector_thread.rs rill-telemetry/src/debug/state.rs
git commit -m 'feat(rill-telemetry): add inspect_handler to CollectorThread for custom commands'
```

---

### Task 8: ModularSystem Debug Wiring

**Files:**
- Modify: `rill-adrift/src/modular/mod.rs`
- Modify: `rill-adrift/Cargo.toml`

- [ ] **Step 1: Ensure debug feature enables needed deps**

In `rill-adrift/Cargo.toml`:
```toml
debug = ["telemetry", "rill-telemetry/debug", "rill-patchbay/debug"]
```

- [ ] **Step 2: Add imports in mod.rs**

At the top of the file:
```rust
#[cfg(feature = "debug")]
use rill_telemetry::debug::{
    collector_thread::CollectorThread,
    protocol::{AnalyzerConfig, AnalyzerCommand, AnalyzerResponse, OutputMode},
    state::ProbeState,
};
#[cfg(feature = "debug")]
use rill_patchbay::debug::PatchbayInspector;
#[cfg(feature = "debug")]
use std::sync::Arc;
```

- [ ] **Step 3: Modify signal closure to send debug state**

Replace the `mpsc::channel::<ActorRef<CommandEnum>>()` with a struct:

```rust
#[cfg(feature = "debug")]
struct DebugState {
    probe_slots: Vec<Arc<rill_lang::debug::ProbeSlot>>,
    debug_control: rill_lang::debug::DebugControl,
    command_queue: Arc<rill_core::queues::spsc::SpscQueue<rill_lang::debug::CommandFrame, 256>>,
    node_names: Vec<String>,
}

let (graph_tx, graph_rx) = std::sync::mpsc::channel::<(ActorRef<CommandEnum>, Option<DebugState>)>();
```

In the signal closure, after engine creation:
```rust
#[cfg(feature = "debug")]
let debug_state = Some(DebugState {
    probe_slots: engine.clone_debug_state().0,
    debug_control: engine.clone_debug_state().1,
    command_queue: engine.clone_debug_state().2,
    node_names: scheduled.program_names.clone(),
});
#[cfg(not(feature = "debug"))]
let debug_state: Option<DebugState> = None;

let _ = graph_tx.send((engine.handle(), debug_state));
```

- [ ] **Step 4: Receive debug state and build inspector**

After `let graph_ref = graph_rx.recv()...`:

```rust
let (graph_ref, debug_state) = graph_rx
    .recv()
    .map_err(|e| ModularError::Graph(format!("graph handle: {e}")))?;
```

- [ ] **Step 5: Create PatchbayInspector, register with ModuleFactory**

Before the module construction loop:

```rust
#[cfg(feature = "debug")]
let inspector = Arc::new(PatchbayInspector::new());
```

Update `ModuleFactory::construct()` calls to pass `inspector.as_deref()`.

- [ ] **Step 6: After module construction, spawn CollectorThread**

After the module construction loop:

```rust
#[cfg(feature = "debug")]
if let Some(ds) = debug_state {
    let shmem = crate::debug_init::init_shmem()
        .or_else(crate::debug_init::init_shmem_from_env);

    let inspector_clone = inspector.clone();
    let inspect_handler: Box<dyn Fn(&AnalyzerCommand) -> Option<AnalyzerResponse> + Send> =
        Box::new(move |cmd: &AnalyzerCommand| -> Option<AnalyzerResponse> {
            match cmd {
                AnalyzerCommand::ListAutomata => {
                    Some(AnalyzerResponse::AutomataList(inspector_clone.list_automata()))
                }
                AnalyzerCommand::GetAutomatonState { name } => {
                    inspector_clone.get_automaton_snapshot(name)
                        .map(|snap| {
                            let json = serde_json::to_string(&snap).unwrap_or_default();
                            AnalyzerResponse::AutomatonState(json)
                        })
                }
                AnalyzerCommand::ListSensors => {
                    Some(AnalyzerResponse::SensorList(inspector_clone.list_sensors()))
                }
                AnalyzerCommand::GetSensorStatus { name } => {
                    inspector_clone.get_sensor_snapshot(name)
                        .map(|snap| {
                            let json = serde_json::to_string(&snap).unwrap_or_default();
                            AnalyzerResponse::SensorStatus(json)
                        })
                }
                _ => None,
            }
        });

    let probe_states = Arc::new(DashMap::new());
    // Register probe states from node_names
    for (i, name) in ds.node_names.iter().enumerate() {
        probe_states.insert(i as u32, ProbeState {
            name: name.clone(),
            node_name: name.clone(),
        });
    }

    let probe_queues: Vec<_> = ds.probe_slots.iter()
        .map(|s| s.queue.clone())
        .collect();

    let (resp_tx, _resp_rx) = std::sync::mpsc::channel();
    let (_, cmd_tx) = CollectorThread::spawn(
        AnalyzerConfig::default(),
        probe_states,
        probe_queues,
        ds.command_queue,
        ds.probe_slots,
        ds.debug_control,
        resp_tx,
        shmem,
        Some(inspect_handler),
    );
    // Store cmd_tx somewhere for programmatic control (optional for MVP)
    log::info!("rill-debug: collector thread started for rack '{}'", rd.name);
}
```

- [ ] **Step 7: Verify**

```bash
cargo check -p rill-adrift --features debug
cargo check -p rill-adrift
```

- [ ] **Step 8: Commit**

```bash
git add rill-adrift/src/modular/mod.rs rill-adrift/Cargo.toml && git commit -m 'feat(rill-adrift): wire CollectorThread and PatchbayInspector into ModularSystem::launch'
```

---

## Phase 4: Polish

### Task 9: Workspace Check

- [ ] **Step 1: Full workspace compilation**

```bash
cargo check --workspace
```

- [ ] **Step 2: Clippy**

```bash
cargo clippy --workspace
```

Fix any new warnings.

- [ ] **Step 3: Tests**

```bash
cargo test --workspace
```

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m 'chore: workspace check clean for patchbay integration'
```

---

## Plan Summary

| Phase | Tasks | Crate | Key Deliverable |
|---|---|---|---|
| 1 | 1 | `rill-lang` | `Arc<ProbeSlot>`, `clone_debug_state()` |
| 2 | 2-6 | `rill-adrift`, `rill-telemetry`, `rill-patchbay` | Logging, protocol ext, PatchbayInspector, Servo/Sensor inspect |
| 3 | 7-8 | `rill-telemetry`, `rill-adrift` | CollectorThread inspect_handler, ModularSystem debug wiring |
| 4 | 9 | All | Workspace check |

**Total tasks: 9**
