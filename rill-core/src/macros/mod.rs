//! # Macros for creating nodes and working with the core
//!
//! This module provides macros to simplify creating
//! various types of nodes in Rill.

#[macro_use]
mod params;
#[macro_use]
mod ports;

#[macro_use]
mod source;
#[macro_use]
mod processor;
#[macro_use]
mod sink;

mod tests;

// Re-export macros from the top level
pub use crate::{node, processor_node, sink_node, source_node, with_parameters};

/// Prelude for convenient import of all macros
pub mod prelude {
    pub use crate::{node, processor_node, sink_node, source_node, with_parameters};
}
