//! OSC (Open Sound Control) protocol implementation — encoding, decoding, and UDP server.

#![warn(missing_docs)]

/// OSC error types.
pub mod error;
/// OSC protocol types and encode/decode functions.
pub mod osc;
/// UDP-based OSC server for receiving and dispatching messages.
pub mod server;

/// Convenience re-exports for common OSC types.
pub mod prelude {
    pub use crate::error::Error;
    pub use crate::osc::{
        decode, encode, pattern_match, OscBundle, OscMessage, OscPacket, OscType, TimeTag,
    };
    pub use crate::server::OscServer;
}
