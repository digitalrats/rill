# MIDI support (rill-io + rill-patchbay)

MIDI input is handled through a layered architecture:
raw hardware I/O → `MidiBackend` → `MidiHub` → `ControlEvent` → `Patchbay` → `SetParameter` → Graph.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  DEDICATED MIDI THREAD (non‑RT)                                   │
│                                                                    │
│  MidiBackend::poll()  ──→  MidiHub  ──→  Patchbay::handle_event()
│       │                        │                    │              │
│  raw [u8; 3]             parse bytes         mappings             │
│                           into               MIDI CC → param      │
│                           ControlEvent       Note → frequency     │
└──────────────────────────────────────────────────────────────────┘
                                                       │
                                              ActorRef<SetParameter>
                                                       │
                                                       ▼
                                              Graph command queue
```

- **MIDI thread is NOT the audio RT thread** — blocking I/O is allowed
- All MIDI → parameter mapping happens through `Patchbay`'s existing event infrastructure
- The actor owns a `Box<dyn MidiBackend>` and polls at 1 ms intervals

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

Lives in `rill-patchby` behind the `midi` feature gate.
Spawns a dedicated thread, polls the backend, and dispatches parsed events
to a shared `Patchbay`.

```rust,no_run
use std::sync::{Arc, Mutex};
use rill_patchbay::midi::MidiHub;
use rill_patchbay::Patchbay;
use rill_io::midi_backend::MidiBackend;

let patchbay = Arc::new(Mutex::new(Patchbay::new(actor_ref)));
let backend: Box<dyn MidiBackend> = /* ... */;

let mut actor = MidiHub::start(backend, patchbay.clone());
// ... run ...
actor.stop();
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
use std::sync::{Arc, Mutex};
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

// 3. Start MIDI actor
let patchbay = Arc::new(Mutex::new(patchbay));
let backend = Box::new(MidirBackend::new("rill-midi").unwrap());
let mut actor = MidiHub::start(backend, patchbay.clone());

// 4. Run graph on audio thread, drain commands
// ... graph.run(running) ...

// 5. Stop
actor.stop();
```

## Feature flags

| Feature | Crate | Enables |
|---------|-------|---------|
| `midir` (default) | `rill-io` | `MidirBackend` — cross‑platform MIDI input |
| `alsa` | `rill-io` | `AlsaSeqBackend` — ALSA sequencer virtual port |
| `midi` | `rill-patchbay` | `MidiHub` — pulls `rill-io` dependency |

`rill-adrift` enables `midi` in `default` features when MIDI is available on the target platform.

## Commands

```bash
# Build with MIDI support
cargo check -p rill-io --features "midir,alsa"

# Build patchbay with MidiHub
cargo check -p rill-patchbay --features midi

# Build drift with MIDI (all features)
cargo check -p drift --all-features
```
