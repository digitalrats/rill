//! Analog circuit models — operational amplifiers, tape decks, preamps.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod cassette;
mod nodes;
mod op_amp;

pub use cassette::CassetteDeckModel;
pub use nodes::CassetteDeckProcessor;
pub use op_amp::OperationalAmplifier;
