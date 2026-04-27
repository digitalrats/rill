pub mod error;
pub mod osc;
pub mod server;

pub mod prelude {
    pub use crate::error::Error;
    pub use crate::osc::{
        decode, encode, pattern_match, OscBundle, OscMessage, OscPacket, OscType, TimeTag,
    };
    pub use crate::server::OscServer;
}
