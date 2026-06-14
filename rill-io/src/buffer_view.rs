//! Backend-specific BufferView implementations.
//!
//! Each backend provides its own `BufferView` adapter that encapsulates
//! per-backend rules (interleave/deinterleave semantics) for reading
//! input from and writing output to lock-free `IoRingBuffer`s.

use std::sync::Arc;

use rill_core::traits::buffer_view::BufferView;

use crate::buffer::IoRingBuffer;

/// Maximum interleaved buffer size: 1024 frames × 8 channels = 8192 floats.
/// Stack-allocated to avoid heap allocation in the RT path.
const MAX_INTERLEAVED: usize = 8192;

/// BufferView for interleaved backends (PipeWire, PortAudio, JACK, ALSA).
///
/// Deinterleaves on read, interleaves on write. The `IoRingBuffer` pair
/// bridges the I/O callback thread (where the backend fills/drains DMA)
/// and the graph processing (which operates on per-channel `FixedBuffer`s).
pub struct DeinterleavedView {
    input_ring: Arc<IoRingBuffer>,
    output_ring: Arc<IoRingBuffer>,
    num_input_channels: usize,
    num_output_channels: usize,
    block_size: usize,
}

impl DeinterleavedView {
    /// Create a new view wrapping the given ring buffer pair.
    ///
    /// `block_size` is the maximum number of frames per processing block.
    pub fn new(
        input_ring: Arc<IoRingBuffer>,
        output_ring: Arc<IoRingBuffer>,
        num_input_channels: usize,
        num_output_channels: usize,
        block_size: usize,
    ) -> Self {
        Self {
            input_ring,
            output_ring,
            num_input_channels,
            num_output_channels,
            block_size,
        }
    }
}

impl BufferView for DeinterleavedView {
    fn num_input_channels(&self) -> usize {
        self.num_input_channels
    }

    fn num_output_channels(&self) -> usize {
        self.num_output_channels
    }

    fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize {
        if self.num_input_channels == 0 || channel >= self.num_input_channels {
            dst.fill(0.0);
            return dst.len();
        }
        let n_frames = dst.len().min(self.block_size);
        let stride = self.num_input_channels;
        let needed = n_frames * stride;
        if needed == 0 {
            dst.fill(0.0);
            return 0;
        }
        // Stack-allocated temp buffer for interleaved read
        let cap = needed.min(MAX_INTERLEAVED);
        let mut interleaved = [0.0f32; MAX_INTERLEAVED];
        let read_samples = self.input_ring.read(&mut interleaved[..cap]);
        let read_frames = read_samples / stride;
        let frames = read_frames.min(n_frames);
        // Deinterleave into dst
        for i in 0..frames {
            dst[i] = interleaved[i * stride + channel];
        }
        for s in dst.iter_mut().skip(frames) {
            *s = 0.0;
        }
        frames
    }

    fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        if self.num_output_channels == 0 || channel >= self.num_output_channels {
            return src.len();
        }
        let n_frames = src.len().min(self.block_size);
        let stride = self.num_output_channels;
        let needed = n_frames * stride;
        if needed == 0 {
            return 0;
        }
        // Stack-allocated temp buffer for interleaved write
        let cap = needed.min(MAX_INTERLEAVED);
        let mut interleaved = [0.0f32; MAX_INTERLEAVED];
        // Interleave src into temp buffer
        for i in 0..n_frames {
            interleaved[i * stride + channel] = src[i];
        }
        let written_samples = self.output_ring.write(&interleaved[..cap]);
        written_samples / stride
    }
}
