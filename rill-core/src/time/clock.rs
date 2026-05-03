use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use super::tick;

/// High-precision system clock
///
/// Provides sample-accurate timing for signal processing.
/// Uses atomic operations for thread safety without locks.
pub struct SystemClock {
    pub sample_rate: f32,
    position: AtomicU64,
    bpm: AtomicU64,
}

impl SystemClock {
    pub fn new(sample_rate: f32, initial_bpm: f64) -> Self {
        Self {
            sample_rate,
            position: AtomicU64::new(0),
            bpm: AtomicU64::new(initial_bpm.to_bits()),
        }
    }

    pub fn with_sample_rate(sample_rate: f32) -> Self {
        Self::new(sample_rate, 120.0)
    }

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

    pub fn bpm(&self) -> f64 {
        f64::from_bits(self.bpm.load(Ordering::Relaxed))
    }

    pub fn set_bpm(&self, bpm: f64) {
        self.bpm.store(bpm.to_bits(), Ordering::Relaxed);
    }

    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.position.store(0, Ordering::Relaxed);
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
}
