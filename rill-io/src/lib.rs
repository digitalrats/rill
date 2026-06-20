//! Audio and MIDI I/O backends for Rill
//!
//! This crate provides a unified interface to various audio backends
//! (ALSA, PortAudio, PipeWire, JACK) and MIDI input backends.
//! Graph processing is handled by `rill-graph` — this crate is
//! purely about hardware I/O.

#![warn(missing_docs)]

mod backend;
pub mod buffer;
pub mod buffer_view;
mod config;
mod error;

pub mod backends;

pub mod output_window;

/// Audio output sink node (stereo, bridge to hardware).
pub mod output;

/// Audio input source node (stereo, bridge from hardware).
pub mod input;

/// Abstract audio I/O backend + registry.
pub mod audio_io;

/// Signal I/O pointer and backend registry.
pub mod signal_io;

/// Shared ring buffers and downcast helpers for I/O nodes.
pub mod rings;

/// Global PipeWire context (lazily initialised, shared by I/O nodes).
pub mod pw;

/// Raw MIDI message type.
pub mod midi_message;

/// MIDI backend trait.
pub mod midi_backend;

pub use backend::{BackendType, DeviceInfo};
pub use config::AudioConfig;
pub use error::{IoError, IoResult};
pub use input::AudioInput;
pub use input::Input;
pub use output::AudioOutput;
pub use output::Output;
pub use rings::PwBuffers;

pub use midi_backend::MidiBackend;
pub use midi_message::MidiMessage;

pub use backends::NullBackend;

#[cfg(feature = "alsa")]
pub use backends::AlsaBackend;
#[cfg(feature = "alsa")]
pub use backends::AlsaSeqBackend;

#[cfg(feature = "pipewire")]
pub use backends::PipewireBackend;

#[cfg(feature = "jack")]
pub use backends::JackBackend;
#[cfg(feature = "jack")]
pub use backends::JackMidiBackend;

#[cfg(feature = "midir")]
pub use backends::MidirBackend;

pub use audio_io::AudioIoPtr;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = AudioConfig::default();
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.buffer_size, 256);
        assert_eq!(config.input_channels, 2);
        assert_eq!(config.output_channels, 2);
    }

    #[test]
    fn test_config_with_methods() {
        let config = AudioConfig::new()
            .with_sample_rate(44100)
            .with_buffer_size(512)
            .with_channels(1);
        assert_eq!(config.sample_rate, 44100);
        assert_eq!(config.buffer_size, 512);
    }

    #[test]
    fn test_latency_calculation() {
        let config = AudioConfig::new()
            .with_sample_rate(48000)
            .with_buffer_size(256);
        let latency_sec = config.latency_seconds();
        let latency_ms = config.latency_ms();
        assert!((latency_sec - 256.0 / 48000.0).abs() < 1e-10);
        assert!((latency_ms - 256.0 * 1000.0 / 48000.0).abs() < 1e-10);
    }
}
