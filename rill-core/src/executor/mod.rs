//! Graph executor for driving audio processing using the topology graph and active ports.
//!
//! The `GraphExecutor` coordinates clock ticks, pulls data from input ports,
//! calls node processing, and pushes data to output ports.
//!
//! This module provides a prototype implementation that demonstrates the active ports flow.
//! A real implementation would integrate with a concrete graph (e.g., `rill_graph::AudioGraph`).
//!
//! # Example
//! ```
//! use rill_core::executor::GraphExecutor;
//! use rill_core::math::AudioNum;
//!
//! // Create a graph executor (graph omitted for prototype).
//! // let mut executor = GraphExecutor::new(graph);
//! // executor.process_block().unwrap();
//! ```

use crate::math::AudioNum;
use crate::traits::ActivePort;

/// Executor for an audio graph that processes nodes in topological order.
///
/// This is a prototype that outlines the structure. In a real implementation,
/// the executor would hold a concrete graph and iterate over its nodes.
pub struct GraphExecutor<T: AudioNum, const BUF_SIZE: usize> {
    // Placeholder for the audio graph.
    // In reality, this would be something like `graph: rill_graph::AudioGraph<T, BUF_SIZE>`.
    _phantom: std::marker::PhantomData<T>,
}

impl<T: AudioNum, const BUF_SIZE: usize> GraphExecutor<T, BUF_SIZE> {
    /// Create a new executor from an existing graph.
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }

    /// Process one block of audio.
    ///
    /// This advances the clock, obtains topological order, processes each node
    /// by pulling inputs, calling appropriate processing method, and pushing outputs.
    ///
    /// # Returns
    /// `Ok(())` if processing succeeded, or an error if something went wrong.
    ///
    /// # Algorithm (conceptual)
    /// 1. Advance the clock (if the graph provides a clock source).
    /// 2. Obtain topological order (if the graph provides it).
    /// 3. For each node in order:
    ///    a. Gather input data by calling `pull` on each input port (ActivePort trait).
    ///    b. Determine node type (Source/Processor/Sink) via metadata.
    ///    c. Call the appropriate processing method (generate/process/consume).
    ///    d. Push output data by calling `push` on each output port.
    /// 4. Handle errors (disconnected ports, missing data).
    pub fn process_block(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // This is a stub implementation. A real executor would need access to
        // the graph's nodes and connections, which is beyond the scope of this prototype.
        Ok(())
    }
}

/// Example of using ActivePort trait to pull data from an input port.
/// This function illustrates the intended pattern.
pub fn demonstrate_pull<T: AudioNum, const BUF_SIZE: usize>(
    port: &mut dyn ActivePort<T, BUF_SIZE>,
) -> Option<[T; BUF_SIZE]> {
    // Pull data from the port (if connected).
    port.pull()
}

/// Example of pushing data to an output port.
pub fn demonstrate_push<T: AudioNum, const BUF_SIZE: usize>(
    port: &mut dyn ActivePort<T, BUF_SIZE>,
    data: [T; BUF_SIZE],
) -> Result<(), crate::traits::PortError> {
    port.push(data)
}