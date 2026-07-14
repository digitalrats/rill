# rill-patchbay

Automation and control system — LFOs, envelopes, sequencers, sensors, servos, and event mapping for the Rill signal graph.

## Architecture

Two-thread design. Automata run inside **Servos** on the control thread
(tokio actors) and communicate with the signal graph through lock-free
actor mailboxes (`ActorRef<CommandEnum>`).

```
Control thread (soft-RT):                     Signal thread (hard-RT):
  ┌──────────┐   ┌──────────┐
  │ Automaton│   │  Sensor  │                 ┌──────────────────┐
  │ (LFO,ENV)│   │(MIDI,OSC)│                 │  I/O callback    │
  └────┬─────┘   └────┬─────┘                 │  actor.drain()   │
       │              │                       │  generate()      │
       ▼              ▼                       │  process()       │
  ┌──────────────────────────┐   ClockTick    │  propagate()     │
  │        Servo             │◄───────────────│                  │
  │  automaton.step()       │                └────────▲─────────┘
  │  mapping.apply()        │                         │
  │  strategy: control+     │    SetParameter          │
  │            conflict     │─────────────────────────┘
  └──────────────────────────┘
```

Conflicts between automaton output and HID input (MIDI knob, OSC fader)
are resolved inside the Servo via `ControlStrategy` and `ConflictStrategy`. 
See `strategy.rs`.

## Key components

- **Automata** — `LfoAutomaton`, `EnvelopeAutomaton`, `RandomWalkAutomaton`,
  `SequencerAutomaton`, `FunctionAutomaton`, `CellularAutomaton`
- **Servos** — bridge automatons to graph node parameters via
  `ParameterMapping` (Linear, Exponential, Logarithmic, Inverted, Custom).
  Also apply sensor event mappings (MIDI CC → param, OSC address → param).
  Built-in conflict resolution via `ControlStrategy` (Absolute / Modulation)
  and `ConflictStrategy` (TouchOverride / BasePlusModulation / LastWriteWins).
- **Sensors** — acoustic (pitch, envelope follower), physical (knobs,
  buttons), MIDI, OSC (UDP-based address/argument sensors).
- **Event mapping** — MIDI CC → parameter, OSC address → parameter,
  with transforms.
- **`Servo`** — centralised API for bridging automatons to graph
  parameters, adding mappings, and handling sensor events (fka
  `PatchbayControl`).

## Usage

```rust
use std::sync::Arc;
use rill_core::queues::CommandEnum;
use rill_core_actor::{ActorRef, ActorSystem};
use rill_patchbay::prelude::*;

let system = Arc::new(ActorSystem::new());
let (graph_ref, mut graph_actor) = {
    let mut actor = system.spawn("graph", |_cmd: CommandEnum| {});
    (actor.actor_ref(), actor)
};

let lfo = LfoAutomaton::new("vibrato", 5.0, 0.5, 0.0, LfoWaveform::Sine);
let servo = Servo::new(
    "vibrato", lfo, osc_node_id, "frequency",
    ParameterMapping::Linear, 400.0, 480.0,
    system.clone(), graph_ref.clone(),
);
let _lfo_ref = servo.spawn(&system);

let env = EnvelopeAutomaton::adsr("amp_env", 0.01, 0.1, 0.7, 0.2);
let servo_env = Servo::new(
    "amp_env", env, vca_node_id, "gain",
    ParameterMapping::Linear, 0.0, 1.0,
    system.clone(), graph_ref.clone(),
);
let _env_ref = servo_env.spawn(&system);

graph_actor.drain();
```

## Feature flags

| Feature | Description |
|---------|-------------|
| `serde` | Serialization support (JSON/CBOR) |
| `json` | `serde` + JSON serialization |
| `cbor` | `serde` + CBOR serialization |
| `serialization` | `json` + `cbor` |
| `midi` | MIDI input via `rill-io` backends |
| `osc` | OSC input via `rill-osc` |
| `debug` | Control-path inspection (PatchbayInspector, automaton/sensor snapshots) |

### Debug infrastructure (`debug` feature)

- **`PatchbayInspector`** — collects automaton and sensor snapshots for control-path
  debugging. Automata report enabled/disabled state, current output value, and
  internal state (time, phase). Sensors report connection status and event count.
- **`Servo::inspector()`** — returns an `AutomatonInspector` that snapshots the
  servo's internal state via `Arc<Mutex<ServoState<A>>>`
- **`OscSensor::inspect()` / `MidiHub::inspect()`** — capture sensor status
  (connected, tracker active) for the debugger

## Dependencies

- `rill-core` — node traits, queues, types
- `rill-core-actor` — actor model for lock-free message passing
- `tokio` — green thread infrastructure

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-patchbay>
