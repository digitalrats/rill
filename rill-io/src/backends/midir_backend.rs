//! Cross-platform MIDI backend using `midir`.
//!
//! Connects to an existing MIDI input port and pushes received messages
//! through an internal channel, drained by [`MidiInput::poll`].
//!
//! Supported platforms: Linux (ALSA), macOS (CoreMIDI), Windows (WinMM).

use std::sync::mpsc::{channel, Receiver, Sender};

use crate::error::{IoError, IoResult};
use crate::midi_input::MidiInput;
use crate::midi_message::MidiMessage;
use crate::midi_output::MidiOutput;

#[allow(dead_code)]
enum MidirConnection {
    Input(midir::MidiInputConnection<()>),
    Output(midir::MidiOutputConnection),
}

/// Cross-platform MIDI backend powered by `midir`.
///
/// Opens the first available MIDI input port and uses a callback
/// → channel → poll pipeline to deliver [`MidiMessage`]s.
///
/// # Example
///
/// ```rust,no_run
/// use rill_io::midi_input::MidiInput;
/// use rill_io::backends::MidirBackend;
///
/// let mut be = MidirBackend::new("rill-midi").unwrap();
/// let _events = be.poll().unwrap();
/// ```
pub struct MidirBackend {
    rx: Receiver<MidiMessage>,
    _conn: MidirConnection,
}

impl MidirBackend {
    /// List available input ports to stderr (for diagnostics).
    pub fn list_ports(client_name: &str) -> IoResult<()> {
        let mi =
            midir::MidiInput::new(client_name).map_err(|e| IoError::Init(format!("midir: {e}")))?;
        let ports = mi.ports();
        for (i, p) in ports.iter().enumerate() {
            let name = mi.port_name(p).unwrap_or_else(|_| "?".into());
            eprintln!("  MIDI port #{i}: {name}");
        }
        eprintln!("  ({} ports total)", ports.len());
        Ok(())
    }

    /// Create a new MIDI input, connect to the first available port.
    ///
    /// Skips virtual ports (`"Through"`, `"Loop"`, `"RTMidi"`) and
    /// selects the first real hardware port. Falls back to port 0 if
    /// no real ports are found.
    ///
    /// `name` — visible client name (e.g. `"rill-midi"`).
    pub fn new(name: &str) -> IoResult<Self> {
        let mi = midir::MidiInput::new(name).map_err(|e| IoError::Init(format!("midir: {e}")))?;

        let ports = mi.ports();
        if ports.is_empty() {
            return Err(IoError::DeviceNotFound(
                "no MIDI input ports available".into(),
            ));
        }

        let find_real = |pname: &str| -> bool {
            let lower = pname.to_lowercase();
            !lower.contains("through") && !lower.contains("loop") && !lower.contains("rtmidi")
        };

        let mut chosen = 0usize;
        for (i, p) in ports.iter().enumerate() {
            let pname = mi.port_name(p).unwrap_or_else(|_| "?".into());
            if find_real(&pname) {
                chosen = i;
                break;
            }
        }

        Self::connect(name, |midi_in, all_ports| {
            let pname = midi_in
                .port_name(&all_ports[chosen])
                .unwrap_or_else(|_| "?".into());
            Ok((chosen, pname))
        })
    }

    /// Create a new MIDI input, connect to the port at the given index.
    ///
    /// `port_index` — 0-based index into the list of available input ports.
    pub fn new_by_port(name: &str, port_index: usize) -> IoResult<Self> {
        Self::connect(name, |midi_in, ports| {
            if port_index >= ports.len() {
                return Err(IoError::DeviceNotFound(format!(
                    "port index {} out of range ({} total)",
                    port_index,
                    ports.len()
                )));
            }
            Ok((
                port_index,
                midi_in
                    .port_name(&ports[port_index])
                    .unwrap_or_else(|_| "?".into()),
            ))
        })
    }

    /// Create a new MIDI input, connect to a port whose name contains `port_name`.
    ///
    /// `port_name` — substring to search for in the port names (case-sensitive).
    /// Matches the first port whose name contains this string.
    pub fn new_by_name(name: &str, port_name: &str) -> IoResult<Self> {
        Self::connect(name, |midi_in, ports| {
            for (i, p) in ports.iter().enumerate() {
                let pname = midi_in.port_name(p).unwrap_or_else(|_| "?".into());
                if pname.contains(port_name) {
                    return Ok((i, pname));
                }
            }
            Err(IoError::DeviceNotFound(format!(
                "no MIDI port matching '{}' ({} ports available)",
                port_name,
                ports.len()
            )))
        })
    }

    fn connect(
        name: &str,
        find: impl FnOnce(&midir::MidiInput, &[midir::MidiInputPort]) -> IoResult<(usize, String)>,
    ) -> IoResult<Self> {
        let midi_in =
            midir::MidiInput::new(name).map_err(|e| IoError::Init(format!("midir: {e}")))?;

        let ports = midi_in.ports();
        if ports.is_empty() {
            return Err(IoError::DeviceNotFound(
                "no MIDI input ports available".into(),
            ));
        }

        let (port_idx, port_name) = find(&midi_in, &ports)?;
        let port = &ports[port_idx];

        let (tx, rx): (Sender<MidiMessage>, Receiver<MidiMessage>) = channel();
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
            "midir: connected to port #{port_idx} '{port_name}' ({} total)",
            ports.len()
        );

        Ok(Self {
            rx,
            _conn: MidirConnection::Input(conn),
        })
    }

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
                port_name,
                ports.len()
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
        let midi_out =
            midir::MidiOutput::new(name).map_err(|e| IoError::Init(format!("midir out: {e}")))?;
        let ports = midi_out.ports();
        let (port_idx, port_name) = find(&midi_out, &ports)?;
        let port = &ports[port_idx];
        let conn = midi_out
            .connect(port, "rill-midi-out")
            .map_err(|e| IoError::Init(format!("midir out connect: {e}")))?;
        log::info!("midir out: connected to port #{port_idx} '{port_name}'");
        let (_tx, rx) = std::sync::mpsc::channel::<MidiMessage>();
        Ok(Self {
            rx,
            _conn: MidirConnection::Output(conn),
        })
    }
}

impl MidiInput for MidirBackend {
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

impl MidiOutput for MidirBackend {
    fn send(&mut self, message: &MidiMessage) -> IoResult<()> {
        match &mut self._conn {
            MidirConnection::Output(conn) => {
                conn.send(message.as_bytes())
                    .map_err(|e| IoError::Midi(format!("midir send: {e}")))?;
                Ok(())
            }
            MidirConnection::Input(_) => {
                Err(IoError::Midi("backend opened as input, not output".into()))
            }
        }
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
