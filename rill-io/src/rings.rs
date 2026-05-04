//! Shared ring buffer handles for PipeWire I/O.
//!
//! PipeWire uses a push model — its RT callbacks read/write ring buffers.
//! The audio thread reads/writes the same rings during `process_block()`:
//!
//! ```text
//! PW input callback ──write──→ input_ring ──read──→ AudioInput::generate()
//! AudioOutput::consume() ──write──→ output_ring ──read──→ PW output callback
//! ```
//!
//! [`PwBuffers`] holds both rings.

use std::sync::Arc;

use crate::buffer::IoRingBuffer;

/// Input and output ring buffers shared between PipeWire and graph nodes.
/// Input and output ring buffers shared between PipeWire and graph nodes.
pub struct PwBuffers {
    /// Ring buffer written by the PipeWire input callback and read by `AudioInput::generate()`.
    pub input: Arc<parking_lot::RwLock<IoRingBuffer>>,
    /// Ring buffer written by `AudioOutput::consume()` and read by the PipeWire output callback.
    pub output: Arc<parking_lot::RwLock<IoRingBuffer>>,
}
