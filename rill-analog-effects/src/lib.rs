//! Analog circuit models — tape decks, preamps.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod cassette;
mod nodes;

pub use cassette::CassetteDeck;
pub use nodes::CassetteDeckProcessor;
