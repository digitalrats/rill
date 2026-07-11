# Patchbay Integration: Logging + rill-analyzer Control-Path Inspection

**Date:** 2026-07-11
**Status:** Draft

## Motivation

The debug infrastructure currently covers the signal graph (probes, command log, pause/resume). But the control path — ModularSystem, Servo automata, MIDI/OSC sensors — is completely invisible. For debugging scenarios like `chiptune_stc`, we need to see:
- Who sets which parameter and when (automaton → SetParameter flow)
- Whether servos are enabled, what value they're producing
- Sensor status (connected, last event)

Additionally, the ModularSystem lifecycle has zero logging — startup failures are silent.

## Design

### 1. Logging — Lifecycle Events

Add `log::info!`/`log::warn!`/`log::error!` calls in `rill-adrift/src/modular/mod.rs`. The `log` crate is already a dependency.

| Event | Level | Location |
|---|---|---|
| Rack launch started | `info!` | `launch()` — per-rack loop start |
| Graph compiled (nodes, steps) | `info!` | After `graph_lower::lower()` |
| Backend connected | `info!` | After `runner.run_with_driver()` |
| Backend registration | `error!` | `backend create` error branch |
| Graph populate error | `error!` | `graph_def.populate()` error |
| Graph build_ir error | `error!` | `builder.build_ir()` error |
| Module construction error | `warn!` | ModuleFactory construction failure |
| Rack stop | `info!` | `stop()` — per-rack |
| System stop | `info!` | `stop()` — all racks |

### 2. Arc<ProbeSlot> — Shared Between Threads

**Problem:** `RillGraphEngine` owns `Vec<ProbeSlot>`. But `ProbeSlot` must be shared between the signal thread (writes probe frames) and the collector thread (reads them). Currently `ProbeSlot` is stored inline — it must become `Arc<ProbeSlot>` to be clonable.

**Change in `rill-lang/src/graph_engine.rs`:**

```rust
#[cfg(feature = "debug")]
pub(crate) probe_slots: Vec<Arc<ProbeSlot>>,
```

Engine code accesses via `self.probe_slots[id].enabled.store(...)` — same syntax, just one extra deref.

New extraction method:

```rust
#[cfg(feature = "debug")]
pub fn extract_debug_state(&mut self) -> (
    Vec<Arc<ProbeSlot>>,
    DebugControl,
    Arc<SpscQueue<CommandFrame, 256>>,
) {
    (
        std::mem::take(&mut self.probe_slots),
        self.debug_control.clone(),
        self.command_queue.clone(),
    )
}
```

`std::mem::take` replaces `self.probe_slots` with an empty Vec. The signal thread still holds `Arc<ProbeSlot>` references through whatever it cached before the take — but the engine's probe capture code uses `self.probe_slots[id]`, so it MUST keep the Vec populated.

**Actually, simpler approach:** Don't take. Clone the Vec of Arcs:

```rust
#[cfg(feature = "debug")]
pub fn clone_debug_state(&self) -> (
    Vec<Arc<ProbeSlot>>,
    DebugControl,
    Arc<SpscQueue<CommandFrame, 256>>,
) {
    (
        self.probe_slots.clone(),       // clones Vec<Arc<>> — Arcs are shared
        self.debug_control.clone(),
        self.command_queue.clone(),
    )
}
```

This works: the signal thread keeps its `self.probe_slots`, the collector gets a clone with the same Arc references. Both access the same atomics and SpscQueues.

### 3. PatchbayInspector — Control-Path State

New module `rill-patchbay/src/debug.rs` (behind `#[cfg(feature = "debug")]`):

```rust
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;
use dashmap::DashMap;

/// Snapshot of an automaton's current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomatonSnapshot {
    pub name: String,
    pub enabled: bool,
    pub value: f64,
    pub extra: HashMap<String, f64>,
}

/// Snapshot of a sensor's current status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorSnapshot {
    pub name: String,
    pub kind: String,
    pub connected: bool,
    pub event_count: u64,
    pub last_event: Option<String>,
}

/// Collects control-path state for debugger inspection.
pub struct PatchbayInspector {
    /// Automaton state providers, keyed by module name.
    automata: DashMap<String, Box<dyn AutomatonInspector>>,
    /// Sensor inspect functions, keyed by module name.
    sensors: DashMap<String, Box<dyn SensorInspector>>,
}

pub trait AutomatonInspector: Send + Sync {
    fn snapshot(&self) -> AutomatonSnapshot;
}

pub trait SensorInspector: Send + Sync {
    fn snapshot(&self) -> SensorSnapshot;
}

impl PatchbayInspector {
    pub fn new() -> Self { ... }

    pub fn register_automaton(&self, name: String, inspector: Box<dyn AutomatonInspector>) { ... }
    pub fn register_sensor(&self, name: String, inspector: Box<dyn SensorInspector>) { ... }

    pub fn list_automata(&self) -> Vec<String> { ... }
    pub fn get_automaton_snapshot(&self, name: &str) -> Option<AutomatonSnapshot> { ... }
    pub fn list_sensors(&self) -> Vec<String> { ... }
    pub fn get_sensor_snapshot(&self, name: &str) -> Option<SensorSnapshot> { ... }
}
```

### 4. New AnalyzerCommand / AnalyzerResponse Variants

Add to `rill-telemetry/src/debug/protocol.rs`:

```rust
pub enum AnalyzerCommand {
    // ... existing ...
    ListAutomata,
    GetAutomatonState { name: String },
    ListSensors,
    GetSensorStatus { name: String },
    ListQueues,
}

pub enum AnalyzerResponse {
    // ... existing ...
    AutomataList(Vec<String>),
    AutomatonState(AutomatonSnapshot),
    SensorList(Vec<String>),
    SensorStatus(SensorSnapshot),
    QueueList(Vec<QueueStats>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    pub name: String,
    pub capacity: usize,
    pub len: usize,
    pub is_full: bool,
    pub pushes: u64,
    pub pops: u64,
    pub overflows: u64,
}
```

`AutomatonSnapshot` and `SensorSnapshot` are defined in `rill-patchbay/src/debug.rs`. They need `Serialize, Deserialize` for IPC transport. Add `serde` as an optional dependency to rill-patchbay under `debug` feature.

### 5. Integration Point — ModularSystem::launch()

```rust
#[cfg(feature = "debug")]
use rill_telemetry::debug::{
    collector_thread::CollectorThread,
    ipc::ShmemRegion,
    state::ProbeState,
};
#[cfg(feature = "debug")]
use rill_patchbay::debug::PatchbayInspector;
```

After the module construction loop (line 266), after `self.tokio_rt = Some(tokio_rt)`:

```rust
        #[cfg(feature = "debug")]
        {
            // 1. Create/open shmem region
            let shmem = crate::debug_init::init_shmem()
                .or_else(crate::debug_init::init_shmem_from_env);

            if let Some(shmem) = shmem {
                log::info!("rill-debug: shmem ready for rack '{}'", rd.name);

                // 2. Collect probe slots, command queue, debug control from engine
                //    The engine is inside ProgramRunner on the signal thread.
                //    We need to send debug state through an mpsc channel from the
                //    signal thread. Currently the signal closure (line 190-235)
                //    creates the engine — we modify it to also send debug state.

                // 3. Build PatchbayInspector from registered modules
                let inspector = Arc::new(PatchbayInspector::new());
                for (name, actor_ref) in modules.lock().unwrap().iter() {
                    // For Servo modules: register automaton inspector
                    // For Sensor modules: register sensor inspector
                    // This requires Servo/Sensor to expose inspect() methods
                }

                // 4. Spawn CollectorThread with shmem + inspector
                //    (requires debug state from signal thread — see step 2)
            }
        }
```

**Step 2 detail:** The signal closure currently sends only `engine.handle()` through `graph_tx`. We need to change the channel type to carry `(ActorRef<CommandEnum>, DebugState)`:

```rust
struct DebugState {
    probe_slots: Vec<std::sync::Arc<rill_lang::debug::ProbeSlot>>,
    debug_control: rill_lang::debug::DebugControl,
    command_queue: std::sync::Arc<rill_core::queues::spsc::SpscQueue<rill_lang::debug::CommandFrame, 256>>,
    node_names: Vec<String>,
}
```

The signal closure after engine creation:
```rust
let engine = RillGraphEngine::new(scheduled, programs, mailbox, buf_size);

#[cfg(feature = "debug")]
let debug_state = DebugState {
    probe_slots: engine.clone_debug_state().0,
    debug_control: engine.clone_debug_state().1,
    command_queue: engine.clone_debug_state().2,
    node_names: scheduled.program_names.clone(),
};

let _ = graph_tx.send((engine.handle(), debug_state));
```

**Step 3 detail:** Currently `modules` is `Arc<Mutex<HashMap<String, ActorRef<CommandEnum>>>>`. The `ActorRef` only allows sending commands, not querying state. To support inspection, Servo and Sensor need to expose an `inspect()` method or register themselves with the `PatchbayInspector` during construction.

The cleanest approach: `ModuleFactory::construct()` returns `ActorRef<CommandEnum>` AND optionally registers with `PatchbayInspector`. Add an optional parameter:

```rust
pub fn construct(
    &self,
    module: &ModuleDef,
    automaton_defs: &[AutomatonDef],
    system: &Arc<ActorSystem>,
    graph_ref: &ActorRef<CommandEnum>,
    inspector: Option<&PatchbayInspector>,  // NEW
) -> Result<ActorRef<CommandEnum>, ModuleError>
```

### 6. Servo Inspection API

Add to `rill-patchbay/src/engine.rs`:

```rust
impl<A: Automaton> Servo<A> {
    /// Return an inspector that can snapshot this servo's state.
    pub fn inspector(&self) -> Option<Box<dyn AutomatonInspector>> {
        let state = self.state.clone();
        let name = self.id.clone();
        Some(Box::new(ServoInspector { name, state }))
    }
}

struct ServoInspector<A: Automaton> {
    name: String,
    state: Arc<Mutex<ServoState<A>>>,
}

impl<A: Automaton> AutomatonInspector for ServoInspector<A> {
    fn snapshot(&self) -> AutomatonSnapshot {
        let s = self.state.lock();
        AutomatonSnapshot {
            name: self.name.clone(),
            enabled: s.enabled,
            value: s.value.to_f64().unwrap_or(s.base),
            extra: {
                let mut m = HashMap::new();
                m.insert("time".into(), s.time.seconds as f64);
                m.insert("base".into(), s.base);
                m.insert("frozen".into(), if s.frozen { 1.0 } else { 0.0 });
                m
            },
        }
    }
}
```

Wait — `ServoState<A>` is not public. It's `pub(crate)`. The `ServoInspector` needs to be inside the `rill-patchbay` crate anyway, so `pub(crate)` is fine.

Actually, `ServoInspector` is a new struct living inside `rill-patchbay/src/debug.rs`. It has access to `pub(crate)` types.

## Implementation Plan

### Task 1: Arc<ProbeSlot> + clone_debug_state

**Files:**
- `rill-lang/src/debug.rs` — add `ProbeSlot: Send + Sync` safety docs
- `rill-lang/src/graph_engine.rs` — change `probe_slots` to `Vec<Arc<ProbeSlot>>`, add `clone_debug_state()`
- Update all capture sites to use `self.probe_slots[id]` (Arc Deref works)

### Task 2: Lifecycle Logging

**Files:**
- `rill-adrift/src/modular/mod.rs` — add `log::info!/warn!/error!` in `launch()` and `stop()`

### Task 3: Protocol Extension

**Files:**
- `rill-telemetry/src/debug/protocol.rs` — add `ListAutomata`, `GetAutomatonState`, `ListSensors`, `GetSensorStatus`, `ListQueues` commands + responses + `QueueStats` struct

### Task 4: PatchbayInspector

**Files:**
- `rill-patchbay/src/debug.rs` (new) — `PatchbayInspector`, `AutomatonInspector`, `SensorInspector` traits, `AutomatonSnapshot`, `SensorSnapshot`
- `rill-patchbay/Cargo.toml` — add `debug` feature, `serde` dependency

### Task 5: Servo Inspection

**Files:**
- `rill-patchbay/src/engine.rs` — add `Servo::inspector()` method
- `rill-patchbay/src/debug.rs` — `ServoInspector` impl

### Task 6: CollectorThread Integration

**Files:**
- `rill-telemetry/src/debug/collector_thread.rs` — handle new `AnalyzerCommand` variants via `PatchbayInspector`

### Task 7: ModularSystem Integration

**Files:**
- `rill-adrift/src/modular/mod.rs` — modify signal closure to send DebugState; wire CollectorThread + PatchbayInspector after module construction
- `rill-adrift/Cargo.toml` — ensure `debug` feature enables everything needed

### Task 8: Workspace Check

- `cargo check --workspace`, `cargo clippy`, `cargo test`
