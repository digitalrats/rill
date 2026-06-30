/// Raw MIDI message — three bytes packed as [status, data1, data2].
///
/// Single-byte system real-time messages (Clock: `0xF8`, Start: `0xFA`,
/// Stop: `0xFC`, Continue: `0xFB`) pad data bytes with `0`.
/// Two-byte messages (Program Change: `0xC0–0xCF`) pad data2 with `0`.
///
/// Backends produce this type via [`MidiInput::poll`].
/// The interpretation (NoteOn, CC, Clock, Transport) happens downstream
/// in the [`MidiHub`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiMessage(pub [u8; 3]);

impl MidiMessage {
    /// Create a new MIDI message from three bytes.
    pub const fn new(status: u8, data1: u8, data2: u8) -> Self {
        Self([status, data1, data2])
    }

    /// MIDI status byte (upper nibble = message type, lower nibble = channel).
    pub fn status(&self) -> u8 {
        self.0[0]
    }

    /// First data byte.
    pub fn data1(&self) -> u8 {
        self.0[1]
    }

    /// Second data byte.
    pub fn data2(&self) -> u8 {
        self.0[2]
    }

    /// Full message as `[u8; 3]`.
    pub fn as_bytes(&self) -> &[u8; 3] {
        &self.0
    }

    /// Message type nibble (upper 4 bits of status byte).
    pub fn message_type(&self) -> u8 {
        self.0[0] & 0xF0
    }

    /// MIDI channel (lower 4 bits of status byte), valid for channel messages.
    pub fn channel(&self) -> u8 {
        self.0[0] & 0x0F
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_midi_message_fields() {
        let msg = MidiMessage::new(0x90, 0x3C, 0x7F);
        assert_eq!(msg.status(), 0x90);
        assert_eq!(msg.data1(), 0x3C);
        assert_eq!(msg.data2(), 0x7F);
        assert_eq!(msg.message_type(), 0x90);
        assert_eq!(msg.channel(), 0x00);
    }

    #[test]
    fn test_midi_message_channel() {
        let msg = MidiMessage::new(0x94, 0x40, 0x60);
        assert_eq!(msg.channel(), 0x04);
        assert_eq!(msg.message_type(), 0x90);
    }

    #[test]
    fn test_midi_message_system_realtime() {
        let clock = MidiMessage::new(0xF8, 0, 0);
        assert_eq!(clock.message_type(), 0xF0);
        assert_eq!(clock.channel(), 0x08);
    }
}
