//! Audio I/O backends for Rill
//!
//! This crate provides a unified interface to various audio backends
//! (ALSA, CPAL, PipeWire, JACK). Graph processing is handled by
//! `rill-graph` — this crate is purely about hardware I/O.

#![warn(missing_docs)]

mod backend;
pub mod buffer;
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

pub use backend::{AudioBackend, BackendType, DeviceInfo};
pub use config::AudioConfig;
pub use error::{IoError, IoResult};
pub use input::AudioInput;
pub use output::AudioOutput;
pub use rings::PwBuffers;

pub use backends::NullBackend;

#[cfg(feature = "cpal")]
pub use backends::CpalBackend;

#[cfg(feature = "alsa")]
pub use backends::AlsaBackend;

#[cfg(feature = "pipewire")]
pub use backends::PipewireBackend;

#[cfg(feature = "jack")]
pub use backends::JackBackend;

pub use signal_io::IoBackendPtr;

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
