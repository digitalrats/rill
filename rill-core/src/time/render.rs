use std::fmt;

/// Musical transport state — derived from JACK/PipeWire transport or MIDI clock.
///
/// Carries the current bar/beat/tempo state of the host timeline.
/// Populated by the I/O backend once per processing block.
#[derive(Debug, Clone, Copy)]
pub struct TransportState {
    /// Whether the host transport is currently rolling (playing).
    pub is_playing: bool,
    /// Current tempo in beats per minute.
    pub bpm: f64,
    /// Absolute transport frame (song position in samples).
    pub frame_pos: u64,
    /// Beats per bar — time signature numerator (default 4).
    pub time_sig_num: u8,
    /// Beat unit — time signature denominator (default 4 = quarter note).
    pub time_sig_den: u8,
    /// Sample position where the current bar started.
    pub bar_start_frame: u64,
}

impl Default for TransportState {
    fn default() -> Self {
        Self {
            is_playing: true,
            bpm: 120.0,
            frame_pos: 0,
            time_sig_num: 4,
            time_sig_den: 4,
            bar_start_frame: 0,
        }
    }
}

impl fmt::Display for TransportState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {:.1} BPM {} {}/{} frame={} bar_start={}",
            if self.is_playing { "▶" } else { "⏹" },
            self.bpm,
            if self.is_playing {
                "rolling"
            } else {
                "stopped"
            },
            self.time_sig_num,
            self.time_sig_den,
            self.frame_pos,
            self.bar_start_frame,
        )
    }
}

/// Unified render context — passed by `&` reference through the entire
/// signal graph during one processing block.
///
/// Built by the I/O backend once per block on the stack. Contains sample-clock
/// metadata, host transport state, and hardware clock correction factor.
///
/// # Field summary
///
/// | Field | Source | Used by |
/// |-------|--------|---------|
/// | `sample_pos` | I/O backend sample counter | Time-aware generators, sequencers |
/// | `samples_since_last` | Block size | Delta-time computation |
/// | `sample_rate` | I/O backend config | Frequency calculations, filters |
/// | `transport` | JACK/PipeWire transport or MIDI clock | BPM-synced LFOs, sequencers, automations |
/// | `speed_ratio` | SPA clock / DLL filter | Sample-rate conversion (resampling compensation) |
#[derive(Debug, Clone)]
pub struct RenderContext {
    /// Absolute sample position since graph start.
    pub sample_pos: u64,
    /// Number of samples processed in this block.
    pub samples_since_last: u32,
    /// Current sample rate in Hz.
    pub sample_rate: f32,
    /// Host transport state (BPM, playing flag, musical position).
    pub transport: TransportState,
    /// Hardware clock correction factor.
    ///
    /// `1.0` = nominal. `> 1.0` = sound card is slow (resample up).
    /// `< 1.0` = sound card is fast (resample down).
    /// Set by PipeWire's `spa_io_clock.rate_match` or a JACK DLL filter.
    pub speed_ratio: f64,
}

impl RenderContext {
    /// Create a minimal render context from basic clock parameters.
    ///
    /// Transport defaults to 120 BPM, playing, 4/4.
    pub fn new(sample_pos: u64, samples_since_last: u32, sample_rate: f32) -> Self {
        Self {
            sample_pos,
            samples_since_last,
            sample_rate,
            transport: TransportState::default(),
            speed_ratio: 1.0,
        }
    }

    /// Create a render context with an explicit BPM.
    pub fn with_tempo(
        sample_pos: u64,
        samples_since_last: u32,
        sample_rate: f32,
        bpm: f32,
    ) -> Self {
        Self {
            sample_pos,
            samples_since_last,
            sample_rate,
            transport: TransportState {
                bpm: bpm as f64,
                ..TransportState::default()
            },
            speed_ratio: 1.0,
        }
    }

    /// Time delta of this block in seconds.
    pub fn delta_seconds(&self) -> f64 {
        self.samples_since_last as f64 / self.sample_rate as f64
    }

    /// Absolute time in seconds since graph start.
    pub fn absolute_seconds(&self) -> f64 {
        self.sample_pos as f64 / self.sample_rate as f64
    }

    /// Current BPM as `f32` for backward compatibility.
    pub fn bpm(&self) -> f32 {
        self.transport.bpm as f32
    }

    /// Musical beat position (requires valid BPM).
    ///
    /// Returns `None` if BPM is zero.
    pub fn beat_position(&self) -> Option<f64> {
        if self.transport.bpm <= 0.0 {
            return None;
        }
        let seconds_per_beat = 60.0 / self.transport.bpm;
        Some(self.absolute_seconds() / seconds_per_beat)
    }

    /// Musical position as `(bar, beat_in_bar, sixteenth)` triple.
    ///
    /// Uses `time_sig_num` and `time_sig_den` from the transport state.
    pub fn musical_position(&self) -> Option<(u32, u8, u8)> {
        self.beat_position().map(|total_beats| {
            let beats_per_bar = self.transport.time_sig_num as f64;
            let bar = (total_beats / beats_per_bar).floor() as u32;
            let beat_in_bar = (total_beats % beats_per_bar) as u8;
            let sixteenth = ((total_beats.fract() * 4.0) as u8) % 4;
            (bar, beat_in_bar, sixteenth)
        })
    }

    /// Whether this block starts a new bar.
    pub fn is_new_bar(&self) -> bool {
        self.musical_position()
            .map(|(_bar, beat, sixteenth)| beat == 0 && sixteenth == 0)
            .unwrap_or(false)
    }

    /// Whether this block starts a new beat.
    pub fn is_new_beat(&self) -> bool {
        self.musical_position()
            .map(|(_bar, _beat, sixteenth)| sixteenth == 0)
            .unwrap_or(false)
    }
}

impl fmt::Display for RenderContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "RenderContext(sample={}, block={}, sr={:.0}, transport=[{}], ratio={:.6})",
            self.sample_pos,
            self.samples_since_last,
            self.sample_rate,
            self.transport,
            self.speed_ratio,
        )
    }
}
