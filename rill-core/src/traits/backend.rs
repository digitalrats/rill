//! # SignalBackend — abstract reactive stream backend
//!
//! Bridge between the signal graph and external I/O (hardware, network, …).
//! Carried in [`ProcessContext`](super::processable::ProcessContext) so
//! every node can access it without owning a reference.

/// Reactive stream backend for Source / Sink nodes.
///
/// Methods are called from the single processing thread. No locking or
/// synchronisation needed — the implementation owns its state.
pub trait SignalBackend {
    /// Read deinterleaved input (L, R). Returns frames read.
    fn read_input(&self, left: &mut [f32], right: &mut [f32]) -> usize;

    /// Write deinterleaved output (L, R). Returns frames written.
    fn write_output(&self, left: &[f32], right: &[f32]) -> usize;

    /// Start the backend (spawn worker thread / connect to hardware).
    fn start(&self) -> crate::traits::ProcessResult<()>;

    /// Stop the backend.
    fn stop(&self) -> crate::traits::ProcessResult<()>;
}
