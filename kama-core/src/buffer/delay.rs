//! Delay line for audio effects
//!
//! Provides a circular buffer for implementing delay, reverb,
//! and other time-based effects.
use crate::math::AudioNum;
use crate::buffer::AtomicStats;
use core::marker::PhantomData;
#[repr(align(64))]
pub struct DelayLine<T: AudioNum, const MAX_DELAY: usize> {
    /// Circular buffer storage
    buffer: [T; MAX_DELAY],
    
    /// Write position
    write_pos: usize,
    
    /// Current delay in samples
    delay_samples: usize,
    
    /// Sample rate
    sample_rate: f32,
    
    /// Statistics
    stats: AtomicStats,
    
    /// Phantom data
    _phantom: PhantomData<T>,
}

impl<T: AudioNum, const MAX_DELAY: usize> DelayLine<T, MAX_DELAY> {
    /// Create new delay line
    pub fn new(sample_rate: f32) -> Self {
        Self {
            buffer: [T::ZERO; MAX_DELAY],
            write_pos: 0,
            delay_samples: 0,
            sample_rate,
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }
    
    /// Set delay time in seconds
    pub fn set_delay(&mut self, delay_sec: f32) {
        let samples = (delay_sec * self.sample_rate) as usize;
        self.delay_samples = samples.min(MAX_DELAY - 1);
    }
    
    /// Set delay in samples
    pub fn set_delay_samples(&mut self, samples: usize) {
        self.delay_samples = samples.min(MAX_DELAY - 1);
    }
    
    /// Get current delay in samples
    pub fn delay_samples(&self) -> usize {
        self.delay_samples
    }
    
    /// Get maximum delay in samples
    pub fn max_delay(&self) -> usize {
        MAX_DELAY
    }
    
    /// Write a sample and return the delayed sample
    #[inline(always)]
    pub fn write(&mut self, input: T) -> T {
        // Write current sample
        self.buffer[self.write_pos] = input;
        
        // Calculate read position for delayed sample
        let read_pos = if self.write_pos >= self.delay_samples {
            self.write_pos - self.delay_samples
        } else {
            MAX_DELAY + self.write_pos - self.delay_samples
        };
        
        // Read delayed sample
        let output = self.buffer[read_pos];
        
        // Advance write position
        self.write_pos = (self.write_pos + 1) % MAX_DELAY;
        
        // Update statistics
        self.stats.record_write();
        self.stats.record_read();
        
        output
    }
    
    /// Read sample at arbitrary delay (0 = most recent, delay = how many samples back)
    #[inline(always)]
    pub fn read_delayed(&self, delay: usize) -> T {
        let delay = delay.min(MAX_DELAY - 1);
        // Most recent sample is at (write_pos - 1) mod MAX_DELAY
        // So sample from 'delay' samples ago is at:
        // (write_pos - 1 - delay) mod MAX_DELAY
        let read_pos = if self.write_pos > delay {
            self.write_pos - 1 - delay
        } else {
            MAX_DELAY + self.write_pos - 1 - delay
        };
        
        self.buffer[read_pos]
    }
    
    /// Read with linear interpolation between samples
    #[inline(always)]
    pub fn read_interpolated(&self, delay_frac: f32) -> T {
        let delay_int = delay_frac.floor() as usize;
        let frac = T::from_f32(delay_frac.fract());
        
        let s1 = self.read_delayed(delay_int);
        
        if delay_int == MAX_DELAY - 1 {
            s1
        } else {
            let s2 = self.read_delayed(delay_int + 1);
            // Linear interpolation: (1-frac)*s1 + frac*s2
            s1 + (s2 - s1) * frac
        }
    }
    
    /// Clear the delay line
    pub fn clear(&mut self) {
        for i in 0..MAX_DELAY {
            self.buffer[i] = T::ZERO;
        }
        self.write_pos = 0;
        self.stats.reset();
    }
    
    /// Get current write position
    pub fn write_position(&self) -> usize {
        self.write_pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_interpolation() {
        let mut delay = DelayLine::<f32, 1024>::new(44100.0);
        
        // Fill with known values (0..1023)
        for i in 0..1024 {
            delay.write(i as f32);
        }
        
        // After writing 1024 samples:
        // write_pos = 0
        // buffer[0] = 1023 (most recent)
        // buffer[1] = 0
        // buffer[2] = 1
        // ...
        // buffer[1023] = 1022
        
        // Test read_delayed
        assert_eq!(delay.read_delayed(0), 1023.0, "delay 0 should be most recent (1023)");
        assert_eq!(delay.read_delayed(1), 1022.0, "delay 1 should be previous (1022)");
        assert_eq!(delay.read_delayed(100), 923.0, "delay 100 should be 923");
        assert_eq!(delay.read_delayed(200), 823.0, "delay 200 should be 823");
        
        // Test interpolation
        // delay 100.5 should be halfway between 923 and 922 = 922.5
        // но мы ожидаем 923.5? Нет, правильно 922.5
        let test_cases = [
            (100.0, 923.0),      // Exact sample
            (100.5, 922.5),      // Half between 923 and 922
            (101.0, 922.0),      // Next exact
        ];
        
        for &(delay_frac, expected) in &test_cases {
            let val = delay.read_interpolated(delay_frac);
            let diff = (val - expected).abs();
            assert!(
                diff < 0.01,
                "Interpolation at {:.2}: expected {:.2}, got {:.2}, diff {:.2}",
                delay_frac, expected, val, diff
            );
        }
    }
}