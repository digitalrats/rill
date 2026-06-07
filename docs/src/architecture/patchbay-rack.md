# Patchbay Rack

The Patchbay is the **control rack** — an independent subsystem that hosts
modulation generators (automata), event dispatch (MidiHub, OSC), and the
mapping layer that translates external events into graph parameter commands.

```
┌─ Control Rack (Patchbay, soft‑RT) ─────────────────────────────────────┐
│                                                                         │
│  Modules:                                                               │
│  ┌──────────┐  ┌──────────┐  ┌──────────────┐                          │
│  │ Automata │  │  Midi    │  │  Sequencer   │                          │
│  │ (LFO,ENV)│  │  Input   │  │              │                          │
│  └────┬─────┘  └────┬─────┘  └──────┬───────┘                          │
│       │             │               │                                   │
│       ▼             ▼               ▼                                   │
│  ┌─────────────────────────────────────────┐                           │
│  │            PortCombiner(s)               │                           │
│  │    merge automaton + UI with conflict    │                           │
│  │    resolution strategies                │                           │
│  └───────────────────┬─────────────────────┘                           │
│                      │                                                  │
│  ┌───────────────────▼─────────────────────┐                           │
│  │              Mappings                    │                           │
│  │  EventPattern → (node, param, range)    │                           │
│  └───────────────────┬─────────────────────┘                           │
│                      │ ActorRef<SetParameter>                          │
│                      ▼ MpscQueue (lock‑free)                           │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌─ Signal Rack (Graph, hard‑RT) ────────────────────────────────────┐ │
│  │  drain queue → set_parameter → process_block → propagate          │ │
│  │  Input → [processors] → Output                                    │ │
│  └────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
```

## Domain model

The Patchbay is a **rack** — a container for modules. Each module is optional
and configured through a single document (`PatchbayDef`):

| Module | Role | Configured via |
|--------|------|---------------|
| **Automata** | Modulation generators (LFO, envelope) | `automata` + `servos` |
| **MidiInput** | External MIDI event source | `SensorDef::Midi` |
| **OscSensor** | External OSC event source (UDP) | `SensorDef::Osc` |
| **Sequencer** | Step sequencer driven by signal clock | `attach_sequencer()` |
| **OscSurface** | OSC → EventPattern bridge | `osc_surface` |

All modules produce `ControlEvent`s that flow through **mappings** →
`SetParameter` commands → graph's lock‑free queue.

## PatchbayDef — single configuration document

```rust
pub struct PatchbayDef {
    /// Modulation generators (LFO, envelope, named functions)
    pub automata: Vec<AutomatonDef>,

    /// Generator → graph parameter wiring
    pub servos: Vec<ServoDef>,

    /// Event → graph parameter wiring (MIDI CC, OSC address, etc.)
    pub mappings: Vec<MappingDef>,

    /// OSC address → EventPattern bridge
    pub osc_surface: OscSurface,

    /// Unified modules — servos and sensors
    pub modules: Vec<ModuleDef>,

    /// Human‑readable description
    pub description: Option<String>,
}
```

Sensors are configured through `ModuleDef::Sensor`:

```rust
// MIDI sensor
ModuleDef::Sensor(SensorDef::Midi {
    backend: "midir".into(),
    port_name: "rill-midi".into(),
    mappings: vec![...],
})

// OSC sensor
ModuleDef::Sensor(SensorDef::Osc {
    port: 9000,
    mappings: vec![
        MappingDef {
            event_pattern: EventPattern::OscAddress("/fader/1".into()),
            target_node: 1,
            target_param: "gain".into(),
            transform: TransformDef::Linear,
            min: 0.0,
            max: 1.0,
            enabled: true,
        },
    ],
})
```

**Behaviour:** `ModularSystem::launch()` dispatches each `ModuleDef::Sensor` to
the appropriate constructor (`MidiConstructor` or `OscConstructor`), which spawns
a sensor + mapping-only servo pair.

### MidiInputDef (legacy, superseded by SensorDef)

Currently, `MidiHub` is created programmatically — `PatchbayDef` has no
`midi` field. Adding it makes the MidiHub a **first‑class rack module**,
configurable from JSON:

```rust
pub struct MidiInputDef {
    /// Backend name: "midir" or "alsa_seq"
    pub backend: String,

    /// Virtual port name (e.g. "drift-midi" for aconnect)
    pub port_name: String,
}
```

**Behaviour:** `apply_to_async()` creates the `MidiBackend`, starts the
`MidiHub`, and stores the handle. `stop_all()` stops it.

```rust
#[cfg(feature = "midi")]
pub fn apply_to_async(&self, control: &mut Patchbay, registry: &FunctionRegistry) -> Result<(), String> {
    // ... existing automata/servos/mappings setup ...

    if let Some(ref midi_def) = self.midi {
        let backend: Box<dyn MidiBackend> = match midi_def.backend.as_str() {
            "midir" => Box::new(MidirBackend::new(&midi_def.port_name)?),
            "alsa_seq" => Box::new(AlsaSeqBackend::new(&midi_def.port_name)?),
            _ => return Err(format!("unknown midi backend: {}", midi_def.backend)),
        };
        let shared = Arc::new(Mutex::new(control.as_shared()));
        control.set_midi_actor(MidiHub::start(backend, shared));
    }

    Ok(())
}
```

This makes MIDI input purely a configuration concern — no extra code in
`Runtime` or `drift/main.rs`.

## One instance or two? — analysis

The current `Runtime::load_patchbay()` creates **two** `Patchbay` instances:

| Instance | Purpose | Fields populated |
|----------|---------|-----------------|
| `control` | Owns automaton handles (`port_combiners`, `automaton_handles`) | All |
| `control_shared` (`Arc<Mutex<>>`) | Receives events from OSC/MIDI | mappings only |

This split exists because **automata run as tokio green threads** (no Mutex
needed — channels do the work) while **event dispatch needs `&mut self`**
(protected by Mutex). The shared instance is a stripped copy with only
mappings.

### Option A: single `Arc<Mutex<Patchbay>>` (simpler)

```rust
let pb = Arc::new(Mutex::new(Patchbay::new(graph_handle)));
```

- **Event dispatch** (MidiHub): `pb.lock().handle_event(event)` — brief lock
- **Automaton setup**: `pb.lock().add_automaton_task(...)` — done once at init
- **Shutdown**: `pb.lock().stop_all()` — done once

The Mutex is **not** contended during runtime because automata communicate via
channels, not by locking Patchbay. Only the MidiHub's OS thread locks (briefly,
to run `handle_event`). One instance is sufficient.

### Option B: actor model via `ActorCell` (cleaner, long‑term)

`Patchbay` implements `ActorCell<ControlEvent>`, runs its own processing loop,
and MidiHub sends events via `ActorRef<ControlEvent>::send()` — lock‑free.

```
MidiHub (OS thread)                Patchbay (tokio task)
     │                                      │
     │  ActorRef<ControlEvent>::send()     │
     ├──────── lock‑free push ────────────→│
     │                                      ├─ while let Some(event) = mailbox.pop()
     │                                      │    handle_event(event)
     │                                      └─ → ActorRef<SetParameter>.send()
```

This eliminates the Mutex entirely and aligns with `rill-core-actor`. However,
it requires adding a processing loop to Patchbay and changes the lifecycle
(Patchbay becomes an async actor spawned on tokio, not a synchronous object).

### Recommendation

For the Moonlight demo: **Option A** — single `Arc<Mutex<Patchbay>>`. It works
with the existing codebase, requires no restructuring, and the Mutex is
uncontended in practice. The actor model (Option B) is the right long‑term
direction and should be documented as a future evolution.

## Runtime::launch() — two racks, one command

```rust
pub fn launch(config: LaunchConfig) -> Result<Runtime, Error> {
    // ── Create tokio runtime for control rack ──
    let tokio_rt = tokio::runtime::Runtime::new()?;
    let _guard = tokio_rt.enter();

    // ── Rack 2: Signal Graph ──
    let mut builder = self.create_builder();
    config.graph_def.populate(&mut builder)?;
    let mut graph = builder.build()?;
    let graph_handle = graph.handle().expect("no active node");

    // ── Rack 1: Control Patchbay ──
    let registry = FunctionRegistry::builtin();
    let mut control = Patchbay::new(graph_handle);
    config.patchbay_def
        .apply_to_async(&mut control, &registry)?;
    // ↑ One call: automata started, MIDI port opened,
    //   mappings loaded, PortCombiners running.

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    let signal_thread = std::thread::spawn(move || {
        graph.run(r).ok();
    });

    Ok(Runtime {
        control: Arc::new(Mutex::new(control)),
        signal_thread,
        running,
        _tokio: tokio_rt,
    })
}
```

**`Runtime::stop()`** — single exit point:

```rust
pub fn stop(&mut self) {
    self.running.store(false, Ordering::Release);

    // Stop control rack: automata, MidiHub, PortCombiners.
    if let Ok(mut pb) = self.control.lock() {
        pb.stop_all();
    }

    // Signal thread exits when graph.run() sees running=false.
    // Drop tokio runtime → remaining tasks cancelled.
}
```

## Summary of changes

| Crate | File | Change |
|-------|------|--------|
| `rill-patchbay` | `serialization/mod.rs` | Add `MidiInputDef`, `midi` field in `PatchbayDef` |
| `rill-patchbay` | `engine.rs` | Add `set_midi_actor()`, `as_shared()`, extend `stop_all()` |
| `rill-adrift` | `runtime/mod.rs` | `LaunchConfig`, `Runtime::launch()`, rewrite `stop()` |
| `rill-adrift` | `runtime/config.rs` | `LaunchConfig` struct |

The goal: `PatchbayDef` describes the entire control rack. `Runtime::launch()`
builds both racks and wires them together in one call.

## Future: feature-gated modules (Eurorack model)

Currently `rill-patchbay` is monolithic — all automaton types and modules are
compiled unconditionally. With feature gates, each module becomes a slot in
the rack: you install only what you need.

```
[features]
default = []
lfo       = []           # LfoAutomaton + ServoDef::Lfo variant
envelope  = []           # EnvelopeAutomaton + ServoDef::Envelope variant
sequencer = []           # SnapshotSequencer + attach_sequencer
midi      = ["rill-io"]  # MidiHub + MidiInputDef (already behind "midi")
osc       = ["rill-osc"] # OscSurface dispatch (deferred)
```

Usage in downstream crates:

```toml
# Drift — tape delay demo: LFO modulation + MIDI control
rill-patchbay = { features = ["lfo", "midi"] }

# Minimal setup — no automation, just MIDI CC mapping
rill-patchbay = { features = ["midi"] }

# Sequencer-only — clock-driven pattern changes, no LFO
rill-patchbay = { features = ["sequencer", "midi"] }
```

Implementation pattern (follows `rill-io` backend model):

```rust
#[cfg(feature = "lfo")]
impl AutomatonDef {
    pub fn apply_to(&self, control: &mut Patchbay, ...) { ... }
}
#[cfg(not(feature = "lfo"))]
impl AutomatonDef {
    pub fn apply_to(&self, _: &mut Patchbay, ...) {
        compile_error!("LFO module not installed in this rack");
    }
}
```

This makes `rill-patchbay` a literal Eurorack — each feature is a module you
snap into the control rack. The `Cargo.toml` of the consuming crate defines
which modules populate the rack at compile time.

**Status:** deferred. The current monolithic build is sufficient for the
Moonlight demo. Feature gates add build-time modularity without runtime cost
and should be introduced when the module set grows beyond two automaton types.
