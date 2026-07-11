//! Bridge backend trait for duplex execution boundaries.
//!
//! A bridge node splits the signal graph into left (recording) and right (playback)
//! sub-graphs. It maintains internal state across callbacks. Feedback is handled
//! externally via `feedback_read`/`feedback_write` annotations on graph nodes —
//! the bridge itself does not manage feedback.

use crate::math::Transcendental;
use crate::traits::ProcessResult;

/// A graph node that serves as a duplex boundary between recording and playback chains.
///
/// # Execution model
///
/// Each tick is split into five phases:
/// 1. ReadFeedback — mix feedback buffers into node inputs
/// 2. process_left — execute left sub-graph + bridge.process_left(inputs)
/// 3. process_right — bridge.process_right(outputs) + execute right sub-graph
/// 4. WriteFeedback — capture node outputs into feedback buffers
/// 5. Shadow copy — swap read/write feedback buffers
pub trait BridgeAlgorithm<T: Transcendental>: Send + Sync {
    /// Number of signal input channels.
    fn num_inputs(&self) -> usize;
    /// Number of signal output channels.
    fn num_outputs(&self) -> usize;

    /// Input callback: write into bridge state.
    fn process_left(&mut self, inputs: &[&[T]]) -> ProcessResult<()>;

    /// Output callback: read from bridge state.
    fn process_right(&mut self, outputs: &mut [&mut [T]]) -> ProcessResult<()>;

    /// Reset internal state.
    fn reset(&mut self);
}
