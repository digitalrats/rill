use crate::OperationalAmplifier;
use rill_core_wdf::{Capacitor, Inductor, WdfElement};

/// Record head model for analog tape.
///
/// Applies input amplification, bias oscillator mixing, and magnetic tape
/// nonlinearity (saturation + hysteresis). Output is the magnetized signal
/// ready to be stored on tape.
#[derive(Debug, Clone)]
pub struct RecordHeadModel {
    sample_rate: f64,
    input_amp: OperationalAmplifier,
    bias_oscillator: f64,
    record_head: Inductor<f64>,

    /// Tape speed in cm/s.
    pub tape_speed: f64,
    /// Bias level (0.0–1.0).
    pub bias_level: f64,
    /// Magnetic saturation strength.
    pub saturation: f64,
    /// Hysteresis effect strength.
    pub hysteresis: f64,

    tape_position: f64,
}

impl RecordHeadModel {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            input_amp: OperationalAmplifier::new(10.0, 0.5, 1e6),
            bias_oscillator: 100_000.0,
            record_head: Inductor::<f64>::new(100e-6, sample_rate),
            tape_speed: 4.76,
            bias_level: 0.8,
            saturation: 0.9,
            hysteresis: 0.1,
            tape_position: 0.0,
        }
    }

    pub fn set_tape_speed(&mut self, speed: f64) {
        self.tape_speed = speed.clamp(1.19, 19.05);
    }

    pub fn set_bias_level(&mut self, bias: f64) {
        self.bias_level = bias.clamp(0.0, 1.0);
    }

    /// Process one sample through the record chain.
    /// Returns the recorded (magnetized) signal.
    pub fn process(&mut self, input: f64) -> f64 {
        let dt = 1.0 / self.sample_rate;
        let amplified = self.input_amp.process(input, dt);

        let bias_phase = 2.0 * std::f64::consts::PI * self.bias_oscillator * dt;
        let bias_signal = self.bias_level * bias_phase.sin();

        let record_signal = amplified + bias_signal;
        let recorded = self.tape_nonlinearity(record_signal);

        let _head_current = recorded / self.record_head.port_resistance();
        self.tape_position += self.tape_speed * dt;

        recorded
    }

    fn tape_nonlinearity(&self, signal: f64) -> f64 {
        let saturated = signal.tanh() * self.saturation;
        let hysteresis_effect = self.hysteresis * signal.signum() * 0.01;
        saturated + hysteresis_effect
    }
}

/// Playback head model for analog tape.
///
/// Applies wow & flutter, head differentiator, gap loss, playback EQ,
/// print-through, tape noise, and output amplification.
#[derive(Debug, Clone)]
pub struct PlaybackHeadModel {
    sample_rate: f64,
    playback_head: Inductor<f64>,
    eq_filters: [Capacitor<f64>; 2],
    output_amp: OperationalAmplifier,

    /// Tape speed in cm/s.
    pub tape_speed: f64,
    /// Tape width in mm (affects noise floor).
    pub tape_width: f64,
    /// Tape noise floor amplitude.
    pub noise_floor: f64,
    /// Wow & flutter intensity factor.
    pub wow_flutter: f64,
    /// Print-through crosstalk factor.
    pub print_through: f64,

    wow_phase: f64,
    flutter_phase: f64,
    playback_head_flux: f64,
    playback_head_state: f64,
}

impl PlaybackHeadModel {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            playback_head: Inductor::<f64>::new(50e-6, sample_rate),
            eq_filters: [
                Capacitor::<f64>::new(100e-9, sample_rate),
                Capacitor::<f64>::new(1e-6, sample_rate),
            ],
            output_amp: OperationalAmplifier::new(5.0, 0.5, 1e6),
            tape_speed: 4.76,
            tape_width: 3.81,
            noise_floor: 0.0001,
            wow_flutter: 0.002,
            print_through: 0.01,
            wow_phase: 0.0,
            flutter_phase: 0.0,
            playback_head_flux: 0.0,
            playback_head_state: 0.0,
        }
    }

    pub fn set_tape_speed(&mut self, speed: f64) {
        self.tape_speed = speed.clamp(1.19, 19.05);
    }

    pub fn set_tape_width(&mut self, width: f64) {
        self.tape_width = width.clamp(1.0, 25.4);
    }

    /// Process one sample through the playback chain.
    /// `recorded_signal` is the magnetized signal read from tape.
    pub fn process(&mut self, recorded_signal: f64) -> f64 {
        let dt = 1.0 / self.sample_rate;

        let speed_variation = 1.0 + self.wow_and_flutter(dt);
        let tape_signal = recorded_signal * speed_variation;

        // Head differentiator: V ∝ dΦ/dt
        let head_flux = tape_signal;
        let flux_change = head_flux - self.playback_head_flux;
        self.playback_head_flux = head_flux;
        let head_signal = flux_change / dt;

        // Gap loss: first-order low-pass at ~18 kHz
        let gap_freq = 18000.0 * (self.tape_speed / 4.76);
        let gap_alpha = (2.0 * std::f64::consts::PI * gap_freq * dt)
            / (1.0 + 2.0 * std::f64::consts::PI * gap_freq * dt);
        self.playback_head_state =
            gap_alpha * head_signal + (1.0 - gap_alpha) * self.playback_head_state;

        // Head impedance voltage divider
        let head_z = self.playback_head.port_resistance();
        let head_output = self.playback_head_state * (head_z / (head_z + 1000.0));

        // Playback EQ: two-stage capacitive network
        let mut eq_signal = head_output;
        for filter in &mut self.eq_filters {
            let alpha = 1.0 / (1.0 + filter.port_resistance() * 1000.0);
            eq_signal *= alpha;
        }

        let print_through_signal = self.print_through * tape_signal;
        let noise = self.tape_noise();

        let final_signal = eq_signal + print_through_signal + noise;
        self.output_amp.process(final_signal, dt)
    }

    fn wow_and_flutter(&mut self, dt: f64) -> f64 {
        let wow_freq = 2.0;
        self.wow_phase += 2.0 * std::f64::consts::PI * wow_freq * dt;
        let wow = 0.01 * self.wow_flutter * self.wow_phase.sin();

        let flutter_freq = 30.0;
        self.flutter_phase += 2.0 * std::f64::consts::PI * flutter_freq * dt;
        let flutter = 0.005 * self.wow_flutter * self.flutter_phase.sin();

        wow + flutter
    }

    fn tape_noise(&self) -> f64 {
        let width_factor = (3.81 / self.tape_width).sqrt();
        let white_noise = (rand::random::<f64>() - 0.5) * 2.0;
        let pink_noise = white_noise * self.noise_floor * width_factor;

        let click_probability = 0.0001;
        let click = if rand::random::<f64>() < click_probability {
            (rand::random::<f64>() - 0.5) * 0.1
        } else {
            0.0
        };

        pink_noise + click
    }
}

/// Cassette deck model (Sony TC-260 style).
///
/// Combines `RecordHeadModel` and `PlaybackHeadModel` for the full
/// record + playback chain. For tape delay applications, use the
/// head models separately with `TapeLoop`.
#[derive(Debug, Clone)]
pub struct CassetteDeckModel {
    record: RecordHeadModel,
    playback: PlaybackHeadModel,
}

impl CassetteDeckModel {
    /// Create a new cassette deck model at the given sample rate.
    pub fn new(sample_rate: f64) -> Self {
        Self {
            record: RecordHeadModel::new(sample_rate),
            playback: PlaybackHeadModel::new(sample_rate),
        }
    }

    /// Set the tape speed (clamped to 1.19–19.05 cm/s).
    pub fn set_tape_speed(&mut self, speed_cm_per_sec: f64) {
        self.record.set_tape_speed(speed_cm_per_sec);
        self.playback.set_tape_speed(speed_cm_per_sec);
    }

    /// Set the tape width in mm (clamped to 1.0–25.4).
    pub fn set_tape_width(&mut self, width_mm: f64) {
        self.playback.set_tape_width(width_mm);
    }

    /// Set the bias level (clamped to 0.0–1.0).
    pub fn set_bias_level(&mut self, bias: f64) {
        self.record.set_bias_level(bias);
    }

    /// Access the record head model.
    pub fn record_head(&self) -> &RecordHeadModel {
        &self.record
    }

    /// Access the record head model mutably.
    pub fn record_head_mut(&mut self) -> &mut RecordHeadModel {
        &mut self.record
    }

    /// Access the playback head model.
    pub fn playback_head(&self) -> &PlaybackHeadModel {
        &self.playback
    }

    /// Access the playback head model mutably.
    pub fn playback_head_mut(&mut self) -> &mut PlaybackHeadModel {
        &mut self.playback
    }

    /// Process recording step (delegates to `RecordHeadModel`).
    pub fn process_record(&mut self, input: f64) -> f64 {
        self.record.process(input)
    }

    /// Process playback step (delegates to `PlaybackHeadModel`).
    pub fn process_playback(&mut self, recorded_signal: f64) -> f64 {
        self.playback.process(recorded_signal)
    }

    /// Process one sample through record and playback chain.
    pub fn process(&mut self, input: f64) -> f64 {
        let recorded = self.record.process(input);
        self.playback.process(recorded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cassette_deck_process() {
        let mut deck = CassetteDeckModel::new(44100.0);
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
        let mut deck = CassetteDeckModel::new(44100.0);
        deck.set_tape_speed(9.52);
        deck.set_bias_level(0.9);
        assert!((deck.record.tape_speed - 9.52).abs() < 1e-10);
        assert!((deck.record.bias_level - 0.9).abs() < 1e-10);
    }
}
