# rill-patchbay

Automation and control system — LFOs, envelopes, sequencers, sensors, servos, and event mapping for the Rill audio graph.

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
- **`Engine`** — centralised API for adding automatons, servos, and
  mappings (fka `PatchbayControl`).

## Usage

```rust
use rill_core_actor::ActorRef;
use rill_patchbay::prelude::*;

let (actor_ref, _mailbox) = ActorRef::new_pair();
let mut engine = Engine::new(actor_ref);

engine.add_lfo(
    "vibrato", 5.0, 0.5, 0.0, LfoWaveform::Sine,
    osc_node_id, "frequency", 400.0, 480.0,
);

engine.add_envelope(
    "amp", 0.01, 0.1, 0.7, 0.2,
    vca_node_id, "gain", 0.0, 1.0,
);

engine.update(1.0 / 60.0);
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
- `crossbeam-channel`, `parking_lot`, `tokio` — green thread infrastructure

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-patchbay>
