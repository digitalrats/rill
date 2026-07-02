//! JACK MIDI backend — bridges JACK MIDI input to `MidiInput::poll()`.
//!
//! Creates a lightweight JACK client with a `MidiIn` port. The JACK
//! process callback writes incoming MIDI events to an mpsc channel;
//! `poll()` drains that channel (same pattern as `MidirBackend`).
//!
//! This backend is **not** the same client as `JackBackend` — audio and
//! MIDI use separate JACK clients, which is the standard approach.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

use jack::{Client, ClientOptions, Control, MidiIn, MidiOut, Port, ProcessHandler, ProcessScope};

use crate::error::{IoError, IoResult};
use crate::midi_input::MidiInput;
use crate::midi_message::MidiMessage;
use crate::midi_output::MidiOutput;

const CHANNEL_CAPACITY: usize = 256;

/// JACK MIDI input backend.
///
/// Registers a single `MidiIn` port named `"midi_in"`. Auto-connects to
/// the first available `midi_capture` port on startup.
pub struct JackMidiBackend {
    pub(crate) rx: Option<Receiver<MidiMessage>>,
    tx: Option<SyncSender<MidiMessage>>,
    _active: Option<jack::AsyncClient<(), JackMidiHandler>>,
    client_name: String,
}

impl JackMidiBackend {
    /// Create a new JACK MIDI backend.
    ///
    /// The backend is **not connected** until `connect()` is called.
    /// `client_name` appears in JACK patchbay (e.g. `"rill_midi"`).
    pub fn new(client_name: impl Into<String>) -> IoResult<Self> {
        let (_, rx) = sync_channel::<MidiMessage>(0);
        Ok(Self {
            rx: Some(rx),
            tx: None,
            _active: None,
            client_name: client_name.into(),
        })
    }

    /// Create a JACK MIDI output backend.
    pub fn new_output(client_name: impl Into<String>) -> IoResult<Self> {
        let (tx, _) = sync_channel::<MidiMessage>(0);
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

        let midi_out: Port<MidiOut> = client
            .register_port("midi_out", MidiOut)
            .map_err(|e| format!("JACK MIDI output port: {e:?}"))?;

        let (tx_write, rx) = sync_channel(CHANNEL_CAPACITY);
        let handler = JackMidiHandler {
            tx: None,
            rx: Some(rx),
            midi_in: None,
            midi_out: Some(midi_out),
        };

        let active = client
            .activate_async((), handler)
            .map_err(|e| format!("JACK MIDI output activate: {e:?}"))?;

        self.tx = Some(tx_write);
        self._active = Some(active);
        Ok(())
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
            .register_port("midi_in", MidiIn)
            .map_err(|e| format!("JACK MIDI port: {e:?}"))?;

        let (tx, rx) = sync_channel(CHANNEL_CAPACITY);
        let handler = JackMidiHandler {
            tx: Some(tx),
            rx: None,
            midi_in: Some(midi_in),
            midi_out: None,
        };

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

        self.rx = Some(rx);
        self._active = Some(active);
        Ok(())
    }
}

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

impl MidiOutput for JackMidiBackend {
    fn send(&mut self, message: &MidiMessage) -> IoResult<()> {
        let tx = self
            .tx
            .as_ref()
            .ok_or_else(|| IoError::Midi("backend opened as input, not output".into()))?;
        tx.try_send(*message)
            .map_err(|_| IoError::Midi("JACK MIDI output channel full".into()))?;
        Ok(())
    }
}

impl Drop for JackMidiBackend {
    fn drop(&mut self) {
        self._active.take();
    }
}

// ─── JACK Process Handler ──────────────────────────────────────────────────

struct JackMidiHandler {
    tx: Option<SyncSender<MidiMessage>>,
    rx: Option<Receiver<MidiMessage>>,
    midi_in: Option<Port<MidiIn>>,
    midi_out: Option<Port<MidiOut>>,
}

impl ProcessHandler for JackMidiHandler {
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        if let (Some(ref midi_in), Some(ref tx)) = (&self.midi_in, &self.tx) {
            for event in midi_in.iter(ps) {
                let msg = bytes_to_midi(event.bytes);
                let _ = tx.try_send(msg);
            }
        }
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

fn bytes_to_midi(bytes: &[u8]) -> MidiMessage {
    match bytes.len() {
        0 => MidiMessage::new(0, 0, 0),
        1 => MidiMessage::new(bytes[0], 0, 0),
        2 => MidiMessage::new(bytes[0], bytes[1], 0),
        _ => MidiMessage::new(bytes[0], bytes[1], bytes[2]),
    }
}
