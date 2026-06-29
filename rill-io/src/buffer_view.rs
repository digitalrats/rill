//! Backend-specific BufferView implementations.
//!
//! `DirectView` provides zero-copy access to DMA buffers during the
//! I/O callback.  Raw pointers are valid only for the duration of the
//! `process_cb.call()` — no ring buffers, no extra copies.
//!
//! `DeinterleavedView` bridges `IoRingBuffer` pairs for interleaved backends
//! (PortAudio, ALSA) — deinterleaves on read, interleaves on write.

use std::sync::Arc;

use crate::buffer::IoRingBuffer;
use rill_core::traits::buffer_view::BufferView;

/// Maximum channel count for planar layouts.
const MAX_CH: usize = 8;

/// Zero-copy BufferView that reads/writes directly from/to DMA buffers.
///
/// Created inside the backend's process callback with raw pointers to
/// the hardware DMA buffers.  Pointers are valid only for the duration
/// of `process_cb.call(&tick)`.
///
/// Supports both interleaved (single ptr per direction, stride = channels)
/// and planar (per-channel ptrs, stride = 1) layouts.
pub struct DirectView {
    /// Interleaved input buffer (if planar, unused).
    in_ptr: *const f32,
    /// Interleaved output buffer (if planar, unused).
    out_ptr: *mut f32,
    /// Per-channel input pointers (if interleaved, unused).
    in_ptrs: [*const f32; MAX_CH],
    /// Per-channel output pointers (if interleaved, unused).
    out_ptrs: [*mut f32; MAX_CH],
    num_in: usize,
    num_out: usize,
    n_frames: usize,
    /// true = interleaved (use in_ptr/out_ptr), false = planar (use in_ptrs/out_ptrs).
    planar: bool,
}

// Safety: pointers are valid for the duration of the callback,
// single-threaded access inside graph processing.
unsafe impl Send for DirectView {}
unsafe impl Sync for DirectView {}

impl DirectView {
    /// Create a view for interleaved DMA buffers.
    ///
    /// `input` / `output` point to the start of the interleaved buffer.
    /// Stride is `num_channels`.
    pub fn new_interleaved(
        input: *const f32,
        output: *mut f32,
        num_input_channels: usize,
        num_output_channels: usize,
        n_frames: usize,
    ) -> Self {
        Self {
            in_ptr: input,
            out_ptr: output,
            in_ptrs: [std::ptr::null(); MAX_CH],
            out_ptrs: [std::ptr::null_mut(); MAX_CH],
            num_in: num_input_channels,
            num_out: num_output_channels,
            n_frames,
            planar: false,
        }
    }

    /// Create a view for planar (per-channel) DMA buffers.
    ///
    /// `inputs` / `outputs` are arrays of per-channel pointers.
    pub fn new_planar(inputs: &[*const f32], outputs: &[*mut f32], n_frames: usize) -> Self {
        let mut s = Self {
            in_ptr: std::ptr::null(),
            out_ptr: std::ptr::null_mut(),
            in_ptrs: [std::ptr::null(); MAX_CH],
            out_ptrs: [std::ptr::null_mut(); MAX_CH],
            num_in: inputs.len(),
            num_out: outputs.len(),
            n_frames,
            planar: true,
        };
        for (i, &p) in inputs.iter().enumerate() {
            if i < MAX_CH {
                s.in_ptrs[i] = p;
            }
        }
        for (i, &p) in outputs.iter().enumerate() {
            if i < MAX_CH {
                s.out_ptrs[i] = p;
            }
        }
        s
    }

    /// Create an output-only interleaved view (no input).
    pub fn new_output_only(output: *mut f32, num_channels: usize, n_frames: usize) -> Self {
        Self::new_interleaved(std::ptr::null(), output, 0, num_channels, n_frames)
    }
}

impl BufferView for DirectView {
    fn num_input_channels(&self) -> usize {
        self.num_in
    }

    fn num_output_channels(&self) -> usize {
        self.num_out
    }

    fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize {
        if self.num_in == 0 || channel >= self.num_in {
            dst.fill(0.0);
            return dst.len();
        }
        let n = dst.len().min(self.n_frames);
        if self.planar {
            unsafe {
                let src = std::slice::from_raw_parts(self.in_ptrs[channel], self.n_frames);
                dst[..n].copy_from_slice(&src[..n]);
            }
        } else {
            let stride = self.num_in;
            unsafe {
                for i in 0..n {
                    dst[i] = *self.in_ptr.add(i * stride + channel);
                }
            }
        }
        for s in dst.iter_mut().skip(n) {
            *s = 0.0;
        }
        n
    }

    fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        if self.num_out == 0 || channel >= self.num_out {
            return src.len();
        }
        let n = src.len().min(self.n_frames);
        if self.planar {
            unsafe {
                let dst = std::slice::from_raw_parts_mut(self.out_ptrs[channel], self.n_frames);
                dst[..n].copy_from_slice(&src[..n]);
            }
        } else {
            let stride = self.num_out;
            unsafe {
                for i in 0..n {
                    *self.out_ptr.add(i * stride + channel) = src[i];
                }
            }
        }
        n
    }
}

/// BufferView that bridges `IoRingBuffer` pairs for interleaved backends
/// (PortAudio, ALSA). Deinterleaves on read, interleaves on write.
pub struct DeinterleavedView {
    input_ring: Arc<IoRingBuffer>,
    output_ring: Arc<IoRingBuffer>,
    num_input_channels: usize,
    num_output_channels: usize,
    block_size: usize,
}

impl DeinterleavedView {
    /// Create a new deinterleaved view bridging input and output ring buffers.
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
        let n = dst.len().min(self.block_size);
        let stride = self.num_input_channels;
        let mut buf = vec![0.0f32; n * stride];
        let read = self.input_ring.read(&mut buf);
        for frame in 0..(read / stride).min(n) {
            dst[frame] = buf[frame * stride + channel];
        }
        (read / stride).min(n)
    }

    fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        let n = src.len().min(self.block_size);
        let stride = self.num_output_channels;
        let mut buf = vec![0.0f32; n * stride];
        for frame in 0..n {
            buf[frame * stride + channel] = src[frame];
        }
        self.output_ring.write(&buf)
    }
}
