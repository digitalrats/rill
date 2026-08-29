//! Analog circuit models — cassette deck.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod cassette;
mod nodes;

pub use cassette::CassetteDeck;
pub use nodes::CassetteDeckProcessor;

/// Register graph nodes and lang builtins for analog effects.
pub mod register;

/// rill-lang builtins for analog effects.
#[cfg(feature = "lang")]
mod lang;
