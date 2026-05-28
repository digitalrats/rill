//! Cross-platform MIDI backend using `midir`.
//!
//! Connects to an existing MIDI input port and pushes received messages
//! through an internal channel, drained by [`MidiBackend::poll`].
//!
//! Supported platforms: Linux (ALSA), macOS (CoreMIDI), Windows (WinMM).

use std::sync::mpsc::{channel, Receiver, Sender};

use crate::error::{IoError, IoResult};
use crate::midi_backend::MidiBackend;
use crate::midi_message::MidiMessage;

/// Cross-platform MIDI backend powered by `midir`.
///
/// Opens the first available MIDI input port and uses a callback
/// → channel → poll pipeline to deliver [`MidiMessage`]s.
///
/// # Example
///
/// ```rust,no_run
/// use rill_io::midi_backend::MidiBackend;
/// use rill_io::backends::MidirBackend;
///
/// let mut be = MidirBackend::new("rill-midi").unwrap();
/// let _events = be.poll().unwrap();
/// ```
pub struct MidirBackend {
    rx: Receiver<MidiMessage>,
    _conn: midir::MidiInputConnection<()>,
}

impl MidirBackend {
    /// Create a new MIDI input, connect to the first available port.
    ///
    /// `name` — visible client name (e.g. `"rill-midi"`).
    pub fn new(name: &str) -> IoResult<Self> {
        Self::new_by_port(name, 0)
    }

    /// Create a new MIDI input, connect to the port at the given index.
    ///
    /// `port_index` — 0-based index into the list of available input ports.
    pub fn new_by_port(name: &str, port_index: usize) -> IoResult<Self> {
        let midi_in =
            midir::MidiInput::new(name).map_err(|e| IoError::Init(format!("midir new: {e}")))?;

        let ports = midi_in.ports();
        if ports.is_empty() {
            return Err(IoError::DeviceNotFound(
                "no MIDI input ports available".into(),
            ));
        }

        if port_index >= ports.len() {
            return Err(IoError::DeviceNotFound(format!(
                "MIDI port index {} out of range ({} ports available)",
                port_index,
                ports.len()
            )));
        }

        let (tx, rx): (Sender<MidiMessage>, Receiver<MidiMessage>) = channel();

        let port = &ports[port_index];
        let port_name = midi_in.port_name(port).unwrap_or_else(|_| "unknown".into());

        let conn = midi_in
            .connect(
                port,
                "rill-midi-in",
                move |_timestamp, message, _data| {
                    let msg = bytes_to_midi(message);
                    let _ = tx.send(msg);
                },
                (),
            )
            .map_err(|e| IoError::Init(format!("midir connect: {e}")))?;

        log::info!(
            "midir: connected to port #{port_index} '{port_name}' ({} total)",
            ports.len()
        );

        Ok(Self { rx, _conn: conn })
    }
}

impl MidiBackend for MidirBackend {
    fn poll(&mut self) -> IoResult<Vec<MidiMessage>> {
        let mut events = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(msg) => events.push(msg),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(IoError::Backend("midir channel disconnected".into()));
                }
            }
        }
        Ok(events)
    }
}

/// Convert raw MIDI bytes from `midir` callback into a [`MidiMessage`].
fn bytes_to_midi(bytes: &[u8]) -> MidiMessage {
    match bytes.len() {
        0 => MidiMessage::new(0, 0, 0),
        1 => MidiMessage::new(bytes[0], 0, 0),
        2 => MidiMessage::new(bytes[0], bytes[1], 0),
        _ => MidiMessage::new(bytes[0], bytes[1], bytes[2]),
    }
}
