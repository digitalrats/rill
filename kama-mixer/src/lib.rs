//! Audio mixer for Kama Audio
//!
//! Provides:
//! - Multiple mono/stereo channels
//! - Per-channel volume, pan, mute, solo
//! - Aux sends for effects
//! - Master output with volume

#![warn(missing_docs)]

mod channel;
mod send;
mod mixer;

pub use channel::{ChannelConfig, ChannelMode, ChannelState};
pub use send::{SendConfig, SendType};
pub use mixer::MixerNode;

// Re-export common types
pub use kama_core_traits::{AudioNode, ParamValue};