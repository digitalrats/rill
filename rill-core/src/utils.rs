//! # Utilities
//!
//! Helper functions and types.

use std::time::{Duration, Instant};

/// Timer for measuring time in the audio thread
#[derive(Debug, Clone)]
pub struct AudioTimer {
    start: Instant,
    samples: u64,
    sample_rate: f32,
}

impl AudioTimer {
    /// Create a new timer
    pub fn new(sample_rate: f32) -> Self {
        Self {
            start: Instant::now(),
            samples: 0,
            sample_rate,
        }
    }
    
    /// Reset the timer
    pub fn reset(&mut self) {
        self.start = Instant::now();
        self.samples = 0;
    }
    
    /// Update the sample counter
    pub fn tick(&mut self, samples: u64) {
        self.samples += samples;
    }
    
    /// Get the current time in samples
    pub fn samples(&self) -> u64 {
        self.samples
    }
    
    /// Get the current time in seconds (based on samples)
    pub fn seconds(&self) -> f64 {
        self.samples as f64 / self.sample_rate as f64
    }
    
    /// Get the real time (wall-clock)
    pub fn real_seconds(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
    
    /// Check for drift (for debugging)
    pub fn drift(&self) -> f64 {
        (self.real_seconds() - self.seconds()).abs()
    }
}

/// Simple RMS analyzer
#[derive(Debug, Clone)]
pub struct RmsAnalyzer {
    sum_squares: f64,
    count: usize,
}

impl RmsAnalyzer {
    /// Create a new analyzer
    pub fn new() -> Self {
        Self {
            sum_squares: 0.0,
            count: 0,
        }
    }
    
    /// Add a sample
    pub fn add_sample<T: crate::math::Transcendental>(&mut self, sample: T) {
        let val = sample.to_f64();
        self.sum_squares += val * val;
        self.count += 1;
    }
    
    /// Add a slice
    pub fn add_slice<T: crate::math::Transcendental>(&mut self, slice: &[T]) {
        for &s in slice {
            self.add_sample(s);
        }
    }
    
    /// Get the current RMS
    pub fn rms(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            (self.sum_squares / self.count as f64).sqrt()
        }
    }
    
    /// Reset
    pub fn reset(&mut self) {
        self.sum_squares = 0.0;
        self.count = 0;
    }
}

/// Simple peak detector
#[derive(Debug, Clone)]
pub struct PeakDetector {
    peak: f32,
    decay: f32,
}

impl PeakDetector {
    /// Create a new detector
    pub fn new(decay: f32) -> Self {
        Self {
            peak: 0.0,
            decay: decay.clamp(0.0, 1.0),
        }
    }
    
    /// Add a sample
    pub fn add_sample<T: crate::math::Transcendental>(&mut self, sample: T) {
        let abs = sample.to_f32().abs();
        if abs > self.peak {
            self.peak = abs;
        } else {
            self.peak *= self.decay;
        }
    }
    
    /// Get the current peak
    pub fn peak(&self) -> f32 {
        self.peak
    }
    
    /// Reset
    pub fn reset(&mut self) {
        self.peak = 0.0;
    }
}

/// Performance measurement
#[derive(Debug, Clone)]
pub struct PerfTimer {
    name: String,
    start: Instant,
    total: Duration,
    count: usize,
}

impl PerfTimer {
    /// Create a new timer
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            start: Instant::now(),
            total: Duration::default(),
            count: 0,
        }
    }
    
    /// Start measurement
    pub fn start(&mut self) {
        self.start = Instant::now();
    }
    
    /// Stop measurement
    pub fn stop(&mut self) {
        self.total += self.start.elapsed();
        self.count += 1;
    }
    
    /// Get average time
    pub fn average(&self) -> Duration {
        if self.count == 0 {
            Duration::default()
        } else {
            self.total / self.count as u32
        }
    }
    
    /// Get statistics
    pub fn stats(&self) -> String {
        format!(
            "{}: avg={:?}, total={:?}, count={}",
            self.name,
            self.average(),
            self.total,
            self.count
        )
    }
}

/// Convert MIDI note to frequency
pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

/// Convert frequency to MIDI note
pub fn freq_to_midi(freq: f32) -> f32 {
    69.0 + 12.0 * (freq / 440.0).log2()
}

/// Convert decibels to linear value
pub fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Convert linear value to decibels
pub fn linear_to_db(linear: f32) -> f32 {
    20.0 * linear.max(1e-6).log10()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rms() {
        let mut rms = RmsAnalyzer::new();
        rms.add_slice(&[1.0f32, 1.0, 1.0, 1.0]);
        assert!((rms.rms() - 1.0).abs() < 1e-6);
    }
    
    #[test]
    fn test_peak() {
        let mut peak = PeakDetector::new(0.5);
        peak.add_sample(0.8f32);
        assert!((peak.peak() - 0.8).abs() < 1e-6);
        peak.add_sample(0.1);
        assert!(peak.peak() < 0.8);
    }
    
    #[test]
    fn test_midi_conversion() {
        let freq = midi_to_freq(69);
        assert!((freq - 440.0).abs() < 1.0);
        
        let midi = freq_to_midi(440.0);
        assert!((midi - 69.0).abs() < 0.1);
    }
}