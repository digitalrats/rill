//! Backend-specific BufferView implementations.
//!
//! Each backend provides its own `BufferView` adapter that encapsulates
//! per-backend rules (interleave/deinterleave semantics) for reading
//! input from and writing output to lock-free `IoRingBuffer`s.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rill_core::traits::buffer_view::BufferView;

use crate::buffer::IoRingBuffer;

/// Maximum interleaved buffer size: 1024 frames × 8 channels = 8192 floats.
const MAX_INTERLEAVED: usize = 8192;

/// BufferView for interleaved backends (PipeWire, PortAudio, JACK, ALSA).
///
/// Input: the first `read_input` call of a block drains `input_ring` and
/// caches the interleaved data; subsequent calls within the same block
/// serve from the cache.
///
/// Output: `write_output` calls accumulate per-channel data into a shared
/// interleaved buffer.  The buffer is flushed to `output_ring` when all
/// channels have been written for the block.
pub struct DeinterleavedView {
    input_ring: Arc<IoRingBuffer>,
    output_ring: Arc<IoRingBuffer>,
    num_input_channels: usize,
    num_output_channels: usize,
    block_size: usize,
    /// Input cache: interleaved data + byte length.
    in_cache: UnsafeCell<([f32; MAX_INTERLEAVED], usize)>,
    /// Output cache: interleaved accumulation buffer + channels written count.
    out_cache: UnsafeCell<([f32; MAX_INTERLEAVED], usize)>,
    out_channels_written: AtomicUsize,
}

// Safety: DeinterleavedView is used single-threaded inside the graph
// processing callback.
unsafe impl Send for DeinterleavedView {}
unsafe impl Sync for DeinterleavedView {}

impl DeinterleavedView {
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
            in_cache: UnsafeCell::new(([0.0f32; MAX_INTERLEAVED], 0)),
            out_cache: UnsafeCell::new(([0.0f32; MAX_INTERLEAVED], 0)),
            out_channels_written: AtomicUsize::new(0),
        }
    }

    fn ensure_input_cache(&self) -> &[f32] {
        unsafe {
            let (ref mut buf, ref mut len) = *self.in_cache.get();
            let has_new = !self.input_ring.is_empty();
            if has_new || *len == 0 {
                if self.num_input_channels > 0 {
                    let needed = (self.block_size * self.num_input_channels).min(MAX_INTERLEAVED);
                    *len = self.input_ring.read(&mut buf[..needed]);
                }
            }
            &buf[..*len]
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
        let stride = self.num_input_channels;
        let interleaved = self.ensure_input_cache();
        let total_frames = if stride > 0 {
            interleaved.len() / stride
        } else {
            0
        };
        let n_frames = dst.len().min(total_frames);
        for i in 0..n_frames {
            dst[i] = interleaved[i * stride + channel];
        }
        for s in dst.iter_mut().skip(n_frames) {
            *s = 0.0;
        }
        n_frames
    }

    fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        if self.num_output_channels == 0 || channel >= self.num_output_channels {
            return src.len();
        }
        let n_frames = src.len().min(self.block_size);
        let stride = self.num_output_channels;
        if n_frames == 0 || stride == 0 {
            return 0;
        }

        // Accumulate into shared interleaved output buffer
        unsafe {
            let (ref mut buf, ref mut len) = *self.out_cache.get();
            let needed = n_frames * stride;
            if *len == 0 {
                // Clear the cache for a new block
                for v in buf[..needed.min(MAX_INTERLEAVED)].iter_mut() {
                    *v = 0.0;
                }
            }
            for i in 0..n_frames {
                buf[i * stride + channel] = src[i];
            }
            *len = needed;

            let prev = self.out_channels_written.fetch_add(1, Ordering::Relaxed);
            // Last channel flushes the buffer to the ring
            if prev + 1 >= self.num_output_channels {
                let written = self.output_ring.write(&buf[..needed.min(MAX_INTERLEAVED)]);
                *len = 0;
                self.out_channels_written.store(0, Ordering::Relaxed);
                return written / stride;
            }
        }
        n_frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deinterleaved_view_read_input() {
        let input_ring = Arc::new(IoRingBuffer::new(64));
        let output_ring = Arc::new(IoRingBuffer::new(64));
        let view = DeinterleavedView::new(input_ring.clone(), output_ring, 2, 0, 8);

        let interleaved: [f32; 16] = [
            0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        input_ring.write(&interleaved);

        let mut left = [0.0f32; 8];
        let mut right = [0.0f32; 8];
        let n_left = view.read_input(0, &mut left);
        let n_right = view.read_input(1, &mut right);

        assert_eq!(n_left, 8);
        assert_eq!(n_right, 8);
        assert_eq!(left, [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0]);
        assert_eq!(right, [1.0, 3.0, 5.0, 7.0, 9.0, 11.0, 13.0, 15.0]);
    }

    #[test]
    fn test_deinterleaved_view_write_output() {
        let input_ring = Arc::new(IoRingBuffer::new(64));
        let output_ring = Arc::new(IoRingBuffer::new(64));
        let view = DeinterleavedView::new(input_ring, output_ring.clone(), 0, 2, 8);

        let left = [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0_f32];
        let right = [1.0, 3.0, 5.0, 7.0, 9.0, 11.0, 13.0, 15.0_f32];
        view.write_output(0, &left);
        view.write_output(1, &right);

        let mut interleaved = [0.0f32; 16];
        output_ring.read(&mut interleaved);
        assert_eq!(
            interleaved,
            [
                0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0,
                15.0
            ]
        );
    }

    #[test]
    fn test_roundtrip_push_pull() {
        let input_ring = Arc::new(IoRingBuffer::new(256));
        let output_ring = Arc::new(IoRingBuffer::new(256));
        let view = DeinterleavedView::new(input_ring.clone(), output_ring.clone(), 2, 2, 8);

        // Push: interleaved input
        let stereo_in: [f32; 16] = [
            0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6,
        ];
        input_ring.write(&stereo_in);

        // Read per-channel (graph input)
        let mut lb = [0.0f32; 8];
        let mut rb = [0.0f32; 8];
        view.read_input(0, &mut lb);
        view.read_input(1, &mut rb);

        // Identity: write same data (graph output)
        view.write_output(0, &lb);
        view.write_output(1, &rb);

        // Pull: interleaved output
        let mut stereo_out = [0.0f32; 16];
        output_ring.read(&mut stereo_out);
        assert_eq!(stereo_out, stereo_in);
    }

    #[test]
    fn test_push_pull_multiple_blocks() {
        let input_ring = Arc::new(IoRingBuffer::new(256));
        let output_ring = Arc::new(IoRingBuffer::new(256));
        let view = DeinterleavedView::new(input_ring.clone(), output_ring.clone(), 2, 2, 4);

        for block in 0..3 {
            let base = (block * 8) as f32;
            let in_data: [f32; 8] = [
                base,
                base + 1.0,
                base + 2.0,
                base + 3.0,
                base + 4.0,
                base + 5.0,
                base + 6.0,
                base + 7.0,
            ];
            input_ring.write(&in_data);

            let mut lb = [0.0f32; 4];
            let mut rb = [0.0f32; 4];
            view.read_input(0, &mut lb);
            view.read_input(1, &mut rb);

            assert_eq!(lb, [base, base + 2.0, base + 4.0, base + 6.0]);
            assert_eq!(rb, [base + 1.0, base + 3.0, base + 5.0, base + 7.0]);

            view.write_output(0, &lb);
            view.write_output(1, &rb);

            let mut out_data = [0.0f32; 8];
            output_ring.read(&mut out_data);
            assert_eq!(out_data, in_data);
        }
    }
}
