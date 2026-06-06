//! JACK MIDI backend — bridges JACK MIDI input to `MidiBackend::poll()`.
//!
//! Creates a lightweight JACK client with a `MidiIn` port. The JACK
//! process callback writes incoming MIDI events to an mpsc channel;
//! `poll()` drains that channel (same pattern as `MidirBackend`).
//!
//! This backend is **not** the same client as `JackBackend` — audio and
//! MIDI use separate JACK clients, which is the standard approach.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

use jack::{Client, ClientOptions, Control, MidiIn, Port, ProcessHandler, ProcessScope};

use crate::error::IoResult;
use crate::midi_backend::MidiBackend;
use crate::midi_message::MidiMessage;

const CHANNEL_CAPACITY: usize = 256;

/// JACK MIDI input backend.
///
/// Registers a single `MidiIn` port named `"midi_in"`. Auto-connects to
/// the first available `midi_capture` port on startup.
pub struct JackMidiBackend {
    rx: Receiver<MidiMessage>,
    _active: Option<jack::AsyncClient<(), JackMidiHandler>>,
    client_name: String,
}

impl JackMidiBackend {
    /// Create a new JACK MIDI backend.
    ///
    /// The backend is **not connected** until `connect()` is called.
    /// `client_name` appears in JACK patchbay (e.g. `"rill_midi"`).
    pub fn new(client_name: impl Into<String>) -> IoResult<Self> {
        let (_tx, rx) = sync_channel::<MidiMessage>(0);
        Ok(Self {
            rx,
            _active: None,
            client_name: client_name.into(),
        })
    }

    /// Connect to JACK and register the MIDI port.
    ///
    /// Must be called before `poll()`. Spawns the JACK process handler
    /// that forwards incoming MIDI events to the internal mpsc channel.
    pub fn connect(&mut self) -> Result<(), String> {
        let name = &self.client_name;
        let (client, _status) = Client::new(name.as_str(), ClientOptions::NO_START_SERVER)
            .map_err(|e| format!("JACK MIDI client new: {e:?}"))?;

        let midi_in: Port<MidiIn> = client
            .register_port("midi_in", MidiIn::default())
            .map_err(|e| format!("JACK MIDI port: {e:?}"))?;

        let (tx, rx) = sync_channel(CHANNEL_CAPACITY);
        let handler = JackMidiHandler { tx, midi_in };

        let active = client
            .activate_async((), handler)
            .map_err(|e| format!("JACK MIDI activate: {e:?}"))?;

        // Auto-connect to first available MIDI capture port, if any
        let port_name = active
            .as_client()
            .ports(None, None, jack::PortFlags::IS_OUTPUT)
            .into_iter()
            .find(|p| p.contains("midi_capture") || p.contains("Midi Through"));
        if let Some(src) = port_name {
            if let Some(midi_name) = active
                .as_client()
                .ports(None, None, jack::PortFlags::IS_INPUT)
                .into_iter()
                .find(|p| p.starts_with(name.as_str()))
            {
                if let Err(e) = active.as_client().connect_ports_by_name(&src, &midi_name) {
                    log::info!("JACK MIDI connect {src} → {midi_name}: {e}");
                }
            }
        }

        self.rx = rx;
        self._active = Some(active);
        Ok(())
    }
}

impl MidiBackend for JackMidiBackend {
    fn poll(&mut self) -> IoResult<Vec<MidiMessage>> {
        let mut events = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(msg) => events.push(msg),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Ok(events);
                }
            }
        }
        Ok(events)
    }
}

impl Drop for JackMidiBackend {
    fn drop(&mut self) {
        self._active.take();
    }
}

// ─── JACK Process Handler ──────────────────────────────────────────────────

struct JackMidiHandler {
    tx: SyncSender<MidiMessage>,
    midi_in: Port<MidiIn>,
}

impl ProcessHandler for JackMidiHandler {
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        for event in self.midi_in.iter(ps) {
            let bytes = event.bytes;
            let msg = bytes_to_midi(bytes);
            let _ = self.tx.try_send(msg);
        }
        Control::Continue
    }
}

fn bytes_to_midi(bytes: &[u8]) -> MidiMessage {
    match bytes.len() {
        0 => MidiMessage::new(0, 0, 0),
        1 => MidiMessage::new(bytes[0], 0, 0),
        2 => MidiMessage::new(bytes[0], bytes[1], 0),
        _ => MidiMessage::new(bytes[0], bytes[1], bytes[2]),
    }
}
