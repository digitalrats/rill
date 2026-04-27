/// Model of an operational amplifier with slew-rate limiting,
/// bandwidth roll-off, and voltage rail clamping.
#[derive(Debug, Clone)]
pub struct OperationalAmplifier {
    gain: f64,
    slew_rate: f64,
    bandwidth: f64,
    voltage_rails: (f64, f64),
    output_voltage: f64,
    internal_state: f64,
}

impl OperationalAmplifier {
    /// Create a new op-amp model.
    ///
    /// * `gain` — open-loop gain (V/V)
    /// * `slew_rate` — slew rate in V/µs
    /// * `bandwidth` — gain-bandwidth product (Hz)
    pub fn new(gain: f64, slew_rate: f64, bandwidth: f64) -> Self {
        Self {
            gain,
            slew_rate: slew_rate * 1e6,
            bandwidth,
            voltage_rails: (-15.0, 15.0),
            output_voltage: 0.0,
            internal_state: 0.0,
        }
    }

    /// Set positive and negative voltage rails
    pub fn set_voltage_rails(&mut self, negative: f64, positive: f64) {
        self.voltage_rails = (negative, positive);
    }

    /// Process one sample.
    ///
    /// * `input` — differential input voltage
    /// * `dt` — sample period (seconds)
    pub fn process(&mut self, input: f64, dt: f64) -> f64 {
        let ideal_output = input * self.gain;

        let max_change = self.slew_rate * dt;
        let output_change = ideal_output - self.internal_state;
        let limited_change = output_change.clamp(-max_change, max_change);
        self.internal_state += limited_change;

        let pole_frequency = self.bandwidth / self.gain;
        let alpha = 1.0 / (1.0 + 2.0 * std::f64::consts::PI * pole_frequency * dt);
        self.output_voltage = alpha * self.internal_state + (1.0 - alpha) * ideal_output;

        self.output_voltage = self
            .output_voltage
            .clamp(self.voltage_rails.0, self.voltage_rails.1);

        self.output_voltage
    }

    /// Reset internal state
    pub fn reset(&mut self) {
        self.output_voltage = 0.0;
        self.internal_state = 0.0;
    }

    /// Current output voltage
    pub fn output_voltage(&self) -> f64 {
        self.output_voltage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_amp_dc_gain() {
        let mut op = OperationalAmplifier::new(10.0, 0.5, 1e6);
        let output = op.process(0.5, 1.0 / 44100.0);
        assert!(output > 0.0);
    }

    #[test]
    fn test_op_amp_rail_clamp() {
        let mut op = OperationalAmplifier::new(100.0, 100.0, 1e6);
        let output = op.process(1.0, 1.0 / 44100.0);
        assert!(output <= 15.0);
        assert!(output >= -15.0);
    }

    #[test]
    fn test_op_amp_reset() {
        let mut op = OperationalAmplifier::new(10.0, 0.5, 1e6);
        op.process(0.5, 1.0 / 44100.0);
        op.reset();
        assert_eq!(op.output_voltage(), 0.0);
    }
}
