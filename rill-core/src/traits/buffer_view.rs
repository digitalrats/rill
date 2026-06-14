//! # BufferView — backend-specific accessor for I/O ring buffers
//!
//! A `BufferView` encapsulates per-backend rules (interleave/deinterleave,
//! stride, alignment) for reading input samples from and writing output
//! samples to cross-thread ring buffers.
//!
//! Each backend provides its own implementation. Nodes use this trait
//! uniformly via `tick.view.read_input(ch, buf)` and
//! `tick.view.write_output(ch, buf)`.

/// Backend-specific accessor for I/O ring buffers.
///
/// Encapsulates per-backend rules (interleave/deinterleave) for reading
/// input samples from and writing output samples to cross-thread ring buffers.
/// Each backend provides its own implementation.
pub trait BufferView: Send + Sync {
    /// Number of input (capture) channels.
    fn num_input_channels(&self) -> usize;

    /// Number of output (playback) channels.
    fn num_output_channels(&self) -> usize;

    /// Read available input samples for one channel into `dst`.
    ///
    /// Returns the number of samples actually read (may be less than `dst.len()`
    /// if insufficient data is available).
    fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize;

    /// Write output samples for one channel from `src`.
    ///
    /// Returns the number of samples actually written (may be less than `src.len()`
    /// if insufficient space is available).
    fn write_output(&self, channel: usize, src: &[f32]) -> usize;
}

/// No-op BufferView for testing and default initialization.
///
/// Fills input with zeros and discards output.
pub struct NullBufferView {
    num_input_channels: usize,
    num_output_channels: usize,
}

impl NullBufferView {
    /// Create a new null view with the given channel counts.
    pub fn new(num_input_channels: usize, num_output_channels: usize) -> Self {
        Self {
            num_input_channels,
            num_output_channels,
        }
    }
}

impl BufferView for NullBufferView {
    fn num_input_channels(&self) -> usize {
        self.num_input_channels
    }

    fn num_output_channels(&self) -> usize {
        self.num_output_channels
    }

    fn read_input(&self, _channel: usize, dst: &mut [f32]) -> usize {
        let n = dst.len();
        dst.fill(0.0);
        n
    }

    fn write_output(&self, _channel: usize, _src: &[f32]) -> usize {
        _src.len()
    }
}
