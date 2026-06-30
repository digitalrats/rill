//! ALSA sequencer MIDI backend.
//!
//! Opens an ALSA sequencer input port and polls for incoming MIDI events.

use std::ffi::CString;

use alsa::seq;
use alsa::Direction;

use crate::error::{IoError, IoResult};
use crate::midi_input::MidiInput;
use crate::midi_message::MidiMessage;

/// ALSA sequencer MIDI backend.
///
/// Creates a read-capable client port.  Auto-connects when another
/// port subscribes.
///
/// # Example
///
/// ```rust,no_run
/// use rill_io::midi_input::MidiInput;
/// use rill_io::backends::AlsaSeqBackend;
///
/// let mut be = AlsaSeqBackend::new("rill-midi").unwrap();
/// let _events = be.poll().unwrap();
/// ```
pub struct AlsaSeqBackend {
    seq: seq::Seq,
}

impl AlsaSeqBackend {
    /// Open the ALSA sequencer and create an input port.
    ///
    /// `name` — visible client name in ALSA patchbays.
    pub fn new(name: &str) -> IoResult<Self> {
        let cname = CString::new(name)
            .map_err(|_| IoError::Init(format!("name contains nul byte: {name}")))?;

        let seq = seq::Seq::open(None, Some(Direction::Capture), true)
            .map_err(|e| IoError::Init(format!("alsa seq open: {e}")))?;
        seq.set_client_name(&cname)
            .map_err(|e| IoError::Init(format!("alsa seq set_client_name: {e}")))?;

        let mut port_info = seq::PortInfo::empty()
            .map_err(|e| IoError::Init(format!("alsa seq port_info: {e}")))?;
        port_info.set_capability(seq::PortCap::READ | seq::PortCap::SUBS_READ);
        port_info.set_type(seq::PortType::MIDI_GENERIC | seq::PortType::APPLICATION);
        port_info.set_name(&cname);

        seq.create_port(&port_info)
            .map_err(|e| IoError::Init(format!("alsa seq create_port: {e}")))?;

        Ok(Self { seq })
    }
}

impl MidiInput for AlsaSeqBackend {
    fn poll(&mut self) -> IoResult<Vec<MidiMessage>> {
        let mut events = Vec::new();
        let mut input = self.seq.input();
        loop {
            match input.event_input() {
                Ok(event) => {
                    if let Some(msg) = alsa_event_to_midi(&event) {
                        events.push(msg);
                    }
                }
                Err(e) => {
                    // EAGAIN (11) — no events available in non-blocking mode
                    if e.errno() == 11 {
                        break;
                    }
                    return Err(IoError::Backend(format!("alsa seq poll: {e}")));
                }
            }
        }
        Ok(events)
    }
}

fn alsa_event_to_midi(ev: &seq::Event) -> Option<MidiMessage> {
    let ev_type = ev.get_type();
    match ev_type {
        seq::EventType::Noteon => {
            let data = ev.get_data::<seq::EvNote>().unwrap_or_default();
            let status = 0x90 | data.channel;
            Some(MidiMessage::new(status, data.note, data.velocity))
        }
        seq::EventType::Noteoff => {
            let data = ev.get_data::<seq::EvNote>().unwrap_or_default();
            let status = 0x80 | data.channel;
            Some(MidiMessage::new(status, data.note, data.velocity))
        }
        seq::EventType::Keypress => {
            let data = ev.get_data::<seq::EvNote>().unwrap_or_default();
            let status = 0xA0 | data.channel;
            Some(MidiMessage::new(status, data.note, data.velocity))
        }
        seq::EventType::Controller => {
            let data = ev.get_data::<seq::EvCtrl>().unwrap_or_default();
            let status = 0xB0 | data.channel;
            Some(MidiMessage::new(status, data.param as u8, data.value as u8))
        }
        seq::EventType::Pgmchange => {
            let data = ev.get_data::<seq::EvCtrl>().unwrap_or_default();
            let status = 0xC0 | data.channel;
            Some(MidiMessage::new(status, data.value as u8, 0))
        }
        seq::EventType::Chanpress => {
            let data = ev.get_data::<seq::EvCtrl>().unwrap_or_default();
            let status = 0xD0 | data.channel;
            Some(MidiMessage::new(status, data.value as u8, 0))
        }
        seq::EventType::Pitchbend => {
            let data = ev.get_data::<seq::EvCtrl>().unwrap_or_default();
            let val = data.value;
            let lsb = (val & 0x7F) as u8;
            let msb = ((val >> 7) & 0x7F) as u8;
            let status = 0xE0 | data.channel;
            Some(MidiMessage::new(status, lsb, msb))
        }
        seq::EventType::Songpos => {
            let data = ev.get_data::<seq::EvCtrl>().unwrap_or_default();
            let pos = data.value as u16;
            let lsb = (pos & 0x7F) as u8;
            let msb = ((pos >> 7) & 0x7F) as u8;
            Some(MidiMessage::new(0xF2, lsb, msb))
        }
        seq::EventType::Clock => Some(MidiMessage::new(0xF8, 0, 0)),
        seq::EventType::Start => Some(MidiMessage::new(0xFA, 0, 0)),
        seq::EventType::Continue => Some(MidiMessage::new(0xFB, 0, 0)),
        seq::EventType::Stop => Some(MidiMessage::new(0xFC, 0, 0)),
        _ => None,
    }
}
