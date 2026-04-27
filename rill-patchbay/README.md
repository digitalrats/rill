# rill-patchbay

Automation and control system — LFO, envelopes, sensors, servos, and event mapping.

## Key components

- **Automata** — `LfoAutomaton`, `EnvelopeAutomaton`, `RandomWalkAutomaton`, `SequencerAutomaton`, `FunctionAutomaton`, `CellularAutomaton`
- **Sensors** — acoustic (pitch, envelope follower), physical (knobs, buttons), MIDI, CV
- **Servos** — apply automaton signals to AudioGraph parameters
- **`PatchbayControl`** — centralized API for adding automata, sensors, and mappings
- **`PatchbayManager`** — manager with a separate update thread
- **Event mapping** — MIDI CC → parameter, OSC address → parameter, with transforms

## Dependencies

- `rill-core` — `AudioNode`, `NodeId`, `PortId`, `ParameterId`, queues

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-patchbay>
