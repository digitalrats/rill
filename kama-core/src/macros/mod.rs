//! Macros for creating audio nodes
//!
//! This module provides macros for easily creating Source, Processor, and Sink nodes
//! with proper support for:
//! - Audio inputs/outputs
//! - Control inputs (from LFOs, envelopes)
//! - Parameters (automation)
//! - Internal state

#[macro_use]
mod source;
#[macro_use]
mod processor;
#[macro_use]
mod sink;

// Re-export macros
pub use crate::{
    source_node, source_node_f32,
    processor_node, processor_node_f32,
    sink_node, sink_node_f32,
};

/// Prelude for macros
pub mod prelude {
    pub use crate::{
        source_node, source_node_f32,
        processor_node, processor_node_f32,
        sink_node, sink_node_f32,
    };
}