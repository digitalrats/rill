use std::io;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("OSC parse error: {0}")]
    Parse(String),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("invalid OSC packet")]
    InvalidPacket,

    #[error("type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },
}
