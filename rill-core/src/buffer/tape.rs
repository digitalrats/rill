use crate::buffer::Buffer;
use crate::math::Transcendental;

/// Heap-allocated ring buffer for tape delay — single-threaded.
///
/// Unlike [`DelayLine`](super::DelayLine), `TapeLoop` does NOT use const generics
/// for its capacity — the buffer is allocated on the heap at runtime.
/// This allows arbitrarily large delay lines (millions of samples) without
/// stack overflow.
///
/// # Thread safety
///
/// `TapeLoop` is **not** thread-safe. It uses plain `T`, not `AtomicCell`,
/// because it is only accessed from the single audio thread.
///
/// # Example
///
/// ```rust
/// use rill_core::buffer::TapeLoop;
///
/// let mut tape = TapeLoop::<f32>::new(96000).unwrap();
/// tape.write(0.5);
/// let sample = tape.read(100);
/// ```
#[derive(Debug)]
pub struct TapeLoop<T> {
    buffer: Box<[T]>,
    capacity: usize,
    write_pos: usize,
}

impl<T: Transcendental> TapeLoop<T> {
    /// Allocate a new tape loop with the given capacity (in samples).
    pub fn new(capacity: usize) -> Option<Self> {
        if capacity == 0 {
            return None;
        }
        let mut vec = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            vec.push(T::ZERO);
        }
        Some(Self {
            buffer: vec.into_boxed_slice(),
            capacity,
            write_pos: 0,
        })
    }

    /// Maximum capacity in samples.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    /// Current write cursor position.
    pub fn write_pos(&self) -> usize {
        self.write_pos
    }

    /// Write a single sample and advance the write cursor.
    #[inline(always)]
    pub fn write(&mut self, sample: T) {
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % self.capacity;
    }

    /// Read a sample at `delay` samples behind the write position.
    #[inline(always)]
    pub fn read(&self, delay: usize) -> T {
        let d = delay.min(self.capacity - 1);
        let read_pos = if self.write_pos > d {
            self.write_pos - 1 - d
        } else {
            self.capacity + self.write_pos - 1 - d
        };
        self.buffer[read_pos]
    }

    /// Read with linear interpolation between samples.
    #[inline(always)]
    pub fn read_interpolated(&self, delay: f64) -> T {
        let d = delay as usize;
        let frac = T::from_f64(delay.fract());
        let s1 = self.read(d);
        let s2 = self.read(d + 1);
        s1 + (s2 - s1) * frac
    }

    /// Write a full block of samples.
    #[inline(always)]
    pub fn write_block(&mut self, block: &[T]) {
        let len = block.len().min(self.capacity);
        for (i, &b) in block.iter().enumerate().take(len) {
            self.buffer[(self.write_pos + i) % self.capacity] = b;
        }
        self.write_pos = (self.write_pos + len) % self.capacity;
    }

    /// Read a full block starting at `delay` samples behind write position.
    #[inline(always)]
    pub fn read_block(&self, delay: usize, output: &mut [T]) {
        let len = output.len().min(self.capacity);
        let d = delay.min(self.capacity - 1);
        for (i, out) in output.iter_mut().enumerate().take(len) {
            *out = self.read(d + len - 1 - i);
        }
    }

    /// Fill the entire buffer with a constant value.
    pub fn fill(&mut self, value: T) {
        for slot in self.buffer.iter_mut() {
            *slot = value;
        }
    }

    /// Reset write position and zero the buffer.
    pub fn clear(&mut self) {
        for slot in self.buffer.iter_mut() {
            *slot = T::ZERO;
        }
        self.write_pos = 0;
    }
}

// ── Buffer trait impl ──────────────────────────────────────────────

impl<T: Transcendental> Buffer<T> for TapeLoop<T> {
    fn capacity(&self) -> usize {
        self.capacity
    }

    fn len(&self) -> usize {
        self.capacity
    }

    fn as_slice(&self) -> &[T] {
        &self.buffer
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.buffer
    }

    fn fill(&mut self, value: T) {
        for slot in self.buffer.iter_mut() {
            *slot = value;
        }
    }

    fn copy_from(&mut self, src: &[T]) {
        let len = src.len().min(self.capacity);
        self.buffer[..len].copy_from_slice(&src[..len]);
    }

    fn clear(&mut self) {
        for slot in self.buffer.iter_mut() {
            *slot = T::ZERO;
        }
        self.write_pos = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tape_basic_write_read() {
        let mut tape = TapeLoop::<f32>::new(1024).unwrap();
        tape.write(1.0);
        tape.write(2.0);
        tape.write(3.0);
        assert_eq!(tape.read(0), 3.0);
        assert_eq!(tape.read(1), 2.0);
        assert_eq!(tape.read(2), 1.0);
    }

    #[test]
    fn test_tape_wraparound() {
        let mut tape = TapeLoop::<f32>::new(4).unwrap();
        for i in 0..10 {
            tape.write(i as f32);
        }
        assert_eq!(tape.read(0), 9.0);
        assert_eq!(tape.read(1), 8.0);
        assert_eq!(tape.read(2), 7.0);
        assert_eq!(tape.read(3), 6.0);
    }

    #[test]
    fn test_tape_block_ops() {
        let mut tape = TapeLoop::<f32>::new(64).unwrap();
        let block = [1.0f32; 64];
        tape.write_block(&block);
        let mut out = [0.0f32; 64];
        tape.read_block(63, &mut out);
        assert_eq!(out[0], 1.0);
    }

    #[test]
    fn test_tape_large_capacity() {
        let tape = TapeLoop::<f32>::new(1_000_000).unwrap();
        assert_eq!(tape.capacity(), 1_000_000);
    }

    #[test]
    fn test_tape_zero_capacity() {
        assert!(TapeLoop::<f32>::new(0).is_none());
    }

    #[test]
    fn test_read_interpolated() {
        let mut tape = TapeLoop::<f32>::new(1024).unwrap();
        tape.write(0.0);
        tape.write(1.0);
        let v = tape.read_interpolated(0.5);
        assert!((v - 0.5).abs() < 0.01);
    }
}
