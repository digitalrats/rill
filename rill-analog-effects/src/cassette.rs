use rill_core_model::tape::{PlaybackHead, RecordHead};
use rill_core_model::OpAmp;

/// Cassette deck model (Sony TC-260 style).
///
/// Combines `RecordHead`, `PlaybackHead`, and op-amp stages for the full
/// record + playback chain. For tape delay applications, use the head
/// models directly with `TapeLoop`.
#[derive(Debug, Clone)]
pub struct CassetteDeck {
    sample_rate: f64,
    record: RecordHead<f64>,
    playback: PlaybackHead<f64>,
    input_amp: OpAmp<f64>,
    output_amp: OpAmp<f64>,
}

impl CassetteDeck {
    /// Create a new cassette deck model at the given sample rate.
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            record: RecordHead::<f64>::new(sample_rate as f32),
            playback: PlaybackHead::<f64>::new(sample_rate as f32),
            input_amp: OpAmp::<f64>::new(10.0, 0.5, 1e6),
            output_amp: OpAmp::<f64>::new(5.0, 0.5, 1e6),
        }
    }

    /// Set the tape speed (clamped to 1.19–19.05 cm/s).
    pub fn set_tape_speed(&mut self, speed_cm_per_sec: f64) {
        self.record.tape_speed = speed_cm_per_sec.clamp(1.19, 19.05);
        self.playback.tape_speed = speed_cm_per_sec.clamp(1.19, 19.05);
    }

    /// Set the tape width in mm (clamped to 1.0–25.4).
    pub fn set_tape_width(&mut self, width_mm: f64) {
        self.playback.tape_width = width_mm.clamp(1.0, 25.4);
    }

    /// Set the bias level (clamped to 0.0–1.0).
    pub fn set_bias_level(&mut self, bias: f64) {
        self.record.bias_level = bias.clamp(0.0, 1.0);
    }

    /// Access the record head model.
    pub fn record_head(&self) -> &RecordHead<f64> {
        &self.record
    }

    /// Access the record head model mutably.
    pub fn record_head_mut(&mut self) -> &mut RecordHead<f64> {
        &mut self.record
    }

    /// Access the playback head model.
    pub fn playback_head(&self) -> &PlaybackHead<f64> {
        &self.playback
    }

    /// Access the playback head model mutably.
    pub fn playback_head_mut(&mut self) -> &mut PlaybackHead<f64> {
        &mut self.playback
    }

    /// Process recording step: input_amp → record_head physics.
    pub fn process_record(&mut self, input: f64) -> f64 {
        let dt = 1.0 / self.sample_rate;
        let amplified = self.input_amp.process(input, dt);
        self.record.process_sample(amplified)
    }

    /// Process playback step: playback_head physics → output_amp.
    pub fn process_playback(&mut self, recorded_signal: f64) -> f64 {
        let dt = 1.0 / self.sample_rate;
        let signal = self.playback.process_sample(recorded_signal);
        self.output_amp.process(signal, dt)
    }

    /// Process one sample through record and playback chain.
    pub fn process(&mut self, input: f64) -> f64 {
        let recorded = self.process_record(input);
        self.process_playback(recorded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cassette_deck_process() {
        let mut deck = CassetteDeck::new(44100.0);
        let test_freq = 1000.0;
        let num_samples = 4410;

        let mut max_output = 0.0;
        for i in 0..num_samples {
            let t = i as f64 / 44100.0;
            let input = (2.0 * std::f64::consts::PI * test_freq * t).sin() * 0.3;
            let output = deck.process(input);
            if output.abs() > max_output {
                max_output = output.abs();
            }
        }

        assert!(max_output > 0.0);
        assert!(max_output <= 1.0);
    }

    #[test]
    fn test_cassette_deck_set_params() {
        let mut deck = CassetteDeck::new(44100.0);
        deck.set_tape_speed(9.52);
        deck.set_bias_level(0.9);
        assert!((deck.record.tape_speed - 9.52).abs() < 1e-10);
        assert!((deck.record.bias_level - 0.9).abs() < 1e-10);
    }
}
