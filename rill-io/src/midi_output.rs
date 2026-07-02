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
