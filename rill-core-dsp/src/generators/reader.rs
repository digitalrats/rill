//! Fractional-position buffer reader with interpolation.
//!
//! Provides [`InterpolatedReader`] for reading from heap-allocated sample
//! buffers with linear or cubic interpolation, wrap/clamp boundary modes,
//! and variable playback rate.

use rill_core::interpolate::Interpolate;
use rill_core::Transcendental;

fn len_remainder(pos: f64, len: f64) -> f64 {
    let r = pos % len;
    if r < 0.0 { r + len } else { r }
}

/// Heap-allocated buffer reader with fractional-position interpolation.
///
/// When `wrap` is true the position wraps modulo `len` (periodic / wavetable mode).
/// When `wrap` is false the position clamps at buffer boundaries (sample mode).
pub struct InterpolatedReader<T> {
    buffer: Box<[T]>,
    position: f64,
    rate: f64,
    cubic: bool,
    wrap: bool,
}

impl<T: Transcendental + Copy> InterpolatedReader<T> {
    /// Create a new reader from a `Vec<T>`.
    ///
    /// The reader starts at position 0 with unit rate and linear interpolation.
    pub fn new(buffer: Vec<T>) -> Self {
        Self {
            buffer: buffer.into_boxed_slice(),
            position: 0.0,
            rate: 1.0,
            cubic: false,
            wrap: false,
        }
    }

    /// Create a new reader from a pre-allocated `Box<[T]>`.
    pub fn from_boxed(buffer: Box<[T]>) -> Self {
        Self {
            buffer,
            position: 0.0,
            rate: 1.0,
            cubic: false,
            wrap: false,
        }
    }

    /// Return the number of samples in the buffer.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns `true` if the buffer is empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Current read position in samples (fractional).
    #[inline(always)]
    pub fn position(&self) -> f64 {
        self.position
    }

    /// Set the read position (in samples).
    #[inline(always)]
    pub fn set_position(&mut self, pos: f64) {
        self.position = pos;
    }

    /// Playback rate in samples per output sample (1.0 = normal speed).
    #[inline(always)]
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Set the playback rate.
    #[inline(always)]
    pub fn set_rate(&mut self, rate: f64) {
        self.rate = rate;
    }

    /// Returns `true` if cubic Hermite interpolation is enabled.
    #[inline(always)]
    pub fn is_cubic(&self) -> bool {
        self.cubic
    }

    /// Enable (`true`) or disable (`false`) cubic Hermite interpolation.
    #[inline(always)]
    pub fn set_cubic(&mut self, cubic: bool) {
        self.cubic = cubic;
    }

    /// Returns `true` when the read position wraps at buffer boundaries.
    #[inline(always)]
    pub fn is_wrap(&self) -> bool {
        self.wrap
    }

    /// Enable (`true`) or disable (`false`) wrap mode (periodic / wavetable).
    #[inline(always)]
    pub fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
    }

    /// Replace the internal buffer and reset the position to 0.
    pub fn set_buffer(&mut self, buffer: Vec<T>) {
        self.buffer = buffer.into_boxed_slice();
        self.position = 0.0;
    }

    /// Return the internal buffer as an immutable slice.
    #[inline(always)]
    pub fn as_slice(&self) -> &[T] {
        &self.buffer
    }

    /// Wrap-aware linear interpolation.
    fn read_wrap_linear(&self, pos: f64) -> T {
        let len = self.len();
        let i0 = pos.floor() as usize % len;
        let i1 = (i0 + 1) % len;
        let frac = T::from_f64(pos.fract());
        let a = self.buffer[i0];
        let b = self.buffer[i1];
        a + (b - a) * frac
    }

    /// Wrap-aware cubic Hermite interpolation (periodic boundary).
    fn read_wrap_cubic(&self, pos: f64) -> T {
        let len = self.len();
        let i = pos.floor() as usize;
        let i0 = (i + len - 1) % len;
        let i1 = i % len;
        let i2 = (i + 1) % len;
        let i3 = (i + 2) % len;
        let frac = T::from_f64(pos.fract());

        let c0 = self.buffer[i1];
        let c1 = (self.buffer[i2] - self.buffer[i0]) * T::from_f32(0.5);
        let c2 = self.buffer[i0] * T::from_f32(-1.5)
            + self.buffer[i1] * T::from_f32(2.0)
            + self.buffer[i2] * T::from_f32(-0.5);
        let c3 = self.buffer[i0] * T::from_f32(-0.5)
            + self.buffer[i1] * T::from_f32(1.5)
            + self.buffer[i2] * T::from_f32(-1.5)
            + self.buffer[i3] * T::from_f32(0.5);

        let f2 = frac * frac;
        let f3 = f2 * frac;
        c0 + c1 * frac + c2 * f2 + c3 * f3
    }

    /// Read a single sample at the current position without advancing.
    #[inline(always)]
    pub fn read_one(&self) -> T {
        if self.is_empty() {
            return T::ZERO;
        }
        let pos = if self.wrap {
            len_remainder(self.position, self.len() as f64)
        } else {
            self.position
        };
        if self.cubic && self.len() >= 4 {
            if self.wrap {
                self.read_wrap_cubic(pos)
            } else {
                self.buffer.interpolate_cubic(pos)
            }
        } else if self.wrap {
            self.read_wrap_linear(pos)
        } else {
            self.buffer.interpolate_linear(pos)
        }
    }

    /// Advance position by `self.rate` (one sample).
    #[inline(always)]
    pub fn advance(&mut self) {
        self.position += self.rate;
    }

    /// Read the next block of samples, advancing `self.position`.
    pub fn render_block(&mut self, output: &mut [T]) {
        if self.is_empty() {
            for s in output.iter_mut() {
                *s = T::ZERO;
            }
            return;
        }

        if self.wrap {
            let len_f = self.len() as f64;
            if self.cubic && self.len() >= 4 {
                for s in output.iter_mut() {
                    *s = self.read_wrap_cubic(len_remainder(self.position, len_f));
                    self.position += self.rate;
                }
            } else {
                for s in output.iter_mut() {
                    *s = self.read_wrap_linear(len_remainder(self.position, len_f));
                    self.position += self.rate;
                }
            }
        } else if self.cubic {
            for s in output.iter_mut() {
                *s = self.buffer.interpolate_cubic(self.position);
                self.position += self.rate;
            }
        } else {
            for s in output.iter_mut() {
                *s = self.buffer.interpolate_linear(self.position);
                self.position += self.rate;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_read() {
        let buf = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let mut reader = InterpolatedReader::new(buf);
        let mut out = [0.0f64; 4];
        reader.render_block(&mut out);
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0]);
        assert!((reader.position() - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_rate_half() {
        let buf = vec![0.0, 2.0, 4.0, 6.0, 8.0];
        let mut reader = InterpolatedReader::new(buf);
        reader.set_rate(0.5);
        let mut out = [0.0f64; 4];
        reader.render_block(&mut out);
        assert!((out[0] - 0.0).abs() < 1e-10);
        assert!((out[1] - 1.0).abs() < 1e-10);
        assert!((out[2] - 2.0).abs() < 1e-10);
        assert!((out[3] - 3.0).abs() < 1e-10);
        assert!((reader.position() - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_empty_buffer() {
        let buf: Vec<f64> = vec![];
        let mut reader = InterpolatedReader::new(buf);
        let mut out = [1.0f64; 4];
        reader.render_block(&mut out);
        assert_eq!(out, [0.0; 4]);
    }

    #[test]
    fn test_set_buffer() {
        let buf = vec![0.0, 0.0];
        let mut reader = InterpolatedReader::new(buf);
        reader.set_position(10.0);
        reader.set_buffer(vec![10.0, 20.0, 30.0]);
        assert_eq!(reader.position(), 0.0);
        assert_eq!(reader.len(), 3);
    }

    #[test]
    fn test_wrap_linear() {
        let buf = vec![0.0, 1.0, 2.0, 3.0];
        let mut reader = InterpolatedReader::new(buf);
        reader.set_wrap(true);
        reader.set_position(3.5);
        let mut out = [0.0f64; 2];
        reader.render_block(&mut out);
        assert!((out[0] - 1.5).abs() < 1e-10, "wrap 3.5 -> 1.5, got {}", out[0]);
        assert!((out[1] - 0.5).abs() < 1e-10, "wrap 4.5 -> 0.5, got {}", out[1]);
    }

    #[test]
    fn test_wrap_cubic_at_boundary() {
        let buf = vec![0.0, 1.0, 2.0, 3.0];
        let mut reader = InterpolatedReader::new(buf);
        reader.set_wrap(true);
        reader.set_cubic(true);
        reader.set_position(0.0);
        let mut out = [0.0f64; 1];
        reader.render_block(&mut out);
        assert!((out[0] - 0.0).abs() < 1e-10, "cubic wrap at 0 -> 0, got {}", out[0]);
    }

    #[test]
    fn test_clamp_at_end() {
        let buf = vec![10.0, 20.0];
        let mut reader = InterpolatedReader::new(buf);
        reader.set_position(5.0);
        let mut out = [0.0f64; 3];
        reader.render_block(&mut out);
        assert_eq!(out, [20.0, 20.0, 20.0]);
    }
}
