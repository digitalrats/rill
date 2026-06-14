//! # Clock tick - the heartbeat of signal processing
//!
//! A `ClockTick` represents a single moment in signal time, containing
//! information about sample position, block size, and tempo.

use std::fmt;
use std::sync::Arc;

use crate::traits::buffer_view::{BufferView, NullBufferView};

/// A tick of the system clock
///
/// Sent to nodes on every signal block to provide timing information
/// and synchronize processing. This is the fundamental timing primitive
/// in Rill.
///
/// # Fields
///
/// * `sample_pos` - Absolute sample position since start
/// * `samples_since_last` - Number of samples since the last tick
/// * `is_new_block` - Whether this is the start of a new block
/// * `sample_rate` - Current sample rate in Hz
/// * `tempo` - Current tempo in BPM (if available)
/// * `source` - Which backend produced this tick (e.g. "alsa:default")
/// * `view` - Backend-specific buffer accessor
///
/// # Example
///
/// ```
/// use rill_core::time::ClockTick;
/// use std::sync::Arc;
/// use rill_core::traits::buffer_view::NullBufferView;
///
/// let view = Arc::new(NullBufferView::new(2, 2));
/// let tick = ClockTick::new(44100, 64, 44100.0, "test".into(), view);
/// assert_eq!(tick.absolute_seconds(), 1.0);
/// assert_eq!(tick.delta_seconds(), 64.0 / 44100.0);
/// ```
#[derive(Clone)]
pub struct ClockTick {
    /// Absolute sample position since start
    pub sample_pos: u64,

    /// Number of samples since the last tick
    pub samples_since_last: u32,

    /// Whether this is the start of a new block
    pub is_new_block: bool,

    /// Current sample rate in Hz
    pub sample_rate: f32,

    /// Current tempo in BPM (if available)
    pub tempo: Option<f32>,

    /// Which backend produced this tick (e.g. "alsa:default", "pipewire:0").
    pub source: String,

    /// Backend-specific buffer accessor for reading input / writing output.
    #[doc(hidden)]
    pub view: Arc<dyn BufferView>,

    /// Hardware clock correction factor: `configured_rate / actual_rate`.
    ///
    /// `1.0` = nominal (rates match). `< 1.0` = hardware runs faster.
    /// `> 1.0` = hardware runs slower.  Set by the backend when the
    /// negotiated hardware rate differs from the graph's configured rate.
    pub speed_ratio: f64,
}

impl PartialEq for ClockTick {
    fn eq(&self, other: &Self) -> bool {
        self.sample_pos == other.sample_pos
            && self.samples_since_last == other.samples_since_last
            && self.is_new_block == other.is_new_block
            && self.sample_rate == other.sample_rate
            && self.tempo == other.tempo
            && self.source == other.source
    }
}

impl fmt::Debug for ClockTick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClockTick")
            .field("sample_pos", &self.sample_pos)
            .field("samples_since_last", &self.samples_since_last)
            .field("is_new_block", &self.is_new_block)
            .field("sample_rate", &self.sample_rate)
            .field("tempo", &self.tempo)
            .field("source", &self.source)
            .field("view", &"<BufferView>")
            .finish()
    }
}

impl ClockTick {
    /// Create a new clock tick
    ///
    /// # Arguments
    /// * `sample_pos` - Absolute sample position
    /// * `samples_since_last` - Samples since last tick
    /// * `sample_rate` - Sample rate in Hz
    /// * `source` - Backend source name
    /// * `view` - Backend buffer accessor
    ///
    /// # Returns
    /// A new `ClockTick` with `is_new_block = true` and `tempo = None`
    pub fn new(
        sample_pos: u64,
        samples_since_last: u32,
        sample_rate: f32,
        source: String,
        view: Arc<dyn BufferView>,
    ) -> Self {
        Self {
            sample_pos,
            samples_since_last,
            is_new_block: true,
            sample_rate,
            tempo: None,
            source,
            view,
            speed_ratio: 1.0,
        }
    }

    /// Create a new clock tick with tempo information
    ///
    /// # Arguments
    /// * `sample_pos` - Absolute sample position
    /// * `samples_since_last` - Samples since last tick
    /// * `sample_rate` - Sample rate in Hz
    /// * `tempo` - Tempo in BPM
    /// * `source` - Backend source name
    /// * `view` - Backend buffer accessor
    pub fn with_tempo(
        sample_pos: u64,
        samples_since_last: u32,
        sample_rate: f32,
        tempo: f32,
        source: String,
        view: Arc<dyn BufferView>,
    ) -> Self {
        Self {
            sample_pos,
            samples_since_last,
            is_new_block: true,
            sample_rate,
            tempo: Some(tempo),
            source,
            view,
            speed_ratio: 1.0,
        }
    }

    /// Get the time since the last tick in seconds
    ///
    /// # Returns
    /// Time in seconds since the previous tick
    #[inline(always)]
    pub fn delta_seconds(&self) -> f32 {
        self.samples_since_last as f32 / self.sample_rate
    }

    /// Get the absolute time in seconds since start
    ///
    /// # Returns
    /// Absolute time in seconds
    #[inline(always)]
    pub fn absolute_seconds(&self) -> f64 {
        self.sample_pos as f64 / self.sample_rate as f64
    }

    /// Get the current beat position (if tempo is available)
    ///
    /// # Returns
    /// * `Some(beat)` - Current beat position (fractional)
    /// * `None` - No tempo information available
    #[inline(always)]
    pub fn beat_position(&self) -> Option<f64> {
        self.tempo.map(|bpm| {
            let seconds_per_beat = 60.0 / bpm as f64;
            self.absolute_seconds() / seconds_per_beat
        })
    }

    /// Get the current bar-beat-sixteenth position (if tempo is available)
    ///
    /// # Returns
    /// * `Some((bar, beat, sixteenth))` - Musical position
    /// * `None` - No tempo information available
    pub fn musical_position(&self) -> Option<(u32, u8, u8)> {
        self.tempo.map(|bpm| {
            let seconds_per_beat = 60.0 / bpm as f64;
            let total_beats = self.absolute_seconds() / seconds_per_beat;

            let bar = (total_beats / 4.0).floor() as u32;
            let beat_in_bar = (total_beats % 4.0) as u8;
            let sixteenth = ((total_beats.fract() * 4.0) as u8) % 4;

            (bar, beat_in_bar, sixteenth)
        })
    }

    /// Advance to the next tick
    ///
    /// # Arguments
    /// * `samples` - Number of samples to advance
    pub fn advance(&mut self, samples: u32) {
        self.sample_pos += samples as u64;
        self.samples_since_last = samples;
        self.is_new_block = true;
    }

    /// Check if this tick is at the start of a new bar
    ///
    /// # Returns
    /// `true` if this is the first beat of a bar
    pub fn is_new_bar(&self) -> bool {
        if let Some((_, beat, sixteenth)) = self.musical_position() {
            beat == 0 && sixteenth == 0
        } else {
            false
        }
    }

    /// Check if this tick is at the start of a new beat
    ///
    /// # Returns
    /// `true` if this is the start of a beat
    pub fn is_new_beat(&self) -> bool {
        if let Some((_, _, sixteenth)) = self.musical_position() {
            sixteenth == 0
        } else {
            false
        }
    }
}

impl Default for ClockTick {
    fn default() -> Self {
        Self {
            sample_pos: 0,
            samples_since_last: 0,
            is_new_block: false,
            sample_rate: 44100.0,
            tempo: None,
            source: String::new(),
            view: Arc::new(NullBufferView::new(2, 2)),
            speed_ratio: 1.0,
        }
    }
}

impl fmt::Display for ClockTick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ClockTick(pos={}, delta={}ms, rate={}Hz, source={}",
            self.sample_pos,
            self.delta_seconds() * 1000.0,
            self.sample_rate,
            self.source,
        )?;

        if let Some(tempo) = self.tempo {
            write!(f, ", tempo={}BPM", tempo)?;
        }

        write!(f, ")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn null_view() -> Arc<dyn BufferView> {
        Arc::new(NullBufferView::new(2, 2))
    }

    #[test]
    fn test_clock_tick_creation() {
        let tick = ClockTick::new(44100, 44100, 44100.0, "test".into(), null_view());
        assert_eq!(tick.sample_pos, 44100);
        assert_eq!(tick.samples_since_last, 44100);
        assert!(tick.is_new_block);
        assert_eq!(tick.sample_rate, 44100.0);
        assert_eq!(tick.tempo, None);
        assert_eq!(tick.source, "test");
    }

    #[test]
    fn test_clock_tick_with_tempo() {
        let tick = ClockTick::with_tempo(44100, 44100, 44100.0, 120.0, "test".into(), null_view());
        assert_eq!(tick.tempo, Some(120.0));
    }

    #[test]
    fn test_delta_seconds() {
        let tick = ClockTick::new(0, 44100, 44100.0, "test".into(), null_view());
        assert_eq!(tick.delta_seconds(), 1.0);

        let tick = ClockTick::new(0, 22050, 44100.0, "test".into(), null_view());
        assert_eq!(tick.delta_seconds(), 0.5);
    }

    #[test]
    fn test_absolute_seconds() {
        let tick = ClockTick::new(44100, 44100, 44100.0, "test".into(), null_view());
        assert_eq!(tick.absolute_seconds(), 1.0);

        let tick = ClockTick::new(88200, 44100, 44100.0, "test".into(), null_view());
        assert_eq!(tick.absolute_seconds(), 2.0);
    }

    #[test]
    fn test_beat_position() {
        let tick = ClockTick::with_tempo(44100, 44100, 44100.0, 120.0, "test".into(), null_view());
        // At 120 BPM, one beat = 0.5 seconds
        // 1 second = 2 beats
        assert_eq!(tick.beat_position(), Some(2.0));
    }

    #[test]
    fn test_musical_position() {
        let tick =
            ClockTick::with_tempo(44100 * 2, 44100, 44100.0, 120.0, "test".into(), null_view());
        // 2 seconds at 120 BPM = 4 beats
        // 4 beats = 1 bar
        let pos = tick.musical_position();
        assert_eq!(pos, Some((1, 0, 0)));

        let tick =
            ClockTick::with_tempo(44100 * 3, 44100, 44100.0, 120.0, "test".into(), null_view());
        // 3 seconds = 6 beats = 1.5 bars
        let pos = tick.musical_position();
        assert_eq!(pos, Some((1, 2, 0)));
    }

    #[test]
    fn test_advance() {
        let mut tick = ClockTick::new(0, 0, 44100.0, "test".into(), null_view());
        tick.advance(64);
        assert_eq!(tick.sample_pos, 64);
        assert_eq!(tick.samples_since_last, 64);
        assert!(tick.is_new_block);
    }

    #[test]
    fn test_is_new_bar() {
        let tick = ClockTick::with_tempo(0, 0, 44100.0, 120.0, "test".into(), null_view());
        assert!(tick.is_new_bar());

        let tick = ClockTick::with_tempo(22050, 22050, 44100.0, 120.0, "test".into(), null_view());
        // 0.5 seconds = 1 beat, not new bar
        assert!(!tick.is_new_bar());
    }

    #[test]
    fn test_is_new_beat() {
        let tick = ClockTick::with_tempo(0, 0, 44100.0, 120.0, "test".into(), null_view());
        assert!(tick.is_new_beat());

        let tick = ClockTick::with_tempo(11025, 11025, 44100.0, 120.0, "test".into(), null_view());
        // 0.25 seconds = half beat, not new beat
        assert!(!tick.is_new_beat());
    }

    #[test]
    fn test_default() {
        let tick = ClockTick::default();
        assert_eq!(tick.sample_pos, 0);
        assert_eq!(tick.samples_since_last, 0);
        assert!(!tick.is_new_block);
        assert_eq!(tick.sample_rate, 44100.0);
        assert_eq!(tick.tempo, None);
        assert_eq!(tick.source, "");
    }

    #[test]
    fn test_display() {
        let tick = ClockTick::new(44100, 44100, 44100.0, "test".into(), null_view());
        let display = format!("{}", tick);
        assert!(display.contains("pos=44100"));
        assert!(display.contains("delta=1000ms"));
        assert!(display.contains("source=test"));

        let tick = ClockTick::with_tempo(44100, 44100, 44100.0, 120.0, "test".into(), null_view());
        let display = format!("{}", tick);
        assert!(display.contains("tempo=120BPM"));
    }
}
