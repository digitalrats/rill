# rill-patchbay

Automation and control system — LFOs, envelopes, sequencers, sensors, servos, and event mapping for the Rill audio graph.

## Architecture

Two-thread design. Automata run on the **control thread** (green threads
via tokio) and communicate with the audio thread through lock-free
`MpscQueue<ParameterCommand>`.

```
Control thread (tokio):
  Automaton ── mpsc<f64> ──→ PortCombiner ── MpscQueue ──→ Audio thread
  UI/MIDI    ── mpsc<UiCmd> → PortCombiner ── MpscQueue ──→ Audio thread
  Sequencer  ◀── crossbeam<Telemetry> ◀──────────────────── Audio thread
```

## Key components

- **Automata** — `LfoAutomaton`, `EnvelopeAutomaton`, `RandomWalkAutomaton`,
  `SequencerAutomaton`, `FunctionAutomaton`, `CellularAutomaton`
- **PortCombiner** — sits between automaton and audio thread. Resolves
  conflicts between automaton output and UI/MIDI/OSC input using
  `ControlStrategy` (Absolute / Modulation) and `ConflictStrategy`.
- **Sequencer** — `SnapshotSequencer` driven by clock ticks from the audio
  thread via crossbeam channel. `SequencerHandle` for start/stop/reset.
- **Servos** — apply automaton signals to graph node parameters via
  `ParameterMapping` (Linear, Exponential, Logarithmic, Inverted, Custom).
- **Sensors** — acoustic (pitch, envelope follower), physical (knobs,
  buttons), MIDI, OSC (UDP-based address/argument sensors).
- **Event mapping** — MIDI CC → parameter, OSC address → parameter,
  with transforms.
- **`Engine`** — centralised API for adding automata, servos, mappings,
  green threads, and port combiners (fka `PatchbayControl`).
- **`Manager`** — high-level coordinator with per-port cancellation
  domains.

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

## Dependencies

- `rill-core` — node traits, queues, types
- `crossbeam-channel`, `parking_lot`, `tokio` — green thread infrastructure

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-patchbay>
