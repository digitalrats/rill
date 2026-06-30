# MIDI Output Design — MIDI Clock & Transport

**Date:** 2026-06-30
**Status:** Approved
**Scope:** rill workspace — `rill-io`, `rill-core`, `rill-patchbay`, `rill-adrift`

---

## Problem

rill currently supports only MIDI input. There is no infrastructure to send MIDI messages to external
hardware or software. The primary use case is **rill as MIDI master** — generating MIDI Clock (24ppqn)
and Transport (Start/Stop/Continue) to synchronize external devices. MIDI Note output is deferred to
a future phase.

---

## Architecture Overview

The design mirrors the existing MIDI input architecture with the actor model:

```
RT Signal Thread                        Non-RT Output Actor Thread
─────────────                          ─────────────────────────────
                                        ┌──────────────────────────┐
Rack broadcast                          │  MidiOutputActor         │
  ClockTick ──────────────────────────→ │                          │
                                        │  MidiClockGenerator      │
                                        │       ↓                  │
                                        │  Vec<ControlEvent>       │
                                        │       ↓                  │
                                        │  serialize_to_midi()     │
                                        │       ↓                  │
                                        │  MidiOutput::send()      │
                                        └───────────┬──────────────┘
                                                    │
                                          MIDI Hardware / JACK / ALSA
```

Key principles:
- **`MidiInput`** (renamed from `MidiBackend`) — input-only: `poll() -> Vec<MidiMessage>`
- **`MidiOutput`** — output-only: `send(&mut self, &MidiMessage) -> IoResult<()>`
- **`MidiClockGenerator`** — pure math: `ClockTick → Vec<ControlEvent>`, no I/O, fully testable
- **`MidiOutputActor`** — owns generator + backend, drains actor mailbox, dispatches events
- **`ClockDef`** — declarative config for serialization systems

---

## Components

### 1. `MidiInput` trait (rename of `MidiBackend`)

**File:** `rill-io/src/midi_input.rs` (renamed from `midi_backend.rs`)

```rust
pub trait MidiInput: Send + 'static {
    fn poll(&mut self) -> IoResult<Vec<MidiMessage>>;
}
```

All references to `MidiBackend` renamed to `MidiInput` across the workspace. All three backends
(`MidirBackend`, `AlsaSeqBackend`, `JackMidiBackend`) keep their existing `MidiInput`
implementation unchanged.

### 2. `MidiOutput` trait (new)

**File:** `rill-io/src/midi_output.rs`

```rust
pub trait MidiOutput: Send + 'static {
    fn send(&mut self, message: &MidiMessage) -> IoResult<()>;
}
```

One message per call. No `flush()` — all three backends send messages immediately.

#### 2a. `MidirBackend` — output constructors

```rust
impl MidirBackend {
    pub fn new_output(port_name: &str) -> Option<Self>
    pub fn new_output_by_port(index: usize) -> Option<Self>
    pub fn new_output_by_name(substring: &str) -> Option<Self>
}
```

Uses `midir::MidiOutput::connect()` to open a write port.

#### 2b. `AlsaSeqBackend` — output constructors

```rust
impl AlsaSeqBackend {
    pub fn new_output(port_name: &str) -> IoResult<Self>
}
```

Creates an ALSA sequencer port with `PortCap::WRITE | PortCap::SUBS_WRITE`, `PortType::MIDI_GENERIC | PortType::APPLICATION`.

#### 2c. `JackMidiBackend` — output constructors

```rust
impl JackMidiBackend {
    pub fn new_output(client_name: &str) -> IoResult<Self>
}
```

Registers a `MidiOut` JACK port, similar to the existing input implementation but reversed direction.

### 3. `MidiClockGenerator` (new)

**File:** `rill-patchbay/src/midi_clock.rs` (added to existing file, alongside `MidiClockTracker`)

```rust
pub struct MidiClockGenerator {
    next_tick_at: f64,     // absolute sample position of the next MIDI clock tick
    samples_per_tick: f64, // sample_rate * 60.0 / (bpm * 24.0)
    bpm: f64,
    playing: bool,
}

impl MidiClockGenerator {
    pub fn new() -> Self

    /// Process one signal block. Returns MIDI clock events for ticks
    /// that fall within [clock.sample_pos, clock.sample_pos + block_size).
    /// Returns empty vec if !playing.
    pub fn tick(&mut self, clock: &ClockTick) -> Vec<ControlEvent>
}
```

**Algorithm:**
1. If `clock.tempo` changed (or not yet set), recalculate `samples_per_tick`
2. If not `playing`, return empty
3. While `next_tick_at < clock.sample_pos + block_size`:
   - Push `ControlEvent::MidiClock`
   - `next_tick_at += samples_per_tick`
4. Return accumulated events

**Transport state machine:**

```
   Start
┌──────────────┐
│              ▼
│   ┌──────────────────┐
│   │    PLAYING        │
│   │  clock ticks on   │
│   └───┬──────────┬───┘
│       │ Stop     │ Continue
│       ▼          │
│   ┌──────────┐   │
│   │ STOPPED   │───┘
│   │ no ticks  │
│   └─────┬────┘
│         │ Start
│         ▼
│   [reset next_tick_at to current sample_pos, go to PLAYING]
│
└── Start from PLAYING is a no-op
```

- `Start` from `STOPPED` resets `next_tick_at` to the block start sample position
- `Start` from `PLAYING` is a no-op (already running)
- `Continue` from `STOPPED` resumes without resetting position
- `Stop` from `PLAYING` sets `playing = false`, stops tick generation

### 4. `MidiOutputActor` (new)

**File:** `rill-patchbay/src/midi_clock.rs`

```rust
pub fn spawn_midi_clock_output(
    system: &ActorSystem,
    output: Box<dyn MidiOutput>,
) -> ActorRef<CommandEnum>
```

The actor:
- Owns `Box<dyn MidiOutput>` and `MidiClockGenerator`
- Receives `CommandEnum::ClockTick(ClockTick)` via Rack broadcast — no explicit wiring in signal thread
- Receives `CommandEnum::Control(ControlEvent::MidiTransport { kind })` for transport commands
- For each `ClockTick`: runs generator, serializes each resulting `ControlEvent::MidiClock` to `MidiMessage(0xF8, 0, 0)`, calls `MidiOutput::send()`
- For each `MidiTransport`: updates generator state, serializes to the appropriate MIDI message (`0xFA`/`0xFB`/`0xFC`), calls `MidiOutput::send()`
- Future: `ControlEvent::MidiNote` → Note On/Off messages

### 5. Serialization — `serialize_to_midi()` (new)

**File:** `rill-patchbay/src/midi.rs` (added)

Reverse of `parse_midi()`. Converts `ControlEvent` to `MidiMessage` bytes:

| `ControlEvent` | Status byte | Data1 | Data2 |
|---|---|---|---|
| `MidiClock` | `0xF8` | 0 | 0 |
| `MidiTransport { kind: Start }` | `0xFA` | 0 | 0 |
| `MidiTransport { kind: Stop }` | `0xFC` | 0 | 0 |
| `MidiTransport { kind: Continue }` | `0xFB` | 0 | 0 |
| `MidiNote { note, on: true }` | `0x90` | note | velocity |
| `MidiNote { note, on: false }` | `0x80` | note | 0 |

### 6. `ModuleDef::Clock` variant (new)

**File:** `rill-patchbay/src/module_def.rs`

```rust
pub enum ModuleDef {
    Servo(ServoDef),
    Sensor(SensorDef),
    Clock(ClockDef),
    Custom { type_name: String, params: BTreeMap<String, Value> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockDef {
    pub backend: String,       // "midir" | "alsa_seq" | "jack"
    pub port_name: String,
    #[serde(default)]
    pub auto_start: bool,
}
```

### 7. `ClockConstructor` (new)

**File:** `rill-adrift/src/registration.rs`

Registered as `"clock"` in `ModuleFactory`:

```
factory.register("clock", ClockConstructor);
```

`ClockConstructor::construct()`:
1. Creates `MidiOutput` backend by matching `backend` string (same pattern as `MidiConstructor`)
2. Calls `spawn_midi_clock_output(system, output)`
3. If `auto_start`, sends `MidiTransport { kind: Start }` to the actor
4. Returns `ActorRef<CommandEnum>`

---

## Example Usage

### Programmatic API

```rust
use rill_patchbay::midi_clock::spawn_midi_clock_output;
use rill_io::backends::midir_backend::MidirBackend;

let backend = MidirBackend::new_output_by_name("My Synth").unwrap();
let clock_ref = spawn_midi_clock_output(&system, Box::new(backend));

system_clock.set_bpm(138.0);
clock_ref.send(CommandEnum::Control(ControlEvent::MidiTransport {
    kind: MidiTransportKind::Start,
}));
// ... clock ticks flow automatically via Rack broadcast ...

clock_ref.send(CommandEnum::Control(ControlEvent::MidiTransport {
    kind: MidiTransportKind::Stop,
}));
```

### Declarative JSON

```json
{
  "format_version": "rill/1",
  "sample_rate": 48000.0,
  "block_size": 256,
  "racks": [{
    "name": "main",
    "graph": { ... },
    "modules": [{
      "type": "Clock",
      "backend": "midir",
      "port_name": "rill-clock",
      "auto_start": true
    }]
  }]
}
```

---

## Renames (breaking)

| Old | New | Reason |
|---|---|---|
| `MidiBackend` trait | `MidiInput` | Reflects input-only role |
| `midi_backend.rs` | `midi_input.rs` | Consistency |
| Doc references to `MidiBackend` | `MidiInput` | Accuracy |

---

## Non-goals (deferred)

- MIDI Note output (deferred to future phase)
- Song Position Pointer (SPP) output
- MIDI Thru (input → output pass-through)
- MIDI clock slave sync improvements (existing `MidiClockTracker` remains for input)

---

## Testing Strategy

- `MidiClockGenerator::tick()` — unit tests with synthetic `ClockTick` values at various BPM/sample rates
- `serialize_to_midi()` — round-trip test: `parse_midi(serialize_to_midi(event)) == event`
- `MidiOutput` backends — integration test with virtual MIDI ports (ALSA seq or platform equivalent)
