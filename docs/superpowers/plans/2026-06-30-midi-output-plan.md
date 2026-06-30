# MIDI Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement MIDI Clock and Transport output for rill as MIDI master, with declarative serialization support.

**Architecture:** New `MidiOutput` trait in `rill-io` (mirrors `MidiInput`), `MidiClockGenerator` in `rill-patchbay` (pure math: `ClockTick` → `Vec<ControlEvent>`), `spawn_midi_clock_output()` actor that owns generator + backend, `ClockDef` for declarative config. Rename `MidiBackend` → `MidiInput`.

**Tech Stack:** `midir` v0.11, `alsa` v0.9, `jack` v0.10 (existing deps, no new crates).

---

### Task 1: Rename `MidiBackend` → `MidiInput`

**Files:**
- Delete: `rill-io/src/midi_backend.rs`
- Create: `rill-io/src/midi_input.rs`
- Modify: `rill-io/src/lib.rs:41-54`
- Modify: `rill-io/src/backends/midir_backend.rs:11,166`
- Modify: `rill-io/src/backends/alsa_seq.rs:11,58`
- Modify: `rill-io/src/backends/jack_midi.rs:15,89`
- Modify: `rill-patchbay/src/midi.rs:17,34,41,59,75,170`
- Modify: `rill-patchbay/src/module_def.rs:243-244`
- Modify: `rill-adrift/src/registration.rs:445,476`

- [ ] **Step 1: Create `rill-io/src/midi_input.rs`**

```bash
cp rill-io/src/midi_backend.rs rill-io/src/midi_input.rs
```

- [ ] **Step 2: Edit `rill-io/src/midi_input.rs`**

Replace all occurrences of `MidiBackend` with `MidiInput` in the file. Change the doc comment on line 1 to say "MIDI input trait". Change line 25: `pub trait MidiInput: Send + 'static {`.

- [ ] **Step 3: Edit `rill-io/src/lib.rs`**

Change line 42: `pub mod midi_backend;` → `pub mod midi_input;`
Change line 53: `pub use midi_backend::MidiBackend;` → `pub use midi_input::MidiInput;`

- [ ] **Step 4: Edit `rill-io/src/backends/midir_backend.rs`**

Line 11: `use crate::midi_backend::MidiBackend;` → `use crate::midi_input::MidiInput;`
Line 166: `impl MidiBackend for MidirBackend {` → `impl MidiInput for MidirBackend {`
Lines 4,22-23: update doc comments referencing `MidiBackend` → `MidiInput`

- [ ] **Step 5: Edit `rill-io/src/backends/alsa_seq.rs`**

Line 11: `use crate::midi_backend::MidiBackend;` → `use crate::midi_input::MidiInput;`
Line 58: `impl MidiBackend for AlsaSeqBackend {` → `impl MidiInput for AlsaSeqBackend {`
Lines 22-23: update doc comments

- [ ] **Step 6: Edit `rill-io/src/backends/jack_midi.rs`**

Line 15: `use crate::midi_backend::MidiBackend;` → `use crate::midi_input::MidiInput;`
Line 89: `impl MidiBackend for JackMidiBackend {` → `impl MidiInput for JackMidiBackend {`

- [ ] **Step 7: Edit `rill-patchbay/src/midi.rs`**

Line 17: `use rill_io::midi_backend::MidiBackend;` → `use rill_io::midi_input::MidiInput;`
Replace all `Box<dyn MidiBackend>` with `Box<dyn MidiInput>` (lines 34, 41, 59, 75, 170). Update doc comments.

- [ ] **Step 8: Edit `rill-patchbay/src/module_def.rs`**

Line 243: `use rill_io::midi_backend::MidiBackend;` → `use rill_io::midi_input::MidiInput;`
Line 244: `Box<dyn MidiBackend>` → `Box<dyn MidiInput>`

- [ ] **Step 9: Edit `rill-adrift/src/registration.rs`**

Line 445: `use rill_io::midi_backend::MidiBackend;` → `use rill_io::midi_input::MidiInput;`
Line 476: `Box<dyn MidiBackend>` → `Box<dyn MidiInput>`

- [ ] **Step 10: Remove old file and verify**

```bash
rm rill-io/src/midi_backend.rs
cargo build -p rill-io --features midir,alsa,jack 2>&1
cargo build -p rill-patchbay --features midi,alsa 2>&1
cargo build -p rill-adrift --features midi,alsa 2>&1
```

Expected: all three compile cleanly.

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m 'refactor(rill-io): rename MidiBackend to MidiInput'
```

---

### Task 2: Create `MidiOutput` trait

**Files:** Create `rill-io/src/midi_output.rs`, modify `rill-io/src/lib.rs`

- [ ] **Step 1: Write `rill-io/src/midi_output.rs`**

```rust
//! MIDI output trait for sending raw MIDI messages to hardware or
//! virtual devices.
//!
//! Implementations:
//! - `MidirBackend` (behind `midir` feature)
//! - `AlsaSeqBackend` (behind `alsa` feature)
//! - `JackMidiBackend` (behind `jack` feature)

use crate::error::IoResult;
use crate::midi_message::MidiMessage;

/// Generic MIDI output backend.
///
/// Sends one message at a time — all current backends
/// (midir, ALSA seq, JACK) deliver messages immediately
/// without internal buffering, so no `flush()` is needed.
pub trait MidiOutput: Send + 'static {
    /// Send a single MIDI message to the output port.
    fn send(&mut self, message: &MidiMessage) -> IoResult<()>;
}
```

- [ ] **Step 2: Edit `rill-io/src/lib.rs`**

After `pub mod midi_input;`, add `pub mod midi_output;`.
After `pub use midi_message::MidiMessage;`, add `pub use midi_output::MidiOutput;`.

- [ ] **Step 3: Verify**

```bash
cargo build -p rill-io 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add rill-io/src/midi_output.rs rill-io/src/lib.rs
git commit -m 'feat(rill-io): add MidiOutput trait'
```

---

### Task 3: Add `MidiOutput` impl to `MidirBackend`

**File:** `rill-io/src/backends/midir_backend.rs`

- [ ] **Step 1: Change struct at line 28**

Replace:
```rust
pub struct MidirBackend {
    rx: Receiver<MidiMessage>,
    _conn: midir::MidiInputConnection<()>,
}
```

With:
```rust
enum MidirConnection {
    Input(midir::MidiInputConnection<()>),
    Output(midir::MidiOutputConnection),
}

pub struct MidirBackend {
    rx: Receiver<MidiMessage>,
    _conn: MidirConnection,
}
```

- [ ] **Step 2: Fix `connect()` to use `MidirConnection::Input(conn)`**

In the `connect()` function, change the last line `Ok(Self { rx, _conn: conn })` to:
```rust
Ok(Self { rx, _conn: MidirConnection::Input(conn) })
```

- [ ] **Step 3: Add output constructors** — insert before the `impl MidiBackend` block (before line 166):

```rust
    /// Create a new MIDI output, connect to a port by substring match.
    pub fn new_output_by_name(name: &str, port_name: &str) -> IoResult<Self> {
        Self::connect_output(name, |midi_out, ports| {
            for (i, p) in ports.iter().enumerate() {
                let pname = midi_out.port_name(p).unwrap_or_else(|_| "?".into());
                if pname.contains(port_name) {
                    return Ok((i, pname));
                }
            }
            Err(IoError::DeviceNotFound(format!(
                "no MIDI output port matching '{}' ({} total)",
                port_name, ports.len()
            )))
        })
    }

    /// Create a new MIDI output, connect to the first available port.
    pub fn new_output(name: &str) -> IoResult<Self> {
        Self::connect_output(name, |midi_out, ports| {
            if ports.is_empty() {
                return Err(IoError::DeviceNotFound("no MIDI output ports".into()));
            }
            let pname = midi_out.port_name(&ports[0]).unwrap_or_else(|_| "?".into());
            Ok((0, pname))
        })
    }

    fn connect_output(
        name: &str,
        find: impl FnOnce(&midir::MidiOutput, &[midir::MidiOutputPort]) -> IoResult<(usize, String)>,
    ) -> IoResult<Self> {
        let midi_out = midir::MidiOutput::new(name)
            .map_err(|e| IoError::Init(format!("midir out: {e}")))?;
        let ports = midi_out.ports();
        let (port_idx, port_name) = find(&midi_out, &ports)?;
        let port = &ports[port_idx];
        let conn = midi_out.connect(port, "rill-midi-out")
            .map_err(|e| IoError::Init(format!("midir out connect: {e}")))?;
        log::info!("midir out: connected to port #{port_idx} '{port_name}'");
        let (_tx, rx) = std::sync::mpsc::channel::<MidiMessage>();
        Ok(Self { rx, _conn: MidirConnection::Output(conn) })
    }
```

- [ ] **Step 4: Add `MidiOutput` impl** — after the `MidiInput` impl block:

```rust
use crate::midi_output::MidiOutput;

impl MidiOutput for MidirBackend {
    fn send(&mut self, message: &MidiMessage) -> IoResult<()> {
        match &mut self._conn {
            MidirConnection::Output(conn) => {
                conn.send(message.as_bytes())
                    .map_err(|e| IoError::Midi(format!("midir send: {e}")))?;
                Ok(())
            }
            MidirConnection::Input(_) => Err(IoError::Midi(
                "backend opened as input, not output".into(),
            )),
        }
    }
}
```

Add the `use crate::midi_output::MidiOutput;` import after the existing `use crate::midi_input::MidiInput;` line.

- [ ] **Step 5: Verify**

```bash
cargo build -p rill-io --features midir 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add rill-io/src/backends/midir_backend.rs
git commit -m 'feat(rill-io): add MidiOutput impl to MidirBackend'
```

---

### Task 4: Add `MidiOutput` impl to `AlsaSeqBackend`

**File:** `rill-io/src/backends/alsa_seq.rs`

- [ ] **Step 1: Add import** — after line 11 (`use crate::midi_input::MidiInput;`):

```rust
use crate::midi_output::MidiOutput;
```

- [ ] **Step 2: Add `new_output()` constructor** — after line 55 (`}` of existing `new()`):

```rust
    /// Create a new MIDI output port on the ALSA sequencer.
    pub fn new_output(name: &str) -> IoResult<Self> {
        let cname = CString::new(name)
            .map_err(|_| IoError::Init(format!("name contains nul byte: {name}")))?;
        use alsa::Direction;
        let seq = seq::Seq::open(None, Some(Direction::Playback), true)
            .map_err(|e| IoError::Init(format!("alsa seq output open: {e}")))?;
        seq.set_client_name(&cname)
            .map_err(|e| IoError::Init(format!("alsa seq set_client_name: {e}")))?;
        let mut port_info = seq::PortInfo::empty()
            .map_err(|e| IoError::Init(format!("alsa seq port_info: {e}")))?;
        port_info.set_capability(seq::PortCap::WRITE | seq::PortCap::SUBS_WRITE);
        port_info.set_type(seq::PortType::MIDI_GENERIC | seq::PortType::APPLICATION);
        port_info.set_name(&cname);
        seq.create_port(&port_info)
            .map_err(|e| IoError::Init(format!("alsa seq create_port: {e}")))?;
        Ok(Self { seq })
    }
```

- [ ] **Step 3: Add `midi_to_alsa_event()` helper** — after line 136 (end of `alsa_event_to_midi`):

```rust
fn midi_to_alsa_event(msg: &MidiMessage) -> seq::Event {
    let status = msg.status();
    let mut ev = seq::Event::empty().unwrap_or_default();
    match status {
        0xF8 => { ev.set_type(seq::EventType::Clock); }
        0xFA => { ev.set_type(seq::EventType::Start); }
        0xFB => { ev.set_type(seq::EventType::Continue); }
        0xFC => { ev.set_type(seq::EventType::Stop); }
        s if s & 0xF0 == 0x90 => {
            ev.set_type(seq::EventType::Noteon);
            let mut data = ev.get_data::<seq::EvNote>().unwrap_or_default();
            data.channel = s & 0x0F;
            data.note = msg.data1();
            data.velocity = msg.data2();
            ev.set_data(&data);
        }
        s if s & 0xF0 == 0x80 => {
            ev.set_type(seq::EventType::Noteoff);
            let mut data = ev.get_data::<seq::EvNote>().unwrap_or_default();
            data.channel = s & 0x0F;
            data.note = msg.data1();
            data.velocity = msg.data2();
            ev.set_data(&data);
        }
        _ => {}
    }
    ev
}
```

- [ ] **Step 4: Add `MidiOutput` impl** — after the `MidiInput` impl block (after line 79):

```rust
impl MidiOutput for AlsaSeqBackend {
    fn send(&mut self, message: &MidiMessage) -> IoResult<()> {
        let mut ev = midi_to_alsa_event(message);
        self.seq.event_output(&mut ev)
            .map_err(|e| IoError::Backend(format!("alsa seq output: {e}")))?;
        self.seq.drain_output()
            .map_err(|e| IoError::Backend(format!("alsa seq drain: {e}")))?;
        Ok(())
    }
}
```

- [ ] **Step 5: Verify**

```bash
cargo build -p rill-io --features alsa 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add rill-io/src/backends/alsa_seq.rs
git commit -m 'feat(rill-io): add MidiOutput impl to AlsaSeqBackend'
```

---

### Task 5: Add `MidiOutput` impl to `JackMidiBackend`

**File:** `rill-io/src/backends/jack_midi.rs`

- [ ] **Step 1: Add imports** — after line 16 (`use crate::midi_input::MidiInput;`):

```rust
use crate::midi_output::MidiOutput;
```

Change line 12 (`use jack::{Client, ClientOptions, Control, MidiIn, Port, ProcessHandler, ProcessScope};`) to add `MidiOut`:

```rust
use jack::{Client, ClientOptions, Control, MidiIn, MidiOut, Port, ProcessHandler, ProcessScope};
```

- [ ] **Step 2: Change struct and handler** — replace lines 24-28 and 113-127 with bidirectional support:

Replace the struct:
```rust
pub struct JackMidiBackend {
    rx: Receiver<MidiMessage>,
    _active: Option<jack::AsyncClient<(), JackMidiHandler>>,
    client_name: String,
}
```

With:
```rust
pub struct JackMidiBackend {
    pub(crate) rx: Option<Receiver<MidiMessage>>,
    tx: Option<SyncSender<MidiMessage>>,
    _active: Option<jack::AsyncClient<(), JackMidiHandler>>,
    client_name: String,
}
```

Replace `new()` (line 36-42) to handle the new field:
```rust
    pub fn new(client_name: impl Into<String>) -> IoResult<Self> {
        let (_tx, rx) = sync_channel::<MidiMessage>(0);
        Ok(Self {
            rx: Some(rx),
            tx: None,
            _active: None,
            client_name: client_name.into(),
        })
    }
```

Replace `connect()`'s `self.rx = rx;` (line 83) with `self.rx = Some(rx);`.

Replace `MidiInput for JackMidiBackend` (lines 89-102):
```rust
impl MidiInput for JackMidiBackend {
    fn poll(&mut self) -> IoResult<Vec<MidiMessage>> {
        let mut events = Vec::new();
        if let Some(ref rx) = self.rx {
            loop {
                match rx.try_recv() {
                    Ok(msg) => events.push(msg),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(events),
                }
            }
        }
        Ok(events)
    }
}
```

Replace the handler struct (lines 113-116):
```rust
struct JackMidiHandler {
    tx: Option<SyncSender<MidiMessage>>,
    rx: Option<Receiver<MidiMessage>>,
    midi_in: Option<Port<MidiIn>>,
    midi_out: Option<Port<MidiOut>>,
}
```

Replace the existing `connect()` handler construction (lines 57-58):
```rust
        let handler = JackMidiHandler { tx: Some(tx), rx: None, midi_in: Some(midi_in), midi_out: None };
```

Replace the `ProcessHandler` impl (lines 118-127):
```rust
impl ProcessHandler for JackMidiHandler {
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        // Input direction
        if let (Some(ref midi_in), Some(ref tx)) = (&self.midi_in, &self.tx) {
            for event in midi_in.iter(ps) {
                let msg = bytes_to_midi(event.bytes);
                let _ = tx.try_send(msg);
            }
        }
        // Output direction
        if let (Some(ref mut midi_out), Some(ref rx)) = (&mut self.midi_out, &self.rx) {
            let mut writer = midi_out.writer(ps);
            loop {
                match rx.try_recv() {
                    Ok(msg) => {
                        let _ = writer.write(&jack::RawMidi {
                            time: 0,
                            bytes: msg.as_bytes(),
                        });
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        }
        Control::Continue
    }
}
```

- [ ] **Step 3: Add `new_output()` and `connect_output()`** — after the existing `new()` and before `connect()`:

```rust
    /// Create a JACK MIDI output backend.
    pub fn new_output(client_name: impl Into<String>) -> IoResult<Self> {
        let (tx, _rx) = sync_channel::<MidiMessage>(0);
        Ok(Self {
            rx: None,
            tx: Some(tx),
            _active: None,
            client_name: client_name.into(),
        })
    }

    /// Connect output to JACK and register the MIDI out port.
    pub fn connect_output(&mut self) -> Result<(), String> {
        let name = &self.client_name;
        let (client, _status) = Client::new(name.as_str(), ClientOptions::NO_START_SERVER)
            .map_err(|e| format!("JACK MIDI output client new: {e:?}"))?;

        let midi_out: Port<MidiOut> = client.register_port("midi_out", MidiOut::default())
            .map_err(|e| format!("JACK MIDI output port: {e:?}"))?;

        let (tx_write, rx) = sync_channel(CHANNEL_CAPACITY);
        let handler = JackMidiHandler {
            tx: None,
            rx: Some(rx),
            midi_in: None,
            midi_out: Some(midi_out),
        };

        let active = client.activate_async((), handler)
            .map_err(|e| format!("JACK MIDI output activate: {e:?}"))?;

        self.tx = Some(tx_write);
        self._active = Some(active);
        Ok(())
    }
```

- [ ] **Step 4: Add `MidiOutput` impl** — after the `MidiInput` impl:

```rust
impl MidiOutput for JackMidiBackend {
    fn send(&mut self, message: &MidiMessage) -> IoResult<()> {
        let tx = self.tx.as_ref().ok_or_else(|| {
            IoError::Midi("backend opened as input, not output".into())
        })?;
        tx.try_send(*message)
            .map_err(|_| IoError::Midi("JACK MIDI output channel full".into()))?;
        Ok(())
    }
}
```

- [ ] **Step 5: Fix `Drop` impl** (line 105-109) — no changes needed, already uses `self._active.take()`.

- [ ] **Step 6: Verify**

```bash
cargo build -p rill-io --features jack 2>&1
```

- [ ] **Step 7: Commit**

```bash
git add rill-io/src/backends/jack_midi.rs
git commit -m 'feat(rill-io): add MidiOutput impl to JackMidiBackend'
```

---

### Task 6: Add `serialize_to_midi()` to `rill-patchbay`

**File:** `rill-patchbay/src/midi.rs`

- [ ] **Step 1: Add function** — after `parse_midi()` function (after line 307):

```rust
/// Serialize a [`ControlEvent`] back to a raw [`MidiMessage`].
///
/// This is the reverse of [`parse_midi`]. Only Clock, Transport,
/// and Note events are supported. Other events return `None`.
pub fn serialize_to_midi(event: &ControlEvent) -> Option<MidiMessage> {
    match event {
        ControlEvent::MidiClock => Some(MidiMessage::new(0xF8, 0, 0)),
        ControlEvent::MidiTransport { kind } => {
            let status = match kind {
                MidiTransportKind::Start => 0xFA,
                MidiTransportKind::Stop => 0xFC,
                MidiTransportKind::Continue => 0xFB,
            };
            Some(MidiMessage::new(status, 0, 0))
        }
        ControlEvent::MidiNote { note, velocity, on, .. } => {
            let status = if *on { 0x90 } else { 0x80 };
            Some(MidiMessage::new(status, *note, if *on { *velocity } else { 0 }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::queues::control_event::{ControlEvent, MidiTransportKind};

    #[test]
    fn test_serialize_midi_clock_roundtrip() {
        let event = ControlEvent::MidiClock;
        let msg = serialize_to_midi(&event).unwrap();
        let back = parse_midi(&msg).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn test_serialize_midi_transport_roundtrip() {
        for kind in [MidiTransportKind::Start, MidiTransportKind::Stop, MidiTransportKind::Continue] {
            let event = ControlEvent::MidiTransport { kind };
            let msg = serialize_to_midi(&event).unwrap();
            let back = parse_midi(&msg).unwrap();
            assert_eq!(back, event);
        }
    }

    #[test]
    fn test_serialize_midi_note_roundtrip() {
        let event = ControlEvent::MidiNote { channel: 0, note: 64, velocity: 100, on: true };
        let msg = serialize_to_midi(&event).unwrap();
        assert_eq!(msg, MidiMessage::new(0x90, 64, 100));

        let event_off = ControlEvent::MidiNote { channel: 0, note: 64, velocity: 0, on: false };
        let msg_off = serialize_to_midi(&event_off).unwrap();
        assert_eq!(msg_off, MidiMessage::new(0x80, 64, 0));
    }

    #[test]
    fn test_serialize_unsupported_returns_none() {
        let event = ControlEvent::MidiControl {
            channel: 0, controller: 7, value: 100, normalized: 0.8,
        };
        assert!(serialize_to_midi(&event).is_none());
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p rill-patchbay --features midi -- serialize_midi 2>&1
```

Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add rill-patchbay/src/midi.rs
git commit -m 'feat(rill-patchbay): add serialize_to_midi() with round-trip tests'
```

---

### Task 7: Create `MidiClockGenerator`

**File:** `rill-patchbay/src/midi_clock.rs` — append to existing file after line 356

- [ ] **Step 1: Add `MidiClockGenerator` struct and impl** — at end of file:

```rust
// =============================================================================
// MidiClockGenerator — output-side MIDI clock pulse generator
// =============================================================================

/// Generates MIDI clock pulses from signal-level [`ClockTick`] events.
///
/// This is the output counterpart of [`MidiClockTracker`] (input side).
/// On each `tick()` call it computes how many 24ppqn MIDI clock pulses
/// fall within the signal block and returns them as [`ControlEvent::MidiClock`].
///
/// Uses absolute sample position from [`ClockTick`] to avoid cumulative
/// drift. When BPM changes, `samples_per_tick` is recalculated.
pub struct MidiClockGenerator {
    next_tick_at: f64,
    samples_per_tick: f64,
    bpm: f64,
    playing: bool,
}

impl MidiClockGenerator {
    /// Create a stopped generator. No ticks are produced until
    /// a [`ControlEvent::MidiTransport`] with `Start` is received.
    pub fn new() -> Self {
        Self {
            next_tick_at: 0.0,
            samples_per_tick: 0.0,
            bpm: 0.0,
            playing: false,
        }
    }

    /// Process one signal block. Returns MIDI clock events for 24ppqn
    /// ticks that fall within `[clock.sample_pos, clock.sample_pos + block_size)`.
    /// Returns empty vec if transport is not playing or tempo is not set.
    pub fn tick(&mut self, clock: &rill_core::time::ClockTick) -> Vec<ControlEvent> {
        if !self.playing {
            return Vec::new();
        }

        let tempo = match clock.tempo {
            Some(t) => t as f64,
            None => return Vec::new(),
        };

        if (tempo - self.bpm).abs() > f64::EPSILON {
            self.bpm = tempo;
            let spb = 60.0 / self.bpm; // seconds per beat
            self.samples_per_tick = clock.sample_rate as f64 * spb / 24.0;
        }

        let block_end = clock.sample_pos as f64 + clock.samples_since_last as f64;
        let mut events = Vec::new();

        while self.next_tick_at < block_end {
            events.push(ControlEvent::MidiClock);
            self.next_tick_at += self.samples_per_tick;
        }

        events
    }

    /// Handle a transport state change.
    pub fn handle_transport(&mut self, kind: MidiTransportKind, current_sample: u64) {
        match kind {
            MidiTransportKind::Start => {
                self.next_tick_at = current_sample as f64;
                self.playing = true;
            }
            MidiTransportKind::Stop => {
                self.playing = false;
            }
            MidiTransportKind::Continue => {
                self.playing = true;
            }
        }
    }
}

impl Default for MidiClockGenerator {
    fn default() -> Self {
        Self::new()
    }
}
```

Add the required imports at top of the `midi_clock.rs` file (after line 16 `use rill_core::time::SystemClock;`):
```rust
use rill_core::queues::control_event::{ControlEvent, MidiTransportKind};
```

- [ ] **Step 2: Add tests** — at end of file, before the `#[cfg(test)]` block (after existing tests at line 356):

Inside the existing `mod tests` block, add:

```rust
    use rill_core::time::ClockTick;

    #[test]
    fn test_clock_generator_no_ticks_when_stopped() {
        let mut gen = MidiClockGenerator::new();
        let tick = ClockTick::new(48000, 256, 48000.0, "test".into());
        let events = gen.tick(&tick);
        assert!(events.is_empty());
    }

    #[test]
    fn test_clock_generator_no_ticks_without_tempo() {
        let mut gen = MidiClockGenerator::new();
        gen.handle_transport(MidiTransportKind::Start, 0);
        let tick = ClockTick {
            sample_pos: 0, samples_since_last: 256,
            is_new_block: true, sample_rate: 48000.0,
            tempo: None, source: "test".into(),
            speed_ratio: 1.0, is_final: true,
        };
        let events = gen.tick(&tick);
        assert!(events.is_empty());
    }

    #[test]
    fn test_clock_generator_produces_ticks_at_120bpm() {
        let mut gen = MidiClockGenerator::new();
        gen.handle_transport(MidiTransportKind::Start, 0);

        // At 120 BPM, 24 ppqn: one tick every 1000 samples at 48kHz
        // samples_per_tick = 48000 * 60 / (120 * 24) = 1000.0
        // A block of 2048 samples should produce 2 ticks (at 1000 and 2000)
        let tick = ClockTick {
            sample_pos: 0, samples_since_last: 2048,
            is_new_block: true, sample_rate: 48000.0,
            tempo: Some(120.0), source: "test".into(),
            speed_ratio: 1.0, is_final: true,
        };
        let events = gen.tick(&tick);
        assert_eq!(events.len(), 2);
        for e in &events {
            assert_eq!(*e, ControlEvent::MidiClock);
        }
    }

    #[test]
    fn test_clock_generator_tracks_phase_across_blocks() {
        let mut gen = MidiClockGenerator::new();
        gen.handle_transport(MidiTransportKind::Start, 0);

        // Block 1: 1200 samples, should get 1 tick at 1000
        let tick1 = ClockTick {
            sample_pos: 0, samples_since_last: 1200,
            is_new_block: true, sample_rate: 48000.0,
            tempo: Some(120.0), source: "test".into(),
            speed_ratio: 1.0, is_final: true,
        };
        let events1 = gen.tick(&tick1);
        assert_eq!(events1.len(), 1); // tick at sample 1000

        // Block 2: 1200 samples, next_tick_at was 2000,
        // now in range [1200, 2400) = one tick at 2000
        let tick2 = ClockTick {
            sample_pos: 1200, samples_since_last: 1200,
            is_new_block: true, sample_rate: 48000.0,
            tempo: Some(120.0), source: "test".into(),
            speed_ratio: 1.0, is_final: true,
        };
        let events2 = gen.tick(&tick2);
        assert_eq!(events2.len(), 1);
    }

    #[test]
    fn test_clock_generator_transport_stop() {
        let mut gen = MidiClockGenerator::new();
        gen.handle_transport(MidiTransportKind::Start, 0);

        let tick = ClockTick {
            sample_pos: 0, samples_since_last: 5000,
            is_new_block: true, sample_rate: 48000.0,
            tempo: Some(120.0), source: "test".into(),
            speed_ratio: 1.0, is_final: true,
        };
        let events = gen.tick(&tick);
        assert_eq!(events.len(), 5); // ticks at 1000, 2000, 3000, 4000, 5000
    }

    #[test]
    fn test_clock_generator_start_resets_phase() {
        let mut gen = MidiClockGenerator::new();
        gen.handle_transport(MidiTransportKind::Start, 48000);

        let tick = ClockTick {
            sample_pos: 48000, samples_since_last: 2048,
            is_new_block: true, sample_rate: 48000.0,
            tempo: Some(120.0), source: "test".into(),
            speed_ratio: 1.0, is_final: true,
        };
        let events = gen.tick(&tick);
        assert_eq!(events.len(), 2); // ticks at 49000, 50000
    }
```

- [ ] **Step 3: Verify tests**

```bash
cargo test -p rill-patchbay --features midi -- clock_generator 2>&1
```

Expected: all 6 new tests pass.

- [ ] **Step 4: Commit**

```bash
git add rill-patchbay/src/midi_clock.rs
git commit -m 'feat(rill-patchbay): add MidiClockGenerator with tests'
```

---

### Task 8: Create `spawn_midi_clock_output()` actor

**File:** `rill-patchbay/src/midi_clock.rs` — add after `MidiClockGenerator`

- [ ] **Step 1: Add imports** — at top of file add:

```rust
use rill_core::queues::CommandEnum;
use rill_core_actor::{ActorRef, ActorSystem};
use rill_io::midi_output::MidiOutput;
use rill_io::midi_message::MidiMessage;
```

- [ ] **Step 2: Add `spawn_midi_clock_output()` function** — after the `MidiClockGenerator` impl:

```rust
/// Spawn a MIDI clock output actor.
///
/// The actor owns a [`MidiOutput`] backend and a [`MidiClockGenerator`].
/// It receives [`CommandEnum::ClockTick`] via Rack broadcast and
/// [`CommandEnum::Control(ControlEvent::MidiTransport)`] for transport
/// control. For each clock tick, the generator produces MIDI clock
/// events which are serialized and sent via the backend.
pub fn spawn_midi_clock_output(
    system: &ActorSystem,
    output: Box<dyn MidiOutput>,
) -> ActorRef<CommandEnum> {
    let mut generator = MidiClockGenerator::new();
    let mut backend = output;

    system.spawn_detached(
        "midi_clock_output",
        move || {
            Box::new(move |msg: CommandEnum| match msg {
                CommandEnum::ClockTick(clock_tick) => {
                    let events = generator.tick(&clock_tick);
                    for event in &events {
                        if let Some(msg) = serialize_to_midi(event) {
                            if let Err(e) = backend.send(&msg) {
                                log::warn!("midi clock output send error: {e}");
                            }
                        }
                    }
                }
                CommandEnum::Control(ControlEvent::MidiTransport { kind }) => {
                    generator.handle_transport(kind, 0);
                    if let Some(msg) = serialize_to_midi(
                        &ControlEvent::MidiTransport { kind },
                    ) {
                        if let Err(e) = backend.send(&msg) {
                            log::warn!("midi transport send error: {e}");
                        }
                    }
                }
                _ => {}
            })
        },
        10,
    )
}
```

Need to import `serialize_to_midi` — since it's in `rill-patchbay::midi`, add crate-internal import:
```rust
use crate::midi::serialize_to_midi;
```

- [ ] **Step 3: Verify compile**

```bash
cargo build -p rill-patchbay --features midi 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add rill-patchbay/src/midi_clock.rs
git commit -m 'feat(rill-patchbay): add spawn_midi_clock_output() actor'
```

---

### Task 9: Add `ClockDef` to `ModuleDef` in `rill-patchbay`

**File:** `rill-patchbay/src/module_def.rs`

- [ ] **Step 1: Add `ClockDef` struct** — before `ModuleDef` (before line 289):

```rust
// ============================================================================
// ClockDef — MIDI clock output definition
// ============================================================================

/// Serializable MIDI clock output configuration.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct ClockDef {
    /// Backend type — `"midir"`, `"alsa_seq"`, or `"jack"`.
    pub backend: String,
    /// Port name for the backend.
    pub port_name: String,
    /// Start clock automatically when the system launches.
    #[cfg_attr(feature = "serde", serde(default))]
    pub auto_start: bool,
}
```

- [ ] **Step 2: Add `Clock` variant to `ModuleDef`** — change line 293-306:

```rust
pub enum ModuleDef {
    /// Servo: automaton → graph parameter bridge.
    Clock(ClockDef),
    /// Servo: automaton → graph parameter bridge.
    Servo(ServoDef),
    /// Sensor: external input (MIDI, OSC, etc.).
    Sensor(SensorDef),
    /// Custom module — dispatched through the module factory.
    Custom {
        /// Module type name for factory lookup.
        type_name: String,
        /// Module-specific parameters.
        #[cfg_attr(feature = "serde", serde(default))]
        params: HashMap<String, ParamValue>,
    },
}
```

Update `ModuleDef::type_name()` (line 310-317):
```rust
    pub fn type_name(&self) -> &str {
        match self {
            ModuleDef::Clock(_) => "clock",
            ModuleDef::Servo(_) => "servo",
            ModuleDef::Sensor(SensorDef::Midi { .. }) => "midi",
            ModuleDef::Sensor(SensorDef::Osc { .. }) => "osc",
            ModuleDef::Custom { type_name, .. } => type_name,
        }
    }
```

- [ ] **Step 3: Verify compile**

```bash
cargo build -p rill-patchbay 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add rill-patchbay/src/module_def.rs
git commit -m 'feat(rill-patchbay): add ClockDef to ModuleDef'
```

---

### Task 10: Update `rill-patchbay/src/lib.rs` exports

**Files:** `rill-patchbay/src/lib.rs`

- [ ] **Step 1: Add re-exports**

After line 116 (after `SongPosition`), add:
```rust
#[cfg(feature = "midi")]
pub use midi_clock::{spawn_midi_clock_output, MidiClockGenerator};
```

After line 91 (after `pub mod midi_clock`), add:
```rust
#[cfg(feature = "midi")]
pub use midi::serialize_to_midi;
```

After line 80 (after `pub use module_def::*;` — wait, there's no blanket re-export of module_def. Let me check... the file has `pub mod module_def;` on line 77. Good, `ClockDef` is accessible through the module path.)

Actually, let's also add `ClockDef` to the re-exports. After line 139:
```rust
    ControlEvent, EventPattern, Mapping,
    MidiNoteKind, Module, NoAction, OscSurface, OscSurfaceEntry, ParameterMapping, Servo, Target,
    Transform,
```
doesn't include `ClockDef` from module_def. Let me add it. Actually, `module_def` is already `pub mod`, so `rill_patchbay::module_def::ClockDef` works. But for convenience, add to re-exports:

After line 77, add an explicit re-export:
```rust
pub use module_def::ClockDef;
```

- [ ] **Step 2: Verify**

```bash
cargo build -p rill-patchbay --features midi 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add rill-patchbay/src/lib.rs
git commit -m 'feat(rill-patchbay): export MidiClockGenerator, spawn_midi_clock_output, ClockDef'
```

---

### Task 11: Add `Clock(ClockDef)` to `rill-adrift`'s `ModuleDef`

**File:** `rill-adrift/src/modular/serialization.rs`

- [ ] **Step 1: Add import**

After line 13 (`use rill_patchbay::serialization::{AutomatonDef, MappingDef, SensorDef, ServoDef};`):
```rust
use rill_patchbay::module_def::ClockDef;
```

- [ ] **Step 2: Add variant to `ModuleDef` enum** (lines 22-40)

Change:
```rust
pub enum ModuleDef {
    Servo(ServoDef),
    Sensor(SensorDef),
    Custom { ... },
    Graph { ... },
}
```

To:
```rust
pub enum ModuleDef {
    Clock(ClockDef),
    Servo(ServoDef),
    Sensor(SensorDef),
    Custom { ... },
    Graph { ... },
}
```

- [ ] **Step 3: Verify**

```bash
cargo build -p rill-adrift --features serialization 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add rill-adrift/src/modular/serialization.rs
git commit -m 'feat(rill-adrift): add ClockDef variant to ModuleDef'
```

---

### Task 12: Update `to_pb_module()` and rack launch dispatch

**File:** `rill-adrift/src/modular/mod.rs`

- [ ] **Step 1: Fix `to_pb_module()`** (line 301-311)

Add `Clock` variant handling:
```rust
fn to_pb_module(m: &ModuleDef) -> rill_patchbay::module_def::ModuleDef {
    match m {
        ModuleDef::Clock(c) => rill_patchbay::module_def::ModuleDef::Clock(c.clone()),
        ModuleDef::Servo(s) => rill_patchbay::module_def::ModuleDef::Servo(s.clone()),
        ModuleDef::Sensor(s) => rill_patchbay::module_def::ModuleDef::Sensor(s.clone()),
        ModuleDef::Custom { type_name, params } => rill_patchbay::module_def::ModuleDef::Custom {
            type_name: type_name.clone(),
            params: params.clone(),
        },
        ModuleDef::Graph { .. } => panic!("Graph modules are not handled by ModuleFactory"),
    }
}
```

- [ ] **Step 2: Fix `launch()` dispatch** (lines 257-268, the `match &pb_module` block)

Add clock handling to the module ID extraction:
```rust
let id = match &pb_module {
    PbModuleDef::Clock(c) => format!("clock_{}", c.port_name),
    PbModuleDef::Servo(s) => s.automaton_id.clone(),
    PbModuleDef::Sensor(s) => match s {
        SensorDef::Midi { port_name, .. } => format!("midi_{port_name}"),
        SensorDef::Osc { port, .. } => format!("osc_{port}"),
    },
    PbModuleDef::Custom { type_name, .. } => type_name.clone(),
};
```

Also ensure the import in this file includes `ClockDef`:
Already imported via `use rill_patchbay::module_def::ModuleDef as PbModuleDef` — and since `PbModuleDef` gets constructed from `to_pb_module()`, the variant will be handled.

- [ ] **Step 3: Verify**

```bash
cargo build -p rill-adrift --features serialization 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add rill-adrift/src/modular/mod.rs
git commit -m 'feat(rill-adrift): wire ClockDef through to_pb_module and rack dispatch'
```

---

### Task 13: Add `ClockConstructor` to `rill-adrift/src/registration.rs`

**File:** `rill-adrift/src/registration.rs`

- [ ] **Step 1: Add `register_clock_module()` function** — after the existing `register_midi_module`:

Insert after line 525 (`}` of `register_midi_module`):

```rust
#[cfg(feature = "midi")]
fn register_clock_module(factory: &mut rill_patchbay::module_factory::ModuleFactory) {
    use rill_core::queues::CommandEnum;
    use rill_io::midi_output::MidiOutput;
    use rill_patchbay::module_def::{ClockDef, ModuleDef};
    use rill_patchbay::module_factory::{ModuleConstructor, ModuleError};
    use rill_patchbay::midi_clock::spawn_midi_clock_output;
    use rill_core_actor::ActorRef;

    struct ClockConstructor;

    impl ModuleConstructor for ClockConstructor {
        fn type_name(&self) -> &'static str {
            "clock"
        }

        fn construct(
            &self,
            module: &ModuleDef,
            _automaton_defs: &[rill_patchbay::module_def::AutomatonDef],
            system: &std::sync::Arc<rill_core_actor::ActorSystem>,
            _graph_ref: &ActorRef<CommandEnum>,
        ) -> Result<ActorRef<CommandEnum>, ModuleError> {
            let (backend, port_name, auto_start) = match module {
                ModuleDef::Clock(ClockDef { backend, port_name, auto_start }) => {
                    (backend, port_name, auto_start)
                }
                _ => {
                    return Err(ModuleError::ConstructionFailed(
                        "ClockConstructor requires ModuleDef::Clock".into(),
                    ));
                }
            };

            let output: Box<dyn MidiOutput> = match backend.as_str() {
                "midir" => {
                    let b = rill_io::backends::MidirBackend::new_output_by_name("rill-clock", port_name)
                        .or_else(|_| rill_io::backends::MidirBackend::new_output("rill-clock"))
                        .map_err(|e| ModuleError::ConstructionFailed(e.to_string()))?;
                    Box::new(b)
                }
                #[cfg(feature = "alsa")]
                "alsa_seq" => Box::new(
                    rill_io::backends::AlsaSeqBackend::new_output(port_name)
                        .map_err(|e| ModuleError::ConstructionFailed(e.to_string()))?,
                ),
                _ => {
                    return Err(ModuleError::ConstructionFailed(format!(
                        "unknown MIDI output backend '{backend}'"
                    )));
                }
            };

            let clock_ref = spawn_midi_clock_output(system, output);

            if *auto_start {
                use rill_core::queues::control_event::{ControlEvent, MidiTransportKind};
                clock_ref.send(CommandEnum::Control(ControlEvent::MidiTransport {
                    kind: MidiTransportKind::Start,
                }));
            }

            Ok(clock_ref)
        }

        fn clone_box(&self) -> Box<dyn ModuleConstructor> {
            Box::new(ClockConstructor)
        }
    }

    factory.register(ClockConstructor);
}
```

- [ ] **Step 2: Call `register_clock_module()`** — find where `register_midi_module` is called

In `register_all_modules` function (search in `registration.rs` for call sites), add after the line that calls `register_midi_module(factory);`:

```rust
    register_clock_module(factory);
```

- [ ] **Step 3: Verify**

```bash
cargo build -p rill-adrift --features midi,serialization 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add rill-adrift/src/registration.rs
git commit -m 'feat(rill-adrift): add ClockConstructor for ModuleFactory'
```

---

### Task 14: Final verification — workspace build and tests

- [ ] **Step 1: Full workspace build**

```bash
cargo build --workspace --features midi,alsa,jack,serialization 2>&1
```

- [ ] **Step 2: Run all tests**

```bash
cargo test --workspace 2>&1
```

- [ ] **Step 3: Run clippy**

```bash
cargo clippy --workspace --features midi,alsa,jack,serialization 2>&1
```

- [ ] **Step 4: Run fmt**

```bash
cargo fmt --check
```

- [ ] **Step 5: Fix any issues, then final commit**

```bash
git add -A
git commit -m 'chore: final cleanup and verification for MIDI output'
```

---

## Summary of all commits

1. `refactor(rill-io): rename MidiBackend to MidiInput`
2. `feat(rill-io): add MidiOutput trait`
3. `feat(rill-io): add MidiOutput impl to MidirBackend`
4. `feat(rill-io): add MidiOutput impl to AlsaSeqBackend`
5. `feat(rill-io): add MidiOutput impl to JackMidiBackend`
6. `feat(rill-patchbay): add serialize_to_midi() with round-trip tests`
7. `feat(rill-patchbay): add MidiClockGenerator with tests`
8. `feat(rill-patchbay): add spawn_midi_clock_output() actor`
9. `feat(rill-patchbay): add ClockDef to ModuleDef`
10. `feat(rill-patchbay): export MidiClockGenerator, spawn_midi_clock_output, ClockDef`
11. `feat(rill-adrift): add ClockDef variant to ModuleDef`
12. `feat(rill-adrift): wire ClockDef through to_pb_module and rack dispatch`
13. `feat(rill-adrift): add ClockConstructor for ModuleFactory`
14. `chore: final cleanup and verification for MIDI output`
