use crate::math::Transcendental;
use std::fmt;

/// Fixed-size ring buffer (power-of-two size). Single-threaded.
///
/// Used inside the signal graph for delay effects and sample buffering.
/// Size must be a power of two.
///
/// # Example
/// ```
/// use rill_core::buffer::RingBuffer;
///
/// let mut buffer = RingBuffer::<f32, 4>::new();
/// buffer.write(1.0);
/// buffer.write(2.0);
/// assert_eq!(buffer.read_delayed(0), 2.0);
/// ```
#[repr(C, align(64))]
pub struct RingBuffer<T: Transcendental, const N: usize> {
    data: [T; N],
    head: usize,
    tail: usize,
    mask: usize,
    full: bool,
}

impl<T: Transcendental, const N: usize> RingBuffer<T, N> {
    /// Create a new ring buffer.
    ///
    /// # Panics
    /// Panics if `N` is not a power of two.
    pub fn new() -> Self {
        assert!(N.is_power_of_two(), "RingBuffer size must be power of two");
        Self {
            data: [T::ZERO; N],
            head: 0,
            tail: 0,
            mask: N - 1,
            full: false,
        }
    }

    /// Write a single sample, advancing the head cursor.
    pub fn write(&mut self, sample: T) {
        self.data[self.head] = sample;
        let next_head = (self.head + 1) & self.mask;
        self.head = next_head;
        if next_head == self.tail {
            self.full = true;
        }
    }

    /// Write multiple samples in sequence.
    pub fn write_slice(&mut self, samples: &[T])
    where
        T: Copy,
    {
        for &sample in samples {
            self.write(sample);
        }
    }

    /// Read the oldest sample, or `None` if empty.
    pub fn read(&mut self) -> Option<T> {
        if self.tail == self.head && !self.full {
            return None;
        }
        let sample = self.data[self.tail];
        self.tail = (self.tail + 1) & self.mask;
        self.full = false;
        Some(sample)
    }

    /// Read a sample at `delay` samples behind head (0 = most recent).
    ///
    /// # Panics
    /// Panics if `delay >= len()`.
    pub fn read_delayed(&self, delay: usize) -> T {
        assert!(delay < self.len(), "Delay must be less than buffer length");
        let read_pos = (self.head + self.capacity() - delay - 1) & self.mask;
        self.data[read_pos]
    }

    /// Read with linear interpolation between samples at fractional delay.
    pub fn read_interpolated(&self, delay_frac: f32) -> T
    where
        T: From<f32> + Into<f32>,
    {
        let delay_int = delay_frac.floor() as usize;
        let frac = delay_frac.fract();
        if frac == 0.0 {
            return self.read_delayed(delay_int);
        }
        let s1: f32 = self.read_delayed(delay_int).into();
        let prev = if delay_int == 0 {
            self.len() - 1
        } else {
            delay_int - 1
        };
        let s2: f32 = self.read_delayed(prev).into();
        T::from(s1 * (1.0 - frac) + s2 * frac)
    }

    /// Read a sequence of interpolated samples into the output buffer,
    /// starting at `start_delay` samples behind head.
    pub fn read_sequence_interpolated(&self, start_delay: f32, output: &mut [T])
    where
        T: From<f32> + Into<f32>,
    {
        let len = self.len();
        for i in 0..output.len() {
            let delay = start_delay + i as f32;
            output[i] = if delay < len as f32 {
                self.read_interpolated(delay)
            } else {
                T::ZERO
            };
        }
    }

    /// Number of samples currently stored.
    pub fn len(&self) -> usize {
        if self.full {
            N
        } else if self.head >= self.tail {
            self.head - self.tail
        } else {
            N - self.tail + self.head
        }
    }

    /// Maximum capacity (const generic parameter).
    pub const fn capacity(&self) -> usize {
        N
    }
    /// Whether the buffer has no samples.
    pub fn is_empty(&self) -> bool {
        self.head == self.tail && !self.full
    }
    /// Whether the buffer is completely full.
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Clear all samples and reset cursors.
    pub fn clear(&mut self) {
        self.data.fill(T::ZERO);
        self.head = 0;
        self.tail = 0;
        self.full = false;
    }

    /// Reset cursors without zeroing the data.
    pub fn reset(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.full = false;
    }
}

impl<T: Transcendental, const N: usize> Default for RingBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental + fmt::Debug, const N: usize> fmt::Debug for RingBuffer<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut preview = Vec::with_capacity(4);
        for i in 0..4.min(N) {
            preview.push(self.data[i]);
        }
        f.debug_struct("RingBuffer")
            .field("head", &self.head)
            .field("tail", &self.tail)
            .field("full", &self.full)
            .field("len", &self.len())
            .field("capacity", &N)
            .field("preview", &preview)
            .finish()
    }
}

// =============================================================================
// Итератор
// =============================================================================

/// Iterator over the contents of a [`RingBuffer`], from oldest to newest.
pub struct RingBufferIter<'a, T: Transcendental, const N: usize> {
    buffer: &'a RingBuffer<T, N>,
    pos: usize,
    end: usize,
}

impl<'a, T: Transcendental, const N: usize> RingBufferIter<'a, T, N> {
    fn new(buffer: &'a RingBuffer<T, N>) -> Self {
        let tail = buffer.tail;
        let head = buffer.head;
        let len = if buffer.full {
            N
        } else if head >= tail {
            head - tail
        } else {
            N - tail + head
        };
        Self {
            buffer,
            pos: tail,
            end: tail + len,
        }
    }
}

impl<'a, T: Transcendental, const N: usize> Iterator for RingBufferIter<'a, T, N> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.end {
            None
        } else {
            let idx = self.pos & self.buffer.mask;
            let value = self.buffer.data[idx];
            self.pos += 1;
            Some(value)
        }
    }
}

impl<'a, T: Transcendental, const N: usize> ExactSizeIterator for RingBufferIter<'a, T, N> {
    fn len(&self) -> usize {
        self.end - self.pos
    }
}

impl<T: Transcendental, const N: usize> RingBuffer<T, N> {
    /// Iterate over buffered samples from oldest to newest.
    pub fn iter(&self) -> RingBufferIter<'_, T, N> {
        RingBufferIter::new(self)
    }
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_basic() {
        let mut buffer = RingBuffer::<f32, 4>::new();
        buffer.write(1.0);
        buffer.write(2.0);
        buffer.write(3.0);
        buffer.write(4.0);
        assert!(buffer.is_full());
        assert_eq!(buffer.len(), 4);
        assert_eq!(buffer.read(), Some(1.0));
        assert_eq!(buffer.read(), Some(2.0));
        assert_eq!(buffer.read(), Some(3.0));
        assert_eq!(buffer.read(), Some(4.0));
        assert_eq!(buffer.read(), None);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_ring_buffer_wraparound() {
        let mut buffer = RingBuffer::<f32, 4>::new();
        for i in 0..10 {
            buffer.write(i as f32);
        }
        assert_eq!(buffer.read_delayed(0), 9.0);
        assert_eq!(buffer.read_delayed(1), 8.0);
        assert_eq!(buffer.read_delayed(2), 7.0);
        assert_eq!(buffer.read_delayed(3), 6.0);
    }

    #[test]
    fn test_ring_buffer_interpolated() {
        let mut buffer = RingBuffer::<f32, 4>::new();
        buffer.write(1.0);
        buffer.write(2.0);
        buffer.write(3.0);
        buffer.write(4.0);
        let val = buffer.read_interpolated(1.5);
        assert!((val - 3.5).abs() < 0.001);
    }

    #[test]
    fn test_ring_buffer_clear() {
        let mut buffer = RingBuffer::<f32, 4>::new();
        buffer.write(1.0);
        buffer.write(2.0);
        assert!(!buffer.is_empty());
        buffer.clear();
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_ring_buffer_iterator() {
        let mut buffer = RingBuffer::<f32, 4>::new();
        buffer.write(1.0);
        buffer.write(2.0);
        buffer.write(3.0);
        buffer.write(4.0);
        let collected: Vec<f32> = buffer.iter().collect();
        assert_eq!(collected, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_ring_buffer_read_sequence() {
        let mut buffer = RingBuffer::<f32, 4>::new();
        buffer.write(1.0);
        buffer.write(2.0);
        buffer.write(3.0);
        buffer.write(4.0);
        let mut output = [0.0; 4];
        buffer.read_sequence_interpolated(0.0, &mut output);
        assert_eq!(output, [4.0, 3.0, 2.0, 1.0]);
    }

    #[test]
    #[should_panic(expected = "Delay must be less than buffer length")]
    fn test_ring_buffer_invalid_delay() {
        let buffer = RingBuffer::<f32, 4>::new();
        let _ = buffer.read_delayed(4);
    }

    #[test]
    fn test_ring_buffer_write_slice() {
        let mut buffer = RingBuffer::<f32, 4>::new();
        buffer.write_slice(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(buffer.read(), Some(1.0));
        assert_eq!(buffer.read(), Some(2.0));
        assert_eq!(buffer.read(), Some(3.0));
        assert_eq!(buffer.read(), Some(4.0));
    }
}
