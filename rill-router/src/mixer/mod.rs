//! Audio mixer for Rill
//!
//! Provides:
//! - Multiple mono/stereo channels
//! - Per-channel volume, pan, mute, solo
//! - Aux sends for effects
//! - Master output with volume

mod channel;
#[allow(clippy::module_name_repetitions)]
mod mixer;
mod send;

pub use channel::{ChannelConfig, ChannelMode, ChannelState};
pub use mixer::MixerNode;
pub use send::{SendConfig, SendType};
