# MIDI support (rill-io + rill-patchbay)

MIDI input is handled through a layered architecture:
raw hardware I/O → `MidiBackend` → `MidiHub` → `ActorRef<ControlEvent>` → `Patchbay::event_mailbox` → `drain_events()` → `handle_event()` → `SetParameter` → Graph.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  DEDICATED MIDI THREAD (non‑RT)                                   │
│                                                                    │
│  MidiBackend::poll()  ──→  MidiHub  ──→  ActorRef<ControlEvent>  │
│       │                        │                    │              │
│  raw [u8; 3]             parse bytes        events.send(event)    │
│                           into                                    │
│                           ControlEvent                            │
└──────────────────────────────────────────────────────────────────┘
                                                     │
                                        Patchbay::event_mailbox
                                                     │
                                               drain_events()
                                                     │
                                               handle_event()
                                                     │
                                               Mapping → SetParameter
                                                     │
                                               ActorRef<SetParameter>
                                                     │
                                                     ▼
                                             Graph command queue
```

- **MIDI thread is NOT the signal RT thread** — blocking I/O is allowed
- All MIDI → parameter mapping happens through `Patchbay`'s existing event infrastructure
- The sensor sends `ControlEvent` via `ActorRef` — lock‑free, no `Arc<Mutex>`
- Multiple sensors (MIDI, OSC, future) share one event mailbox

## `MidiMessage` — raw MIDI bytes

```rust
pub struct MidiMessage(pub [u8; 3]);
```

A lightweight container for three MIDI bytes. Single-byte system messages
(Clock: `0xF8`, Start: `0xFA`, Stop: `0xFC`, Continue: `0xFB`) have data
bytes set to zero. No MIDI semantics — interpretation happens in `MidiHub`.

## `MidiBackend` trait

```rust
pub trait MidiBackend: Send + 'static {
    fn poll(&mut self) -> IoResult<Vec<MidiMessage>>;
}
```

Backends implement hardware-specific MIDI input. `poll()` may block briefly
(typically 1–10 ms) waiting for events.

### Built-in backends

| Backend | Feature | Platform | Notes |
|---------|---------|----------|-------|
| `MidirBackend` | `midir` (default) | All | Cross‑platform via `midir` crate. Connects to first available MIDI input port. |
| `AlsaSeqBackend` | `alsa` | Linux | Creates a dedicated ALSA sequencer port (`"rill‑midi"`). Other applications can connect to it via `aconnect` or patchbays. |

### Choosing a backend

```rust,no_run
// Cross-platform default — connects to hardware MIDI port
use rill_io::backends::MidirBackend;
let backend: Box<dyn MidiBackend> = Box::new(MidirBackend::new("rill-midi").unwrap());

// Linux — dedicated virtual port for patching
#[cfg(feature = "alsa")]
use rill_io::backends::AlsaSeqBackend;
#[cfg(feature = "alsa")]
let backend: Box<dyn MidiBackend> = Box::new(AlsaSeqBackend::new("rill-midi").unwrap());
```

## `MidiHub` — byte parser + dispatcher

Lives in `rill-patchbay` behind the `midi` feature gate. Implements the
[`Sensor`] trait. Spawns a dedicated OS thread, polls the backend, parses
bytes into `ControlEvent`, and sends via `ActorRef` — no locking required.

```rust,no_run
use rill_core_actor::ActorRef;
use rill_patchbay::midi::MidiHub;
use rill_patchbay::engine::ControlEvent;
use rill_io::midi_backend::MidiBackend;

let events: ActorRef<ControlEvent> = /* from Patchbay::event_handle() */;
let backend: Box<dyn MidiBackend> = /* ... */;

let mut hub = MidiHub::start(backend, events);
// ... run ...
hub.stop();
```

### MIDI → ControlEvent translation

The actor parses raw bytes:

| Status byte | ControlEvent variant |
|---|---|
| `0x80` Note Off | `ControlEvent::MidiNote { on: false, velocity: 0 }` |
| `0x90` Note On (vel > 0) | `ControlEvent::MidiNote { on: true, velocity }` |
| `0x90` Note On (vel = 0) | `ControlEvent::MidiNote { on: false }` |
| `0xB0` Control Change | `ControlEvent::MidiControl { controller, normalized: value / 127.0 }` |
| `0xE0` Pitch Bend | `ControlEvent::MidiControl { controller: 128, normalized }` |
| `0xF8` Clock | `ControlEvent::MidiClock` |
| `0xFA` / `0xFB` / `0xFC` | `ControlEvent::MidiTransport { kind: Start/Stop/Continue }` |

## EventPattern matching

Added to `rill-patchbay::engine`:

```rust
pub enum EventPattern {
    // ... existing ...
    MidiClock,
    MidiTransport { kind: Option<MidiTransportKind> },
}

pub enum MidiTransportKind { Start, Stop, Continue }

pub enum ControlEvent {
    // ... existing ...
    MidiClock,
    MidiTransport { kind: MidiTransportKind },
}
```

- `EventPattern::AnyMidi` matches all four MIDI event types
- `EventPattern::MidiTransport { kind: None }` matches Start/Stop/Continue

## Wiring: MidiHub → Patchbay → Graph

```rust,no_run
use rill_core_actor::ActorRef;
use rill_patchbay::{
    midi::MidiHub,
    engine::{Patchbay, EventPattern, midi_cc},
    Transform,
};
use rill_io::backends::MidirBackend;

// 1. Create patchbay with graph's command queue
let (graph_ref, graph_mbox) = ActorRef::new_pair();
let mut patchbay = Patchbay::new(graph_ref);

// 2. Add MIDI mappings
patchbay.add_midi_mapping(
    7,          // controller number (CC#7 = volume)
    None,       // any channel
    NodeId(1),  // target node
    "volume",   // target parameter
    0.0, 1.0,   // value range
    Transform::Linear,
);

// 3. Create and attach MIDI sensor
let backend = Box::new(MidirBackend::new("rill-midi").unwrap());
let mut hub = MidiHub::new(backend);
hub.attach(patchbay.event_handle());
hub.start();
patchbay.add_sensor(Box::new(hub));

// 4. Run graph on signal thread
// ... graph.run(running) ...

// 5. Drain clock & events in control loop
// loop {
//     patchbay.drain_clock();
//     std::thread::sleep(Duration::from_millis(10));
// }

// 6. Stop — calls stop_all() which stops all sensors
patchbay.stop_all();
```

## Feature flags

| Feature | Crate | Enables |
|---------|-------|---------|
| `midir` (default) | `rill-io` | `MidirBackend` — cross‑platform MIDI input |
| `alsa` | `rill-io` | `AlsaSeqBackend` — ALSA sequencer virtual port |
| `midi` | `rill-patchbay` | `MidiHub` + `Sensor` trait — pulls `rill-io` dependency |

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
hardware knobs, and acoustic analysis via [`Hearing`] follow the same pattern — multiple sensors feed
one event mailbox with no locking.

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
# Build with MIDI support
cargo check -p rill-io --features "midir,alsa"

# Build patchbay with MidiHub
cargo check -p rill-patchbay --features midi

# Build drift with MIDI (all features)
cargo check -p drift --all-features
```
