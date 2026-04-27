//! Analog circuit models — operational amplifiers, tape decks, preamps.

#![deny(unsafe_code)]

mod op_amp;
mod cassette;
mod nodes;

pub use op_amp::OperationalAmplifier;
pub use cassette::CassetteDeckModel;
pub use nodes::CassetteDeckProcessor;
