//! Audio and MIDI I/O backends

mod null;

#[cfg(feature = "alsa")]
mod alsa;
#[cfg(feature = "alsa")]
mod alsa_seq;

#[cfg(feature = "pipewire")]
mod pipewire;

#[cfg(feature = "jack")]
mod jack;
#[cfg(feature = "jack")]
mod jack_midi;

#[cfg(feature = "portaudio")]
mod portaudio;

#[cfg(feature = "midir")]
mod midir_backend;

pub use null::NullBackend;

#[cfg(feature = "alsa")]
pub use alsa::AlsaBackend;
#[cfg(feature = "alsa")]
pub use alsa_seq::AlsaSeqBackend;

#[cfg(feature = "pipewire")]
pub use pipewire::PipewireBackend;

#[cfg(feature = "jack")]
pub use jack::JackBackend;
#[cfg(feature = "jack")]
pub use jack_midi::JackMidiBackend;

#[cfg(feature = "portaudio")]
pub use portaudio::PortAudioBackend;

#[cfg(feature = "midir")]
pub use midir_backend::MidirBackend;
