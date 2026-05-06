use std::io;

use thiserror::Error;

/// Errors that can occur during OSC encoding, decoding, or I/O.
#[derive(Error, Debug)]
pub enum Error {
    /// Generic parse error with a description message.
    #[error("OSC parse error: {0}")]
    Parse(String),

    /// Wraps an underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// The packet does not match any known OSC format.
    #[error("invalid OSC packet")]
    InvalidPacket,

    /// Type tag mismatch between expected and actual types.
    #[error("type mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        /// The expected type description.
        expected: String,
        /// The actual type description.
        actual: String,
    },
}
