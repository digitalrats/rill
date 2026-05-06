use crate::queues::{MpscQueue, SetParameter};

/// Handle passed to an [`ActiveNode`] implementation.
///
/// The `nodes` pointer points to the graph's node array cast to `()`.
/// Concrete implementations cast back to `[NodeVariant<f32, B>]`.
///
/// `queue` points to the shared command queue that the audio callback
/// must drain before each processing cycle.
pub struct GraphHandle {
    /// Raw pointer to the graph's node array (cast to `u8`).
    pub nodes: *mut u8,
    /// Number of nodes in the graph.
    pub len: usize,
    /// Index of the source node in the graph.
    pub source_idx: usize,
    /// Sample rate of the audio stream.
    pub sample_rate: f32,
    /// Command queue (control → audio thread).
    pub queue: *const MpscQueue<SetParameter>,
}

/// A node that drives graph processing.
///
/// The implementation receives a [`GraphHandle`] in [`start`](Self::start).
/// It must drain the command queue at the beginning of every processing
/// cycle, apply parameters to the graph nodes, then run the signal DAG.
///
/// # Safety
///
/// The handle is valid only until the corresponding [`stop`](Self::stop)
/// returns.
pub trait ActiveNode {
    /// Start the node with the given graph handle.
    ///
    /// Called once before the audio thread begins processing.
    fn start(&mut self, handle: GraphHandle);
    /// Stop the node.
    ///
    /// Called after the audio thread has stopped.
    fn stop(&mut self);
}
