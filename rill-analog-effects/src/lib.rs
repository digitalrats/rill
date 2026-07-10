//! Analog circuit models — tape decks, preamps.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod cassette;
mod nodes;
mod tape_bridge;

pub use cassette::CassetteDeck;
pub use nodes::CassetteDeckProcessor;
pub use tape_bridge::{HeadConfig, TapeBridgeAlgorithm};

pub mod register;

/// rill-lang builtins for analog effects.
#[cfg(feature = "lang")]
mod lang;
