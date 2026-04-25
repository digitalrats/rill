//! Clock and time abstractions for audio processing
//!
//! Provides timing information for sample-accurate processing
//! and synchronization between audio graph and control world.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use super::tick;

// ============================================================================
// Clock Tick
// ============================================================================

/// A tick of the audio clock
///
/// Sent to nodes on every audio block to provide timing information
/// and synchronize processing.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct ClockTick {
    /// Absolute sample position since start
    pub sample_pos: u64,
    
    /// Number of samples since last tick
    pub samples_since_last: u32,
    
    /// Whether this is the start of a new block
    pub is_new_block: bool,
    
    /// Current sample rate
    pub sample_rate: f32,
    
    /// Current tempo in BPM (if available)
    pub tempo: Option<f64>,
}

#[allow(dead_code)]
impl ClockTick {
    /// Create a new clock tick
    pub fn new(sample_pos: u64, samples_since_last: u32, sample_rate: f32) -> Self {
        Self {
            sample_pos,
            samples_since_last,
            is_new_block: true,
            sample_rate,
            tempo: None,
        }
    }
    
    /// Get time since last tick in seconds
    pub fn delta_seconds(&self) -> f32 {
        self.samples_since_last as f32 / self.sample_rate
    }
    
    /// Get absolute time in seconds
    pub fn absolute_seconds(&self) -> f64 {
        self.sample_pos as f64 / self.sample_rate as f64
    }
    
    /// Advance to next tick
    pub fn advance(&mut self, samples: u32) {
        self.sample_pos += samples as u64;
        self.samples_since_last = samples;
        self.is_new_block = true;
    }
}

// ============================================================================
// Clock Source
// ============================================================================

/// Source of clock ticks
///
/// Can be either a hardware device (ALSA, JACK) or a software generator.
#[allow(dead_code)]
pub trait ClockSource: Send + Sync {
    /// Get the next clock tick
    fn next_tick(&mut self) -> ClockTick;
    
    /// Get the sample rate
    fn sample_rate(&self) -> f32;
    
    /// Start the clock
    fn start(&mut self) -> Result<(), ClockError>;
    
    /// Stop the clock
    fn stop(&mut self) -> Result<(), ClockError>;
}

// ============================================================================
// System Clock
// ============================================================================

/// High-precision system clock
///
/// Provides sample-accurate timing for audio processing.
/// Uses atomic operations for thread safety without locks.
pub struct SystemClock {
    pub sample_rate: f32,
    position: AtomicU64,
    bpm: AtomicU64, // stored as bits for atomic operations
}

impl SystemClock {
    /// Create a new system clock
    pub fn new(sample_rate: f32, initial_bpm: f64) -> Self {
        Self {
            sample_rate,
            position: AtomicU64::new(0),
            bpm: AtomicU64::new(initial_bpm.to_bits()),
        }
    }
    
    /// Create a clock with default BPM (120)
    pub fn with_sample_rate(sample_rate: f32) -> Self {
        Self::new(sample_rate, 120.0)
    }
    
    /// Get the next tick
    pub fn next_tick(&mut self, block_size: usize) -> tick::ClockTick {
        let samples = block_size as u32;
        let pos = self.position.fetch_add(samples as u64, Ordering::Relaxed);
        
        tick::ClockTick {
            sample_pos: pos,
            samples_since_last: samples,
            is_new_block: true,
            sample_rate: self.sample_rate,
            tempo: Some(self.bpm() as f32),
        }
    }
    
    /// Get current BPM
    pub fn bpm(&self) -> f64 {
        f64::from_bits(self.bpm.load(Ordering::Relaxed))
    }
    
    /// Set BPM
    pub fn set_bpm(&self, bpm: f64) {
        self.bpm.store(bpm.to_bits(), Ordering::Relaxed);
    }
    
    /// Get current sample position
    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }
    
    /// Reset position to zero
    pub fn reset(&self) {
        self.position.store(0, Ordering::Relaxed);
    }
}

impl ClockSource for SystemClock {
    fn next_tick(&mut self) -> ClockTick {
        let pos = self.position.fetch_add(1, Ordering::Relaxed);
        ClockTick {
            sample_pos: pos,
            samples_since_last: 1,
            is_new_block: true,
            sample_rate: self.sample_rate,
            tempo: Some(self.bpm()),
        }
    }
    
    fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
    
    fn start(&mut self) -> Result<(), ClockError> {
        Ok(())
    }
    
    fn stop(&mut self) -> Result<(), ClockError> {
        Ok(())
    }
}

impl fmt::Debug for SystemClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemClock")
            .field("sample_rate", &self.sample_rate)
            .field("position", &self.position.load(Ordering::Relaxed))
            .field("bpm", &self.bpm())
            .finish()
    }
}

// ============================================================================
// Clock Error
// ============================================================================

/// Errors that can occur in clock operations
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum ClockError {
    /// Hardware error (ALSA, JACK, etc.)
    #[error("Hardware error: {0}")]
    Hardware(String),
    
    /// Invalid sample rate
    #[error("Invalid sample rate: {0}")]
    InvalidSampleRate(f32),
    
    /// Clock not started
    #[error("Clock not started")]
    NotStarted,
    
    /// Clock already started
    #[error("Clock already started")]
    AlreadyStarted,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_system_clock() {
        let mut clock = SystemClock::new(44100.0, 120.0);
        
        let tick = clock.next_tick(64);
        assert_eq!(tick.sample_pos, 0);
        assert_eq!(tick.samples_since_last, 64);
        assert_eq!(tick.sample_rate, 44100.0);
        assert_eq!(tick.tempo, Some(120.0));
        
        let tick = clock.next_tick(64);
        assert_eq!(tick.sample_pos, 64);
    }
    
    #[test]
    fn test_clock_tick_math() {
        let tick = ClockTick::new(44100, 44100, 44100.0);
        assert_eq!(tick.absolute_seconds(), 1.0);
        assert_eq!(tick.delta_seconds(), 1.0);
    }
}