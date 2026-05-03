//! MIDI event types for I/O backends.
//!
//! These types represent raw MIDI events from hardware input sources
//! (PipeWire, ALSA sequencer, etc.).  Application code converts them
//! into domain-specific control events (e.g. `rill_patchbay::ControlEvent`).

/// A parsed MIDI event from an input device.
#[derive(Debug, Clone, PartialEq)]
pub enum MidiEvent {
    /// Note Off (channel, note, velocity).
    NoteOff { channel: u8, note: u8, velocity: u8 },
    /// Note On (channel, note, velocity; velocity 0 = Note Off).
    NoteOn { channel: u8, note: u8, velocity: u8 },
    /// Polyphonic Key Pressure / Aftertouch.
    PolyphonicKeyPressure { channel: u8, note: u8, pressure: u8 },
    /// Control Change / MIDI CC (channel, controller, value).
    ControlChange { channel: u8, controller: u8, value: u8 },
    /// Program Change (channel, program).
    ProgramChange { channel: u8, program: u8 },
    /// Channel Pressure (channel, pressure).
    ChannelPressure { channel: u8, pressure: u8 },
    /// Pitch Bend (channel, bend; 0–16383, centre 8192).
    PitchBend { channel: u8, bend: u16 },
}

impl MidiEvent {
    /// Parse a raw MIDI byte sequence (1–3 bytes, running status not handled).
    ///
    /// Returns `None` for unrecognised or incomplete messages.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }
        let status = bytes[0];
        let channel = status & 0x0F;
        match status & 0xF0 {
            0x80 if bytes.len() >= 3 => Some(MidiEvent::NoteOff {
                channel,
                note: bytes[1],
                velocity: bytes[2],
            }),
            0x90 if bytes.len() >= 3 => {
                let vel = bytes[2];
                if vel == 0 {
                    Some(MidiEvent::NoteOff {
                        channel,
                        note: bytes[1],
                        velocity: 0,
                    })
                } else {
                    Some(MidiEvent::NoteOn {
                        channel,
                        note: bytes[1],
                        velocity: vel,
                    })
                }
            }
            0xA0 if bytes.len() >= 3 => Some(MidiEvent::PolyphonicKeyPressure {
                channel,
                note: bytes[1],
                pressure: bytes[2],
            }),
            0xB0 if bytes.len() >= 3 => Some(MidiEvent::ControlChange {
                channel,
                controller: bytes[1],
                value: bytes[2],
            }),
            0xC0 if bytes.len() >= 2 => Some(MidiEvent::ProgramChange {
                channel,
                program: bytes[1],
            }),
            0xD0 if bytes.len() >= 2 => Some(MidiEvent::ChannelPressure {
                channel,
                pressure: bytes[1],
            }),
            0xE0 if bytes.len() >= 3 => {
                let lsb = bytes[1] as u16;
                let msb = bytes[2] as u16;
                Some(MidiEvent::PitchBend {
                    channel,
                    bend: (msb << 7) | lsb,
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_note_on() {
        let ev = MidiEvent::from_bytes(&[0x90, 0x3C, 0x7F]).unwrap();
        assert_eq!(ev, MidiEvent::NoteOn { channel: 0, note: 60, velocity: 127 });
    }

    #[test]
    fn test_note_off() {
        let ev = MidiEvent::from_bytes(&[0x80, 0x3C, 0x40]).unwrap();
        assert_eq!(ev, MidiEvent::NoteOff { channel: 0, note: 60, velocity: 64 });
    }

    #[test]
    fn test_velocity_zero_as_note_off() {
        let ev = MidiEvent::from_bytes(&[0x90, 0x3C, 0x00]).unwrap();
        assert_eq!(ev, MidiEvent::NoteOff { channel: 0, note: 60, velocity: 0 });
    }

    #[test]
    fn test_cc() {
        let ev = MidiEvent::from_bytes(&[0xB1, 0x07, 0x40]).unwrap();
        assert_eq!(ev, MidiEvent::ControlChange { channel: 1, controller: 7, value: 64 });
    }

    #[test]
    fn test_pitch_bend() {
        let ev = MidiEvent::from_bytes(&[0xE0, 0x00, 0x40]).unwrap();
        assert_eq!(ev, MidiEvent::PitchBend { channel: 0, bend: 8192 });
    }

    #[test]
    fn test_invalid() {
        assert!(MidiEvent::from_bytes(&[]).is_none());
        assert!(MidiEvent::from_bytes(&[0xF0]).is_none());
    }
}
