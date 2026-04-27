use rill_core_wdf::{Capacitor, Inductor, WdfElement};
use crate::OperationalAmplifier;

/// Cassette deck model (Sony TC-260 style)
///
/// Models tape recording and playback with non-linearities,
/// wow & flutter, and tape noise.
#[derive(Debug, Clone)]
pub struct CassetteDeckModel {
    pub sample_rate: f64,

    input_amp: OperationalAmplifier,
    bias_oscillator: f64,
    record_head: Inductor<f64>,
    #[allow(dead_code)]
    playback_head: Inductor<f64>,
    eq_filters: [Capacitor<f64>; 2],
    output_amp: OperationalAmplifier,

    pub tape_speed: f64,
    #[allow(dead_code)]
    tape_width: f64,
    pub bias_level: f64,
    pub noise_floor: f64,

    hysteresis: f64,
    saturation: f64,
    print_through: f64,
    pub wow_flutter: f64,

    tape_position: f64,
    wow_phase: f64,
    flutter_phase: f64,
}

impl CassetteDeckModel {
    /// Create a new cassette deck model at the given sample rate
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,

            input_amp: OperationalAmplifier::new(10.0, 0.5, 1e6),
            bias_oscillator: 100_000.0,
            record_head: Inductor::<f64>::new(100e-6, sample_rate),
            playback_head: Inductor::<f64>::new(50e-6, sample_rate),
            eq_filters: [
                Capacitor::<f64>::new(100e-9, sample_rate),
                Capacitor::<f64>::new(1e-6, sample_rate),
            ],
            output_amp: OperationalAmplifier::new(5.0, 0.5, 1e6),

            tape_speed: 4.76,
            tape_width: 3.81,
            bias_level: 0.8,
            noise_floor: 0.0001,

            hysteresis: 0.1,
            saturation: 0.9,
            print_through: 0.01,
            wow_flutter: 0.002,

            tape_position: 0.0,
            wow_phase: 0.0,
            flutter_phase: 0.0,
        }
    }

    pub fn set_tape_speed(&mut self, speed_cm_per_sec: f64) {
        self.tape_speed = speed_cm_per_sec.clamp(1.19, 19.05);
    }

    pub fn set_bias_level(&mut self, bias: f64) {
        self.bias_level = bias.clamp(0.0, 1.0);
    }

    fn tape_nonlinearity(&self, signal: f64) -> f64 {
        let saturated = signal.tanh() * self.saturation;
        let hysteresis_effect = self.hysteresis * signal.signum() * 0.01;
        saturated + hysteresis_effect
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
        let white_noise = (rand::random::<f64>() - 0.5) * 2.0;
        let pink_noise = white_noise * self.noise_floor;

        let click_probability = 0.0001;
        let click = if rand::random::<f64>() < click_probability {
            (rand::random::<f64>() - 0.5) * 0.1
        } else {
            0.0
        };

        pink_noise + click
    }

    /// Process recording step
    pub fn process_record(&mut self, input: f64) -> f64 {
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

    /// Process playback step
    pub fn process_playback(&mut self, recorded_signal: f64) -> f64 {
        let dt = 1.0 / self.sample_rate;

        let speed_variation = 1.0 + self.wow_and_flutter(dt);
        let playback_voltage = recorded_signal * speed_variation;

        let mut eq_signal = playback_voltage;
        for filter in &mut self.eq_filters {
            let alpha = 1.0 / (1.0 + filter.port_resistance() * 1000.0);
            eq_signal = alpha * eq_signal;
        }

        let print_through_signal = self.print_through * playback_voltage;
        let noise = self.tape_noise();

        let final_signal = eq_signal + print_through_signal + noise;
        self.output_amp.process(final_signal, dt)
    }

    /// Process one sample through record and playback chain
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
        assert!((deck.tape_speed - 9.52).abs() < 1e-10);
        assert!((deck.bias_level - 0.9).abs() < 1e-10);
    }
}
