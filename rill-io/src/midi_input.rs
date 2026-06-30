//! MIDI input trait for polling raw MIDI messages.
//!
//! Similar in spirit to [`crate::audio_io`] for audio, `MidiInput`
//! provides a uniform interface for receiving MIDI data from hardware
//! or virtual devices.
//!
//! Implementations:
//! - `AlsaSeqBackend` (behind `alsa` feature)
//! - `PipewireMidiBackend` (behind `pipewire` feature, planned)

use crate::error::IoResult;
use crate::midi_message::MidiMessage;

/// Generic MIDI input backend.
///
/// # Lifecycle
///
/// 1. Construct the backend with device-specific configuration
/// 2. Call `poll()` at regular intervals (10–20 ms)
/// 3. Drop the backend to close the device
///
/// `poll()` may block briefly (with a timeout) to wait for events.
/// This trait is intended for use on a dedicated non-RT thread — not
/// the audio callback.
pub trait MidiInput: Send + 'static {
    /// Poll for available MIDI messages.
    ///
    /// Returns immediately if no messages have arrived since the last
    /// call.  May block briefly (~1–10 ms) to batch multiple events
    /// into a single call.
    fn poll(&mut self) -> IoResult<Vec<MidiMessage>>;
}
