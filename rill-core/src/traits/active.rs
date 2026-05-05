use crate::queues::{MpscQueue, SetParameter};

/// Handle passed to an [`ActiveNode`] implementation.
///
/// The `nodes` pointer points to the graph's node array cast to `()`.
/// Concrete implementations cast back to `[NodeVariant<f32, B>]`.
///
/// `queue` points to the shared command queue that the audio callback
/// must drain before each processing cycle.
pub struct GraphHandle {
    pub nodes: *mut u8,
    pub len: usize,
    pub source_idx: usize,
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
    fn start(&mut self, handle: GraphHandle);
    fn stop(&mut self);
}
