use crate::buffer::{AtomicStats, SignalBuffer, BufferStats};
use crate::math::Transcendental;
use core::marker::PhantomData;
use core::ops::{Index, IndexMut};

/// Delay line for audio effects — single-threaded circular buffer.
///
/// Provides a circular buffer for implementing delay, reverb,
/// and other time-based effects.
///
/// # Thread safety
///
/// `DelayLine` is **not** thread-safe. It is designed for single-threaded
/// use inside the audio graph. Use [`PipeBuffer`](super::PipeBuffer) or
/// [`RingBuffer`](super::RingBuffer) for cross-thread communication.
#[repr(align(64))]
pub struct DelayLine<T: Transcendental, const MAX_DELAY: usize> {
    buffer: [T; MAX_DELAY],
    write_pos: usize,
    delay_samples: usize,
    sample_rate: f32,
    stats: AtomicStats,
    _phantom: PhantomData<T>,
}

impl<T: Transcendental, const MAX_DELAY: usize> DelayLine<T, MAX_DELAY> {
    pub fn new(sample_rate: f32) -> Self {
        assert!(MAX_DELAY > 0, "DelayLine must have MAX_DELAY > 0");
        Self {
            buffer: [T::ZERO; MAX_DELAY],
            write_pos: 0,
            delay_samples: 0,
            sample_rate,
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }

    pub fn set_delay(&mut self, delay_sec: f32) {
        let samples = (delay_sec * self.sample_rate) as usize;
        self.delay_samples = samples.min(MAX_DELAY - 1);
    }

    pub fn set_delay_samples(&mut self, samples: usize) {
        self.delay_samples = samples.min(MAX_DELAY - 1);
    }

    pub fn delay_samples(&self) -> usize { self.delay_samples }
    pub const fn max_delay(&self) -> usize { MAX_DELAY }
    pub fn sample_rate(&self) -> f32 { self.sample_rate }

    /// Write a sample and return the delayed sample.
    #[inline(always)]
    pub fn write(&mut self, input: T) -> T {
        self.buffer[self.write_pos] = input;
        let read_pos = if self.write_pos >= self.delay_samples {
            self.write_pos - self.delay_samples
        } else {
            MAX_DELAY + self.write_pos - self.delay_samples
        };
        let output = self.buffer[read_pos];
        self.write_pos = (self.write_pos + 1) % MAX_DELAY;
        self.stats.record_write();
        self.stats.record_read();
        self.stats.update_peak(MAX_DELAY);
        output
    }

    /// Read the delayed sample without writing.
    #[inline(always)]
    pub fn read(&self) -> T {
        let read_pos = if self.write_pos >= self.delay_samples {
            self.write_pos - self.delay_samples
        } else {
            MAX_DELAY + self.write_pos - self.delay_samples
        };
        self.buffer[read_pos]
    }

    /// Read sample at arbitrary delay (0 = most recent).
    #[inline(always)]
    pub fn read_delayed(&self, delay: usize) -> T {
        debug_assert!(delay < MAX_DELAY, "Delay {} out of range (max {})", delay, MAX_DELAY);
        let read_pos = if self.write_pos >= delay + 1 {
            self.write_pos - 1 - delay
        } else {
            MAX_DELAY + self.write_pos - 1 - delay
        };
        self.buffer[read_pos]
    }

    /// Read with linear interpolation between samples.
    #[inline(always)]
    pub fn read_interpolated(&self, delay_frac: f32) -> T {
        let delay_int = delay_frac.floor() as usize;
        let frac = T::from_f32(delay_frac.fract());
        let s1 = self.read_delayed(delay_int);
        if delay_int == MAX_DELAY - 1 {
            s1
        } else {
            let s2 = self.read_delayed(delay_int + 1);
            s1 + (s2 - s1) * frac
        }
    }

    /// Clear the delay line (fill with zeros).
    pub fn clear(&mut self) {
        self.buffer.fill(T::ZERO);
        self.write_pos = 0;
        self.stats.reset();
    }

    pub fn write_position(&self) -> usize { self.write_pos }
}

// ============================================================================
// Index implementations
// ============================================================================

impl<T: Transcendental, const MAX_DELAY: usize> Index<usize> for DelayLine<T, MAX_DELAY> {
    type Output = T;
    fn index(&self, index: usize) -> &Self::Output {
        &self.buffer[index]
    }
}

impl<T: Transcendental, const MAX_DELAY: usize> IndexMut<usize> for DelayLine<T, MAX_DELAY> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.buffer[index]
    }
}

// ============================================================================
// SignalBuffer Implementation
// ============================================================================

impl<T: Transcendental, const MAX_DELAY: usize> SignalBuffer<T> for DelayLine<T, MAX_DELAY> {
    fn capacity(&self) -> usize { MAX_DELAY }
    fn len(&self) -> usize { MAX_DELAY }
    fn is_empty(&self) -> bool { false }
    fn is_full(&self) -> bool { true }
    fn clear(&mut self) { self.clear(); }
    fn stats(&self) -> BufferStats {
        let mut stats = self.stats.snapshot();
        stats.fill_level = 1.0;
        stats
    }
    fn reset_stats(&mut self) { self.stats.reset(); }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delay_line_basic() {
        let mut delay = DelayLine::<f32, 1024>::new(44100.0);
        delay.set_delay_samples(100);
        for i in 0..200 {
            let out = delay.write(i as f32);
            if i >= 100 {
                assert_eq!(out, (i - 100) as f32);
            }
        }
    }

    #[test]
    fn test_delay_line_read_delayed() {
        let mut delay = DelayLine::<f32, 1024>::new(44100.0);
        for i in 0..1024 { delay.write(i as f32); }
        assert_eq!(delay.read_delayed(0), 1023.0);
        assert_eq!(delay.read_delayed(100), 923.0);
    }

    #[test]
    fn test_delay_line_interpolation() {
        let mut delay = DelayLine::<f32, 1024>::new(44100.0);
        for i in 0..1024 { delay.write(i as f32); }
        let val = delay.read_interpolated(100.5);
        assert!((val - 922.5).abs() < 0.01);
    }
}
