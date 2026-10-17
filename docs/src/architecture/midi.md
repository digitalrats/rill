# MIDI support (rill-io + rill-patchbay)

MIDI is handled through a layered architecture with **separate input and
output paths**, both flowing through the actor model:

- **Input:** hardware → `MidiInput::poll()` → `parse_midi()` → `ControlEvent` → `ActorRef<CommandEnum>` → Servo → `SetParameter` → parameter surface (no graph transport in this branch — see the status note below).
- **Output:** `ClockTick` → `MidiClockGenerator` → `ControlEvent` → `serialize_to_midi()` → `MidiOutput::send()` → hardware.

## Status: the automatic clock/control plane is not wired

The MIDI traits and the patchbay clock generator/actor below are live library
APIs — including them in an application works, provided something in that
application produces `CommandEnum::ClockTick`. What is **not** live is the
automatic plumbing that used to feed them: no component broadcasts `ClockTick`
to a rack, and no rack wires a sensor/servo/clock to a parameter surface.

Concretely, in the current `rill-adrift` modular host:

- the rack actor only forwards the commands it receives; nothing sends it a
  `ClockTick`;
- rack programs execute through the IO runner and expose no parameter plane;
- `ModuleDef::Clock`, `ModuleDef::Sensor`, and `ModuleDef::Servo` modules are
  constructed at launch but never driven, so a clock output emits nothing after
  its initial transport message (see
  [Patchbay & rack architecture](patchbay-rack.md)).

Where the text below says "Rack broadcast" or "automatic", read it as the
**reserved design** for the deferred control/audio plan, not as current
behaviour. The internal mechanism (`MidiClockGenerator` advancing on
`ClockTick.sample_pos`, `serialize_to_midi`, the actor loop) is implemented and
tested; it is the source of those ticks that is missing.

## Architecture

```
┌── INPUT PATH (Dedicated OS thread, non‑RT) ────────────────────────────┐
│                                                                         │
│  MidiInput::poll()  ──→  parse_midi()  ──→  ControlEvent               │
│       │                        │                    │                   │
│  raw [u8; 3]             bytes → event       events.send(event)         │
│                                                    │                   │
└────────────────────────────────────────────────────┼───────────────────┘
                                                     │
                                        ActorRef<CommandEnum>
                                                     │
                                                     ▼
                                              Servo (mappings)
                                                     │
                                                SetParameter
                                                     │
                                                     ▼
                                              Graph command queue
                                          (reserved — no graph host
                                           in the current branch)

┌── OUTPUT PATH (Rack actor thread, non‑RT) ─────────────────────────────┐
│  (reserved design — no ClockTick source is wired in the current host)   │
│                                                                         │
│  ClockTick (from a host-supplied clock)  ──→  MidiOutputActor           │
│                                       │                                 │
│                                 MidiClockGenerator                      │
│                                       │                                 │
│                                 Vec<ControlEvent>                       │
│                                       │                                 │
│                                 serialize_to_midi()                     │
│                                       │                                 │
│                                 MidiMessage(0xF8, …)                    │
│                                       │                                 │
│                                 MidiOutput::send()                      │
│                                       │                                 │
│                                       ▼                                 │
│                                 Hardware / JACK / ALSA                  │
└─────────────────────────────────────────────────────────────────────────┘
```

- **MIDI threads are NOT the signal RT thread** — blocking I/O is allowed
- All communication uses `ActorRef<CommandEnum>` — lock‑free, no `Arc<Mutex>`
- Input: sensors produce `ControlEvent`, dispatched through actor mailbox
- Output: a `ClockTick` source (not yet wired in the modular host) drives the clock generator, which produces events that are serialized and sent through the backend

## `MidiMessage` — raw MIDI bytes

```rust
pub struct MidiMessage(pub [u8; 3]);
```

A lightweight container for three MIDI bytes. Single-byte system messages
(Clock: `0xF8`, Start: `0xFA`, Stop: `0xFC`, Continue: `0xFB`) have data
bytes set to zero. No MIDI semantics — interpretation happens in `parse_midi()`
(input) or `serialize_to_midi()` (output).

## `MidiInput` trait

```rust
pub trait MidiInput: Send + 'static {
    fn poll(&mut self) -> IoResult<Vec<MidiMessage>>;
}
```

Backends implement hardware-specific MIDI input. `poll()` may block briefly
(typically 1–10 ms) waiting for events.

## `MidiOutput` trait

```rust
pub trait MidiOutput: Send + 'static {
    fn send(&mut self, message: &MidiMessage) -> IoResult<()>;
}
```

Sends a single MIDI message to an output port. All three backends deliver
messages immediately — no internal buffering, no `flush()` needed.

The `MidiInput`/`MidiOutput` pair mirrors the audio-side
`IoCapture`/`IoPlayback` separation — input and output are distinct traits,
each backend implements the direction(s) it supports.

### Built-in backends

| Backend | Feature | Platform | `MidiInput` | `MidiOutput` |
|---|---|---|---|---|
| `MidirBackend` | `midir` (default) | All | `new()`, `new_by_port()`, `new_by_name()` | `new_output()`, `new_output_by_name()` |
| `AlsaSeqBackend` | `alsa` | Linux | `new()` — `Direction::Capture` port | `new_output()` — `Direction::Playback` port |
| `JackMidiBackend` | `jack` | All | `new()` + `connect()` — `MidiIn` port | `new_output()` + `connect_output()` — `MidiOut` port |

### Choosing a backend (input)

```rust,no_run
// Cross-platform default — connects to hardware MIDI port
use rill_io::backends::MidirBackend;
use rill_io::midi_input::MidiInput;
let backend: Box<dyn MidiInput> = Box::new(MidirBackend::new("rill-midi").unwrap());
```

### Choosing a backend (output)

```rust,no_run
use rill_io::backends::MidirBackend;
use rill_io::midi_output::MidiOutput;
let backend: Box<dyn MidiOutput> = Box::new(
    MidirBackend::new_output_by_name("rill-clock", "My Synth").unwrap()
);
```

## Input path: `parse_midi()` — bytes → `ControlEvent`

The `parse_midi()` function in `rill-patchbay::midi` converts a raw
[`MidiMessage`] into a [`ControlEvent`]:

| Status byte | `ControlEvent` variant |
|---|---|
| `0x80` Note Off | `MidiNote { on: false, velocity: 0 }` |
| `0x90` Note On (vel > 0) | `MidiNote { on: true, velocity }` |
| `0x90` Note On (vel = 0) | `MidiNote { on: false }` |
| `0xA0` Poly Aftertouch | `MidiNote { on: true, velocity }` |
| `0xB0` Control Change | `MidiControl { controller, value, normalized: value / 127 }` |
| `0xE0` Pitch Bend | `MidiControl { controller: 128, normalized }` |
| `0xF8` Clock | `MidiClock` |
| `0xFA` / `0xFB` / `0xFC` | `MidiTransport { kind: Start/Stop/Continue }` |

## Output path: `serialize_to_midi()` — `ControlEvent` → bytes

The reverse of `parse_midi()`. Converts output-bound `ControlEvent`
variants back to [`MidiMessage`] bytes:

| `ControlEvent` | Status byte | Data1 | Data2 |
|---|---|---|---|
| `MidiClock` | `0xF8` | 0 | 0 |
| `MidiTransport { kind: Start }` | `0xFA` | 0 | 0 |
| `MidiTransport { kind: Stop }` | `0xFC` | 0 | 0 |
| `MidiTransport { kind: Continue }` | `0xFB` | 0 | 0 |
| `MidiNote { note, on: true }` | `0x90` | note | velocity |
| `MidiNote { note, on: false }` | `0x80` | note | 0 |

Only Clock, Transport, and Note events are serialized. Other event types
(`MidiControl`, `Button`, `Knob`, etc.) return `None`.

## `MidiClockGenerator` — 24ppqn clock pulse generator

Lives in `rill-patchbay::midi_clock`. Converts timing information from
[`ClockTick`] into MIDI clock pulses (24 pulses per quarter note = 24ppqn).

```rust
pub struct MidiClockGenerator {
    next_tick_at: f64,     // absolute sample position of next tick
    samples_per_tick: f64, // sample_rate × 60 / (bpm × 24)
    bpm: f64,
    playing: bool,
}
```

**Algorithm:** On each `tick(&ClockTick)` call:
1. If BPM changed, recalculate `samples_per_tick` from `clock.tempo`
2. While `next_tick_at < clock.sample_pos + block_size`: emit
   `ControlEvent::MidiClock`, advance `next_tick_at` by `samples_per_tick`
3. Return accumulated events (0, 1, or several per block)

**Transport state machine:**
- `Start` → sets `playing = true`, resets `next_tick_at` to current sample position
- `Stop` → sets `playing = false`, no ticks produced
- `Continue` → sets `playing = true`, continues from current phase
- `Start` while playing → no-op

Uses **absolute sample position** from `ClockTick` for tick scheduling —
no cumulative drift even at non-integer sample-per-tick ratios.

## `spawn_midi_clock_output()` — output actor

Combines `MidiClockGenerator` + `MidiOutput` into a single actor:

```rust,no_run
use rill_core_actor::ActorSystem;
use rill_io::backends::MidirBackend;
use rill_io::midi_output::MidiOutput;
use rill_patchbay::midi_clock::spawn_midi_clock_output;

let system = ActorSystem::new();
let backend: Box<dyn MidiOutput> = Box::new(
    MidirBackend::new_output_by_name("rill-clock", "My Synth").unwrap()
);
let clock_ref = spawn_midi_clock_output(&system, backend);
```

The actor receives:
- `CommandEnum::ClockTick` — **from whatever clock the host supplies**. No
  source is wired by the modular host today (its rack actor broadcasts nothing),
  so a clock module only emits after construction-time transport control; feeding
  it ticks is the reserved control-plane work.
- `CommandEnum::Control(MidiTransport { .. })` — for transport control
  from API or user code

```rust,no_run
use rill_core::queues::CommandEnum;
use rill_core::queues::control_event::{ControlEvent, MidiTransportKind};

// Start clock
clock_ref.send(CommandEnum::Control(ControlEvent::MidiTransport {
    kind: MidiTransportKind::Start,
}));

// Stop clock
clock_ref.send(CommandEnum::Control(ControlEvent::MidiTransport {
    kind: MidiTransportKind::Stop,
}));
```

## `MidiClockTracker` — input-side BPM derivation

The input-side counterpart of `MidiClockGenerator`. Counts incoming 24ppqn
clock pulses (`0xF8`), derives BPM from pulse intervals via running average,
and writes atomically into a shared [`SystemClock`]. Integrated into `MidiHub`
via `MidiHub::with_clock_tracker()`.

Three pluggable [`MidiClockStrategy`] implementations:
- `FreeRunning` — BPM only, ignores transport
- `ResetOnStart` — resets clock position on Start
- `SongPosition` — position reset + `is_playing()` flag

## EventPattern matching

Both input and output use the same [`EventPattern`] matching infrastructure:

```rust
pub enum EventPattern {
    // ... existing ...
    AnyMidi,
    MidiControl { channel: Option<u8>, controller: u8 },
    MidiNote { channel: Option<u8>, note: Option<u8>, kind: MidiNoteKind },
    MidiClock,
    MidiTransport { kind: Option<MidiTransportKind> },
}

pub enum MidiTransportKind { Start, Stop, Continue }

pub enum MidiNoteKind { Frequency, Amplitude, Gate }

pub enum ControlEvent {
    // ... existing ...
    MidiControl { channel, controller, value, normalized },
    MidiNote { channel, note, velocity, on },
    MidiClock,
    MidiTransport { kind: MidiTransportKind },
}
```

- `EventPattern::AnyMidi` matches all four MIDI event types
- `EventPattern::MidiTransport { kind: None }` matches any transport event

## Declarative config: `ClockDef` + `SensorDef::Midi`

MIDI input and clock output can be declared in `ModularSystemDef` JSON
documents without writing Rust code. The documents are accepted and the modules
are constructed; the current host does not yet drive them (see the status note
at the top of this page).

### SensorDef::Midi (input)

```json
{
  "type": "Sensor",
  "Midi": {
    "backend": "midir",
    "port_name": "rill-midi-synth",
    "mappings": [
      {
        "event_pattern": { "MidiControl": { "channel": null, "controller": 7 } },
        "target_node": 1,
        "target_param": "volume",
        "transform": "Linear",
        "min": 0.0,
        "max": 1.0,
        "enabled": true
      }
    ]
  }
}
```

### ClockDef (output)

```json
{
  "type": "Clock",
  "backend": "midir",
  "port_name": "rill-clock",
  "auto_start": true
}
```

`auto_start` — when `true`, sends `MidiTransport::Start` to the clock output at
construction. No clock ticks reach the clock output afterwards: the modular host
wires no clock source, so the control-module plane is currently inert (see
[Patchbay & rack architecture](patchbay-rack.md)).

Both variants use the existing `ModuleFactory` infrastructure:
`MidiConstructor` (registered as `"midi"`) and `ClockConstructor`
(registered as `"clock"`).

## Programmatic API summary

### Input path

```rust,no_run
use rill_core_actor::ActorSystem;
use rill_io::midi_input::MidiInput;
use rill_io::backends::MidirBackend;
use rill_patchbay::midi::spawn_midi_sensor;

// Create sensor: backend → polling thread → servo → graph
let backend: Box<dyn MidiInput> = Box::new(MidirBackend::new("rill-midi").unwrap());
let sensor_ref = spawn_midi_sensor("my_midi", backend, &system, servo_ref);
```

### Output path

```rust,no_run
use rill_io::midi_output::MidiOutput;
use rill_patchbay::midi_clock::spawn_midi_clock_output;

let backend: Box<dyn MidiOutput> = Box::new(
    MidirBackend::new_output_by_name("rill-clock", "My Synth").unwrap()
);
let clock_ref = spawn_midi_clock_output(&system, backend);

// Transport control
use rill_core::queues::control_event::{ControlEvent, MidiTransportKind};
clock_ref.send(CommandEnum::Control(ControlEvent::MidiTransport {
    kind: MidiTransportKind::Start,
}));
```

### Output path (declarative)

```rust,no_run
use rill_adrift::modular::{ModularSystem, ModularConfig};
use rill_adrift::modular::serialization::{
    ModularSystemDef, RackDef, ModuleDef,
};
use rill_patchbay::module_def::ClockDef;

let mut rack = RackDef::new(
    "main",
    "main = do { write (Just \"hi\") stdout; return () };",
);
rack.modules = vec![ModuleDef::Clock(ClockDef {
    backend: "midir".into(),
    port_name: "rill-clock".into(),
    auto_start: true,
})];

let def = ModularSystemDef {
    format_version: "rill/1".into(),
    sample_rate: 48000.0,
    block_size: 256,
    racks: vec![rack],
    description: None,
};

let mut system = ModularSystem::<256>::new(ModularConfig::default());
system.launch(&def).unwrap();
// The clock module is constructed, but no clock ticks are broadcast: the
// control-module host is currently inert.
```

## Feature flags

| Feature | Crate | Enables |
|---------|-------|---------|
| `midir` (default) | `rill-io` | `MidirBackend` — cross‑platform MIDI input + output |
| `alsa` | `rill-io` | `AlsaSeqBackend` — ALSA sequencer input + output |
| `jack` | `rill-io` | `JackMidiBackend` — JACK MIDI input + output |
| `midi` | `rill-patchbay` | `MidiHub`, `MidiClockTracker`, `MidiClockGenerator`, `spawn_midi_sensor()`, `spawn_midi_clock_output()`, `serialize_to_midi()` — pulls `rill-io` dependency |
| `midi` | `rill-adrift` | `MidiConstructor`, `ClockConstructor` — forward to `rill-patchbay/midi` |

## Sensor trait

The [`Sensor`] trait provides a unified interface for external input sources.
All sensors send `ControlEvent` to a shared `ActorRef`, drained by
`Patchbay::drain_events()`.

```rust
pub trait Sensor: Send + 'static {
    fn attach(&mut self, events: ActorRef<ControlEvent>);
    fn start(&mut self);
    fn stop(&mut self);
}
```

`MidiHub` implements `Sensor`. OSC sensors (`OscSensor`, `spawn_osc_sensor`),
hardware knobs, and acoustic analysis via [`Hearing`] follow the same pattern —
multiple sensors feed one event mailbox with no locking.

**MIDI output is NOT a `Sensor`** — it does not produce `ControlEvent` into
the system. Instead, it consumes `ClockTick` (supplied by the host; the modular
host wires no clock source yet) and sends `ControlEvent` out to hardware. The
`MidiOutputActor` is an output endpoint, not a sensor.

## Hearing — signal analysis for acoustic sensors

The [`hearing`] module provides signal analysis algorithms for acoustic
sensors that react to graph signal output:

| Algorithm | What it detects |
|---|---|
| `PitchDetector` | Pitch via autocorrelation |
| `EnvelopeFollower` | Amplitude envelope with attack/release |
| `ZeroCrossing` | Frequency via zero-crossing rate |

Each implements `Hearing: process(&mut self, audio: &[f32]) -> f32`.
An `AcousticSensor` (future) wraps a `Hearing` implementation, subscribes
to graph telemetry, and produces `ControlEvent`s from signal features.

## Commands

```bash
# Build with MIDIR support (input + output)
cargo check -p rill-io --features "midir"

# Build with ALSA sequencer support (input + output)
cargo check -p rill-io --features "alsa"

# Build patchbay with full MIDI (input + output + clock tracker + clock generator)
cargo check -p rill-patchbay --features midi

# Build drift with MIDI (all features)
cargo check -p drift --all-features
```
