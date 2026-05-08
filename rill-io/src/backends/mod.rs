//! Audio I/O backends

mod null;

#[cfg(feature = "alsa")]
mod alsa;

#[cfg(feature = "pipewire")]
mod pipewire;

#[cfg(feature = "jack")]
mod jack;

#[cfg(feature = "portaudio")]
mod portaudio;

pub use null::NullBackend;

#[cfg(feature = "alsa")]
pub use alsa::AlsaBackend;

#[cfg(feature = "pipewire")]
pub use pipewire::PipewireBackend;

#[cfg(feature = "jack")]
pub use jack::JackBackend;

#[cfg(feature = "portaudio")]
pub use portaudio::PortAudioBackend;
