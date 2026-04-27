//! Бэкенды для аудио ввода-вывода

mod null;

#[cfg(feature = "cpal")]
mod cpal;

#[cfg(feature = "alsa")]
mod alsa;

#[cfg(feature = "pipewire")]
mod pipewire;

#[cfg(feature = "jack")]
mod jack;

pub use null::NullBackend;

#[cfg(feature = "cpal")]
pub use cpal::CpalBackend;

#[cfg(feature = "alsa")]
pub use alsa::AlsaBackend;

#[cfg(feature = "pipewire")]
pub use pipewire::PipewireBackend;

#[cfg(feature = "jack")]
pub use jack::JackBackend;
