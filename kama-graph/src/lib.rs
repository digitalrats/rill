//! # Kama Graph - Audio graph with clock synchronization
//!
//! This crate provides a flexible audio graph for real-time processing
//! with clock synchronization and integration with the automaton world.
//!
//! ## Key Features
//!
//! - **Clock-driven processing** - All nodes process on clock ticks
//! - **Multiple signal types** - Audio, control, clock, feedback
//! - **Two-way communication** - Commands from automata, telemetry back
//! - **Micro-control observation** - Track real-time violations
//! - **Topological processing** - Automatic dependency resolution
//!
//! ## Example
//!
//! ```rust
//! use kama_graph::prelude::*;
//! use kama_core::queues::CommandQueue;
//!
//! const BLOCK_SIZE: usize = 64;
//!
//! // Create graph with system clock
//! let mut graph = AudioGraph::<f32, BLOCK_SIZE>::with_sample_rate(44100.0);
//!
//! // Add nodes
//! let osc_id = graph.add_processor(Box::new(MyOscillator::new(440.0)));
//! let filter_id = graph.add_processor(Box::new(MyFilter::new(1000.0)));
//!
//! // Connect them
//! graph.connect_audio(osc_id, 0, filter_id, 0)?;
//!
//! // Connect to automaton world
//! let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
//! graph.connect_command_queue(cmd_rx);
//!
//! // Process a block (active node = filter's output)
//! graph.process_block(filter_id)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![warn(missing_docs)]
#![deny(unsafe_code)]

mod graph;

pub use graph::{AudioGraph, GraphStats};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::{AudioGraph};
    pub use kama_core::prelude::*;
}