//! Utility functions and helpers for the patchbay.
//!
//! Provides value converters, metronome timing, note-type conversion,
//! test helpers, and test-signal generation.

use crate::automaton::Range;
use crate::control::Transform;

// =============================================================================
// Value converters
// =============================================================================

/// Converts values between different scales using a specified transform.
#[derive(Debug, Clone)]
pub struct ValueConverter {
    input_range: Range,
    output_range: Range,
    transform: Transform,
}

impl ValueConverter {
    /// Create a new value converter.
    pub fn new(input_range: Range, output_range: Range, transform: Transform) -> Self {
        Self {
            input_range,
            output_range,
            transform,
        }
    }

    /// Convert a value from the input range to the output range.
    pub fn convert(&self, value: f64) -> f64 {
        let norm = self.input_range.normalize(value);

        let transformed = match self.transform {
            Transform::Linear => norm,
            Transform::Exponential => norm * norm,
            Transform::Logarithmic => (1.0 + norm * 9.0).log10(),
            Transform::Inverted => 1.0 - norm,
            Transform::Custom(ref f) => f(norm as f32) as f64,
        };

        self.output_range.denormalize(transformed)
    }

    /// Convert a value in the reverse direction (approximate).
    pub fn convert_inverse(&self, value: f64) -> f64 {
        let norm = self.output_range.normalize(value);
        self.input_range.denormalize(norm)
    }
}

/// Convert a MIDI value (0–127) to a normalised float (0.0–1.0).
pub fn midi_to_normalized(midi: u8) -> f64 {
    midi as f64 / 127.0
}

/// Convert a normalised float (0.0–1.0) to a MIDI value (0–127).
pub fn normalized_to_midi(norm: f64) -> u8 {
    (norm.clamp(0.0, 1.0) * 127.0).round() as u8
}

/// Convert a frequency in Hz to the nearest MIDI note number.
pub fn freq_to_midi_note(freq: f64) -> f64 {
    69.0 + 12.0 * (freq / 440.0).log2()
}

/// Convert a MIDI note number to frequency in Hz.
pub fn midi_note_to_freq(note: f64) -> f64 {
    440.0 * 2.0_f64.powf((note - 69.0) / 12.0)
}

// =============================================================================
// Timing utilities
// =============================================================================

/// A metronome for synchronisation with BPM.
#[derive(Debug, Clone)]
pub struct Metronome {
    bpm: f64,
    last_tick: f64,
    next_tick: f64,
    quarter_duration: f64,
}

impl Metronome {
    /// Create a new metronome at the given BPM.
    pub fn new(bpm: f64) -> Self {
        let quarter_duration = 60.0 / bpm;
        Self {
            bpm,
            last_tick: 0.0,
            next_tick: quarter_duration,
            quarter_duration,
        }
    }

    /// Advance the metronome and return whether a tick occurred.
    pub fn update(&mut self, time: f64) -> bool {
        if time >= self.next_tick {
            self.last_tick = self.next_tick;
            self.next_tick += self.quarter_duration;
            true
        } else {
            false
        }
    }

    /// Get the current phase (0.0–1.0) within the current quarter note.
    pub fn phase(&self, time: f64) -> f64 {
        ((time - self.last_tick) / self.quarter_duration).clamp(0.0, 1.0)
    }

    /// Set a new BPM value.
    pub fn set_bpm(&mut self, bpm: f64) {
        self.bpm = bpm;
        self.quarter_duration = 60.0 / bpm;
        self.next_tick = self.last_tick + self.quarter_duration;
    }

    /// Reset the metronome to the start of a new bar.
    pub fn reset(&mut self) {
        self.last_tick = 0.0;
        self.next_tick = self.quarter_duration;
    }
}

/// Convert a note type to duration in seconds at the given BPM.
pub fn note_duration_to_seconds(note_type: NoteType, bpm: f64) -> f64 {
    let quarter = 60.0 / bpm;
    match note_type {
        NoteType::Whole => quarter * 4.0,
        NoteType::Half => quarter * 2.0,
        NoteType::Quarter => quarter,
        NoteType::Eighth => quarter / 2.0,
        NoteType::Sixteenth => quarter / 4.0,
        NoteType::ThirtySecond => quarter / 8.0,
        NoteType::Dotted(n) => note_duration_to_seconds(*n, bpm) * 1.5,
        NoteType::Triplet(n) => note_duration_to_seconds(*n, bpm) * 2.0 / 3.0,
    }
}

/// A musical note type for duration calculations.
#[derive(Debug, Clone)]
pub enum NoteType {
    /// Whole note (semibreve).
    Whole,
    /// Half note (minim).
    Half,
    /// Quarter note (crotchet).
    Quarter,
    /// Eighth note (quaver).
    Eighth,
    /// Sixteenth note (semiquaver).
    Sixteenth,
    /// Thirty-second note (demisemiquaver).
    ThirtySecond,
    /// Dotted variant of a note type.
    Dotted(Box<NoteType>),
    /// Triplet variant of a note type.
    Triplet(Box<NoteType>),
}

// =============================================================================
// Test helpers
// =============================================================================

/// Records events for testing purposes.
#[derive(Debug, Default)]
pub struct EventRecorder {
    events: Vec<RecordedEvent>,
}

/// A single recorded event.
#[derive(Debug, Clone)]
pub struct RecordedEvent {
    /// Time of the event.
    pub time: f64,
    /// Event type label.
    pub event_type: String,
    /// Numeric value.
    pub value: f64,
    /// Additional data.
    pub data: String,
}

impl EventRecorder {
    /// Create a new event recorder.
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Record an event.
    pub fn record(&mut self, time: f64, event_type: &str, value: f64, data: &str) {
        self.events.push(RecordedEvent {
            time,
            event_type: event_type.to_string(),
            value,
            data: data.to_string(),
        });
    }

    /// Return all recorded events.
    pub fn events(&self) -> &[RecordedEvent] {
        &self.events
    }

    /// Clear all recorded events.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Find events by type label.
    pub fn find_by_type(&self, event_type: &str) -> Vec<&RecordedEvent> {
        self.events
            .iter()
            .filter(|e| e.event_type == event_type)
            .collect()
    }
}

// =============================================================================
// Test signal generators
// =============================================================================

/// Generates test signals for verification and debugging.
pub struct TestSignalGenerator {
    signal_type: TestSignalType,
    params: TestSignalParams,
}

/// Type of test signal.
#[derive(Debug, Clone)]
pub enum TestSignalType {
    /// Sine wave.
    Sine,
    /// Square wave.
    Square,
    /// Sawtooth wave.
    Saw,
    /// White noise.
    Noise,
    /// ADSR-like envelope.
    Envelope,
}

/// Parameters for a test signal.
#[derive(Debug, Clone)]
pub struct TestSignalParams {
    /// Frequency in Hz.
    pub frequency: f64,
    /// Amplitude.
    pub amplitude: f64,
    /// DC offset.
    pub offset: f64,
    /// Duration in seconds.
    pub duration: f64,
}

impl TestSignalGenerator {
    /// Create a new test signal generator.
    pub fn new(signal_type: TestSignalType, params: TestSignalParams) -> Self {
        Self {
            signal_type,
            params,
        }
    }

    /// Generate the signal value at the given time.
    pub fn generate(&self, time: f64) -> f64 {
        if time > self.params.duration {
            return 0.0;
        }

        match self.signal_type {
            TestSignalType::Sine => {
                let phase = 2.0 * std::f64::consts::PI * self.params.frequency * time;
                self.params.offset + self.params.amplitude * phase.sin()
            }

            TestSignalType::Square => {
                let phase = (self.params.frequency * time) % 1.0;
                let value = if phase < 0.5 { 1.0 } else { -1.0 };
                self.params.offset + self.params.amplitude * value
            }

            TestSignalType::Saw => {
                let phase = (self.params.frequency * time) % 1.0;
                let value = 2.0 * phase - 1.0;
                self.params.offset + self.params.amplitude * value
            }

            TestSignalType::Noise => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                self.params.offset + self.params.amplitude * (rng.gen::<f64>() * 2.0 - 1.0)
            }

            TestSignalType::Envelope => {
                let attack = 0.1;
                let decay = 0.2;
                let sustain = 0.7;
                let release = 0.3;

                if time < attack {
                    (time / attack) * self.params.amplitude
                } else if time < attack + decay {
                    (1.0 - (1.0 - sustain) * ((time - attack) / decay)) * self.params.amplitude
                } else if time < self.params.duration - release {
                    sustain * self.params.amplitude
                } else {
                    let rel_time = time - (self.params.duration - release);
                    (sustain * (1.0 - rel_time / release)) * self.params.amplitude
                }
            }
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_converter() {
        let converter = ValueConverter::new(
            Range::new(0.0, 127.0),
            Range::new(0.0, 1.0),
            Transform::Linear,
        );

        let result = converter.convert(64.0);
        assert!((result - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_metronome() {
        let mut metro = Metronome::new(120.0);

        assert!(!metro.update(0.2));
        assert!(metro.update(0.6));
        assert!((metro.phase(0.6) - 0.2).abs() < 0.01);
    }

    #[test]
    fn test_test_signal() {
        let params = TestSignalParams {
            frequency: 1.0,
            amplitude: 1.0,
            offset: 0.0,
            duration: 2.0,
        };

        let gen = TestSignalGenerator::new(TestSignalType::Sine, params);
        let val = gen.generate(0.25);
        assert!((val - 1.0).abs() < 0.01);
    }
}
