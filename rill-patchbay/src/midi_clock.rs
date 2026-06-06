//! MIDI Clock tracking — counts 24ppqn clock pulses and derives tempo (BPM).
//!
//! ## Architecture
//!
//! The [`MidiClockTracker`] receives raw MIDI system-realtime messages
//! (`0xF8` clock pulse, `0xFA/0xFB/0xFC` transport) and:
//! 1. Derives BPM from clock pulse intervals (running average over 24 ticks)
//! 2. Writes BPM atomically into a shared [`rill_core::time::SystemClock`]
//! 3. Delegates transport behaviour to a pluggable [`MidiClockStrategy`]
//!
//! ## Strategy trait
//!
//! The [`MidiClockStrategy`] trait lets applications customise how Start/Stop/
//! Continue and per-quarter-note BPM updates are handled without coupling
//! the tracker to Patchbay or graph internals.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use rill_core::time::SystemClock;

// =============================================================================
// MidiClockStrategy
// =============================================================================

/// Pluggable strategy for responding to MIDI transport and tempo changes.
///
/// Implementations receive a shared reference to [`SystemClock`] so they
/// can reset position or change BPM via its atomic methods.
pub trait MidiClockStrategy: Send + Sync {
    /// Called when the tracker receives a valid BPM estimate (~every 24 clock ticks).
    fn on_bpm(&mut self, clock: &SystemClock, bpm: f64);

    /// Called on MIDI Start (`0xFA`).
    fn on_start(&mut self, clock: &SystemClock);

    /// Called on MIDI Stop (`0xFC`).
    fn on_stop(&mut self, clock: &SystemClock);

    /// Called on MIDI Continue (`0xFB`).
    fn on_continue(&mut self, clock: &SystemClock);
}

// =============================================================================
// Built-in strategies
// =============================================================================

/// Resets the clock position on Start; freezes position on Stop.
///
/// Typical use: drum machines, hardware sequencers slaved to a master clock.
pub struct ResetOnStart;

impl MidiClockStrategy for ResetOnStart {
    fn on_bpm(&mut self, clock: &SystemClock, bpm: f64) {
        clock.set_bpm(bpm);
    }

    fn on_start(&mut self, clock: &SystemClock) {
        clock.reset();
    }

    fn on_stop(&mut self, _clock: &SystemClock) {}

    fn on_continue(&mut self, _clock: &SystemClock) {}
}

/// Follows BPM changes but ignores transport — the slave clock runs free.
///
/// Typical use: effects processors that only need tempo, not position.
pub struct FreeRunning;

impl MidiClockStrategy for FreeRunning {
    fn on_bpm(&mut self, clock: &SystemClock, bpm: f64) {
        clock.set_bpm(bpm);
    }

    fn on_start(&mut self, _clock: &SystemClock) {}

    fn on_stop(&mut self, _clock: &SystemClock) {}

    fn on_continue(&mut self, _clock: &SystemClock) {}
}

/// Resets position on Start, keeps it on Continue, and tracks whether
/// the transport is currently playing.
///
/// Provides `is_playing()` for downstream consumers that need transport state.
/// Typical use: DAW sync with song-position-aware slaves.
pub struct SongPosition {
    is_playing: bool,
}

impl SongPosition {
    /// Create a new `SongPosition` tracker in stopped state.
    pub fn new() -> Self {
        Self { is_playing: false }
    }

    /// Whether the transport is currently playing (Start or Continue received, not yet Stop).
    pub fn is_playing(&self) -> bool {
        self.is_playing
    }
}

impl Default for SongPosition {
    fn default() -> Self {
        Self::new()
    }
}

impl MidiClockStrategy for SongPosition {
    fn on_bpm(&mut self, clock: &SystemClock, bpm: f64) {
        clock.set_bpm(bpm);
    }

    fn on_start(&mut self, clock: &SystemClock) {
        clock.reset();
        self.is_playing = true;
    }

    fn on_stop(&mut self, _clock: &SystemClock) {
        self.is_playing = false;
    }

    fn on_continue(&mut self, _clock: &SystemClock) {
        self.is_playing = true;
    }
}

// =============================================================================
// MidiClockTracker
// =============================================================================

const TICKS_PER_BEAT: usize = 24;

/// Tracks incoming MIDI clock pulses and derives BPM.
///
/// Maintains a ring buffer of the last N tick intervals (24 ticks = 1 beat).
/// On every clock tick it updates the running average; every 24th tick it
/// calls `strategy.on_bpm()` with the averaged BPM.
///
/// # Thread safety
///
/// Designed to run on the **MIDI polling thread** (not the real-time signal
/// thread). BPM is written atomically into the shared [`SystemClock`]; the
/// signal thread reads it via `SystemClock::bpm()` without locks.
pub struct MidiClockTracker {
    clock: Arc<SystemClock>,
    strategy: Box<dyn MidiClockStrategy>,
    last_tick: Option<Instant>,
    intervals: VecDeque<f64>,
    tick_count: u64,
    ticks_since_bpm: usize,
    bpm: Option<f64>,
    is_playing: Arc<AtomicBool>,
}

impl MidiClockTracker {
    /// Create a tracker sharing the given [`SystemClock`] with the signal thread.
    ///
    /// The tracker writes BPM into `clock` atomically; the signal thread
    /// reads it via `clock.bpm()`.
    pub fn new(clock: Arc<SystemClock>, strategy: Box<dyn MidiClockStrategy>) -> Self {
        let bpm = clock.bpm();
        Self {
            clock,
            strategy,
            last_tick: None,
            intervals: VecDeque::with_capacity(TICKS_PER_BEAT),
            tick_count: 0,
            ticks_since_bpm: 0,
            bpm: Some(bpm),
            is_playing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns a clone of the shared `Arc<SystemClock>` for the signal thread.
    pub fn shared_clock(&self) -> Arc<SystemClock> {
        Arc::clone(&self.clock)
    }

    /// Returns a clone of the atomic `is_playing` flag.
    ///
    /// The flag is set on MIDI Start/Continue and cleared on MIDI Stop.
    /// Sequencers and automations should check this flag before producing output.
    pub fn playing_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.is_playing)
    }

    /// Current BPM estimate, or `None` before enough pulses arrive.
    pub fn bpm(&self) -> Option<f64> {
        self.bpm
    }

    /// Total MIDI clock ticks received since creation or last Start.
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// Feed a raw MIDI system-realtime status byte.
    ///
    /// Returns an optional `ControlEvent`-level reaction (currently always `None`;
    /// transport and clock events are handled internally via the strategy).
    pub fn process_status(&mut self, status: u8) {
        match status {
            0xF8 => self.tick(),
            0xFA => {
                self.is_playing.store(true, Ordering::Release);
                self.strategy.on_start(&self.clock);
                self.reset_tracking();
            }
            0xFC => {
                self.is_playing.store(false, Ordering::Release);
                self.strategy.on_stop(&self.clock);
            }
            0xFB => {
                self.is_playing.store(true, Ordering::Release);
                self.strategy.on_continue(&self.clock);
            }
            _ => {}
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        self.tick_count += 1;
        self.ticks_since_bpm += 1;

        if let Some(prev) = self.last_tick.replace(now) {
            let dt = now.duration_since(prev).as_secs_f64();
            if dt > 0.0 && dt < 2.0 {
                // Discard intervals > 2s (likely transport reset)
                self.intervals.push_back(dt);
                if self.intervals.len() > TICKS_PER_BEAT {
                    self.intervals.pop_front();
                }
            }
        }

        if self.ticks_since_bpm >= TICKS_PER_BEAT {
            self.ticks_since_bpm = 0;
            if !self.intervals.is_empty() {
                let avg: f64 = self.intervals.iter().sum::<f64>() / self.intervals.len() as f64;
                let bpm = 60.0 / (avg * TICKS_PER_BEAT as f64);
                let bpm = bpm.clamp(20.0, 300.0);
                self.bpm = Some(bpm);
                self.strategy.on_bpm(&self.clock, bpm);
            }
        }
    }

    fn reset_tracking(&mut self) {
        self.tick_count = 0;
        self.ticks_since_bpm = 0;
        self.intervals.clear();
        self.last_tick = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_tracker_initial_bpm() {
        let clock = Arc::new(SystemClock::new(44100.0, 120.0));
        let tracker = MidiClockTracker::new(clock, Box::new(FreeRunning));
        assert_eq!(tracker.bpm(), Some(120.0));
    }

    #[test]
    fn test_strategy_free_running() {
        let mut strategy = FreeRunning;
        let clock = SystemClock::new(44100.0, 120.0);
        strategy.on_bpm(&clock, 100.0);
        assert!((clock.bpm() - 100.0).abs() < 1e-10);
    }

    #[test]
    fn test_strategy_reset_on_start() {
        let mut strategy = ResetOnStart;
        let clock = SystemClock::new(44100.0, 120.0);
        strategy.on_start(&clock);
        assert_eq!(clock.position(), 0);
    }

    #[test]
    fn test_strategy_song_position() {
        let mut strategy = SongPosition::new();
        assert!(!strategy.is_playing());

        let clock = SystemClock::new(44100.0, 120.0);
        strategy.on_start(&clock);
        assert!(strategy.is_playing());

        strategy.on_stop(&clock);
        assert!(!strategy.is_playing());

        strategy.on_continue(&clock);
        assert!(strategy.is_playing());
    }

    #[test]
    fn test_bpm_derivation_from_pulses() {
        let clock = Arc::new(SystemClock::new(44100.0, 120.0));
        let mut tracker = MidiClockTracker::new(clock, Box::new(FreeRunning));

        // Simulate 96 clock ticks at 120 BPM (4 beats, should produce 5 BPM updates)
        for _ in 0..96 {
            tracker.process_status(0xF8);
            // At 120 BPM: interval = 60 / (120 * 24) = 20.833 ms
            thread::sleep(std::time::Duration::from_micros(20833));
        }

        let bpm = tracker.bpm().unwrap();
        assert!(
            bpm > 110.0 && bpm < 130.0,
            "expected ~120 BPM, got {:.1}",
            bpm
        );
    }

    #[test]
    fn test_transport_start_resets_ticks() {
        let clock = Arc::new(SystemClock::new(44100.0, 120.0));
        let mut tracker = MidiClockTracker::new(clock, Box::new(FreeRunning));

        tracker.process_status(0xF8);
        tracker.process_status(0xF8);
        assert_eq!(tracker.tick_count(), 2);

        tracker.process_status(0xFA); // Start
        assert_eq!(tracker.tick_count(), 0);
    }

    #[test]
    fn test_clamps_outlier_bpm() {
        let clock = Arc::new(SystemClock::new(44100.0, 120.0));
        let mut tracker = MidiClockTracker::new(clock, Box::new(FreeRunning));

        // Feed one tick then immediately another — tiny interval → huge BPM
        for _ in 0..TICKS_PER_BEAT {
            tracker.process_status(0xF8);
        }

        let bpm = tracker.bpm().unwrap();
        assert!(
            bpm <= 300.0 && bpm >= 20.0,
            "BPM {:.1} out of clamp range",
            bpm
        );
    }
}
