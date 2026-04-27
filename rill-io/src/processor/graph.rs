//! Integration with `rill_graph::AudioGraph`
//!
//! The `GraphProcessor` bridges the real-time I/O callback world
//! (`&[f32]` slices) with the graph-based processing model of `rill_graph`.
//! Since the graph's audio processing is driven externally (via the
//! `GraphExecutor` in `rill_core`), this processor exposes the graph for
//! inspection, parameter dispatch, and access to output buffers.
//! Actual audio processing through the graph will be enabled once the
//! `GraphExecutor` is fully integrated.

use std::marker::PhantomData;

use rill_core::math::Transcendental;
use rill_core::time::ClockSource;
use rill_graph::{AudioGraph, GraphBuilder};

use crate::engine::AudioProcessor;

const DEFAULT_BUF_SIZE: usize = 256;

/// Processor that wraps a `rill_graph::AudioGraph`.
///
/// `BUF_SIZE` must match the block size used when building the graph
/// (defaults to 256).
pub struct GraphProcessor<T: Transcendental = f32, const BUF_SIZE: usize = DEFAULT_BUF_SIZE> {
    graph: AudioGraph<T, BUF_SIZE>,
    _marker: PhantomData<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> GraphProcessor<T, BUF_SIZE> {
    /// Build a graph from a `GraphBuilder`, then wrap it.
    ///
    /// The builder is consumed and the graph is validated internally.
    pub fn from_builder(
        builder: GraphBuilder<T, BUF_SIZE>,
        clock: Box<dyn ClockSource>,
    ) -> Result<Self, rill_graph::BuildError> {
        let graph = builder.build(clock)?;
        Ok(Self {
            graph,
            _marker: PhantomData,
        })
    }

    /// Wrap an already-built `AudioGraph`.
    pub fn from_graph(graph: AudioGraph<T, BUF_SIZE>) -> Self {
        Self {
            graph,
            _marker: PhantomData,
        }
    }

    /// Access the inner graph for reading.
    pub fn graph(&self) -> &AudioGraph<T, BUF_SIZE> {
        &self.graph
    }

    /// Access the inner graph for mutation (only safe when not running).
    pub fn graph_mut(&mut self) -> &mut AudioGraph<T, BUF_SIZE> {
        &mut self.graph
    }

    /// Number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Topological processing order (indices into the node list).
    pub fn topo_order(&self) -> &[usize] {
        self.graph.topo_order()
    }

    /// Get an output buffer from a node port, if available.
    pub fn output_buffer(&self, node_idx: usize, port_idx: usize) -> Option<&[T; BUF_SIZE]> {
        self.graph.output_buffer(node_idx, port_idx)
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> AudioProcessor for GraphProcessor<T, BUF_SIZE> {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        self.graph.dispatch_set_parameters(&[]);
        let n = output.len().min(input.len());
        output[..n].copy_from_slice(&input[..n]);
    }

    fn reset(&mut self) {
        // Graph state will be reset when executor is integrated.
    }

    fn set_sample_rate(&mut self, _sample_rate: f32) {
        // Sample rate is set during graph construction.
    }
}
