//! Core traits for the Kama Audio ecosystem

mod node;
mod param;
mod port;

// Re-exports
pub use node::*;
pub use param::*;
pub use port::*;

// Time traits are re-exported from the time module
pub use crate::time::{Clock, TimeProvider, SystemClock, TickInfo};

/// Prelude for common trait imports
pub mod prelude {
    pub use super::{
        Source, Processor, Sink,
        NodeId, NodeMetadata, NodeCategory, NodeTypeId,
        ParameterId, ParamValue, ParamType, ParamRange, ParamMetadata,
        PortId, PortType,
        // Time traits from the time module
        Clock, TimeProvider, SystemClock, TickInfo,
    };
}