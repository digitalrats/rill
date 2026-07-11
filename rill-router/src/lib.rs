//! Signal routing, mixing, and equalization for Rill
//!
//! This crate provides:
//! - Equalizers (graphic, parametric) that work with any filter implementation
//! - Mixer with multiple channels, aux sends, and master output

#![warn(missing_docs)]

pub mod eq;
pub mod mixer;

pub use rill_core::traits::ParamValue;
pub use rill_core_dsp::filters::{Filter, FilterType};

pub use eq::{log_spaced_frequencies, BandType, EqBand, FilterFactory, GraphicEq};
pub use mixer::{ChannelConfig, ChannelMode, ChannelState, SendType};

pub mod register;

#[cfg(feature = "lang")]
mod lang;
