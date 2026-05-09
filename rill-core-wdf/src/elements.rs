use crate::constants::{BOLTZMANN, ELECTRON_CHARGE, NEWTON_TOLERANCE};
use crate::WdfElement;
use rill_core::Transcendental;

/// Resistor WDF element
#[derive(Debug, Clone, Copy)]
pub struct Resistor<T: Transcendental> {
    resistance: T,
    port_resistance: T,
    voltage: T,
    current: T,
}

impl<T: Transcendental> Resistor<T> {
    /// Create a new resistor with given resistance in ohms
    pub fn new(resistance: T) -> Self {
        Self {
            port_resistance: resistance,
            resistance,
            voltage: T::ZERO,
            current: T::ZERO,
        }
    }

    /// Get resistance value
    pub fn resistance(&self) -> T {
        self.resistance
    }
}

impl<T: Transcendental> WdfElement<T> for Resistor<T> {
    fn port_resistance(&self) -> T {
        self.port_resistance
    }

    fn process_incident(&mut self, _a: T) -> T {
        T::ZERO
    }

    fn update_state(&mut self) {
        self.voltage = self.current * self.resistance;
    }

    fn voltage(&self) -> T {
        self.voltage
    }

    fn current(&self) -> T {
        self.current
    }

    fn reset(&mut self) {
        self.voltage = T::ZERO;
        self.current = T::ZERO;
    }
}

/// Capacitor WDF element (trapezoidal integration)
#[derive(Debug, Clone, Copy)]
pub struct Capacitor<T: Transcendental> {
    capacitance: T,
    sample_rate: T,
    port_resistance: T,
    voltage: T,
    current: T,
    state: T,
}

impl<T: Transcendental> Capacitor<T> {
    /// Create a new capacitor with given capacitance in farads and sample rate
    pub fn new(capacitance: T, sample_rate: T) -> Self {
        let two = T::from_f32(2.0);
        let t = T::ONE / sample_rate;
        let port_resistance = t / (two * capacitance);

        Self {
            capacitance,
            sample_rate,
            port_resistance,
            voltage: T::ZERO,
            current: T::ZERO,
            state: T::ZERO,
        }
    }

    /// Get capacitance value
    pub fn capacitance(&self) -> T {
        self.capacitance
    }

    /// Set capacitance and recompute port resistance
    pub fn set_capacitance(&mut self, capacitance: T) {
        self.capacitance = capacitance;
        let two = T::from_f32(2.0);
        let t = T::ONE / self.sample_rate;
        self.port_resistance = t / (two * capacitance);
    }

    /// Set sample rate and recompute port resistance
    pub fn set_sample_rate(&mut self, sample_rate: T) {
        self.sample_rate = sample_rate;
        let two = T::from_f32(2.0);
        let t = T::ONE / sample_rate;
        self.port_resistance = t / (two * self.capacitance);
    }
}

impl<T: Transcendental> WdfElement<T> for Capacitor<T> {
    fn port_resistance(&self) -> T {
        self.port_resistance
    }

    fn process_incident(&mut self, a: T) -> T {
        let two = T::from_f32(2.0);
        let b = self.state - a;
        self.voltage = (a + b) / two;
        self.current = (a - b) / (two * self.port_resistance);
        let next_state = self.voltage + self.port_resistance * self.current;
        self.state = next_state;
        b
    }

    fn update_state(&mut self) {
        // state already updated in process_incident
    }

    fn voltage(&self) -> T {
        self.voltage
    }

    fn current(&self) -> T {
        self.current
    }

    fn reset(&mut self) {
        self.voltage = T::ZERO;
        self.current = T::ZERO;
        self.state = T::ZERO;
    }
}

/// Inductor WDF element (trapezoidal integration)
#[derive(Debug, Clone, Copy)]
pub struct Inductor<T: Transcendental> {
    inductance: T,
    sample_rate: T,
    port_resistance: T,
    voltage: T,
    current: T,
    state: T,
}

impl<T: Transcendental> Inductor<T> {
    /// Create a new inductor with given inductance in henries and sample rate
    pub fn new(inductance: T, sample_rate: T) -> Self {
        let two = T::from_f32(2.0);
        let t = T::ONE / sample_rate;
        let port_resistance = two * inductance / t;

        Self {
            inductance,
            sample_rate,
            port_resistance,
            voltage: T::ZERO,
            current: T::ZERO,
            state: T::ZERO,
        }
    }
}

impl<T: Transcendental> WdfElement<T> for Inductor<T> {
    fn port_resistance(&self) -> T {
        self.port_resistance
    }

    fn process_incident(&mut self, _a: T) -> T {
        -self.state
    }

    fn update_state(&mut self) {
        self.state = self.current * self.port_resistance;

        let t = T::ONE / self.sample_rate;
        self.current += self.voltage * t / self.inductance;
    }

    fn voltage(&self) -> T {
        self.voltage
    }

    fn current(&self) -> T {
        self.current
    }

    fn reset(&mut self) {
        self.voltage = T::ZERO;
        self.current = T::ZERO;
        self.state = T::ZERO;
    }
}

/// Diode WDF element (nonlinear, Newton-Raphson solution)
#[derive(Debug, Clone, Copy)]
pub struct Diode<T: Transcendental> {
    pub(crate) saturation_current: T,
    pub(crate) thermal_voltage: T,
    pub(crate) ideality_factor: T,
    pub(crate) port_resistance: T,
    pub(crate) voltage: T,
    pub(crate) current: T,
    last_b: T,
}

impl<T: Transcendental> Diode<T> {
    /// Create a new diode with Shockley parameters
    ///
    /// * `saturation_current` - Reverse saturation current Is (amperes)
    /// * `ideality_factor` - Ideality factor n (1-2)
    /// * `temperature_k` - Temperature in Kelvin
    pub fn new(saturation_current: T, ideality_factor: T, temperature_k: T) -> Self {
        let k = T::from_f64(BOLTZMANN);
        let q = T::from_f64(ELECTRON_CHARGE);
        let thermal_voltage = (k * temperature_k) / q;
        let port_resistance = thermal_voltage / saturation_current;

        Self {
            saturation_current,
            thermal_voltage,
            ideality_factor,
            port_resistance,
            voltage: T::ZERO,
            current: T::ZERO,
            last_b: T::ZERO,
        }
    }

    /// Get saturation current
    pub fn saturation_current(&self) -> T {
        self.saturation_current
    }

    /// Get thermal voltage
    pub fn thermal_voltage(&self) -> T {
        self.thermal_voltage
    }

    pub(crate) fn diode_equation(&self, v: T) -> T {
        let vt = self.thermal_voltage * self.ideality_factor;
        self.saturation_current * ((v / vt).exp() - T::ONE)
    }

    pub(crate) fn diode_derivative(&self, v: T) -> T {
        let vt = self.thermal_voltage * self.ideality_factor;
        self.saturation_current * (v / vt).exp() / vt
    }

    pub(crate) fn solve_newton(&self, a: T, r: T) -> T {
        let vt = self.thermal_voltage * self.ideality_factor;
        // Improved initial guess using simplified diode equation.
        // For small a: v ≈ a / (1 + r*Is/vt)
        // For large a: v ≈ vt * ln(a / (r*Is))
        // Using a smoother approximation: v ≈ vt * ln(1 + a/(r*Is))
        let guess = vt * (T::ONE + a / (r * self.saturation_current)).ln();
        let mut v = guess.max(T::ZERO);
        let tolerance = T::from_f64(NEWTON_TOLERANCE);

        for _ in 0..10 {
            let i = self.diode_equation(v);
            let g = self.diode_derivative(v);

            let f = v + r * i - a;

            if f.abs() < tolerance {
                break;
            }

            let df = T::ONE + r * g;
            v -= f / df;
        }

        v
    }
}

impl<T: Transcendental> WdfElement<T> for Diode<T> {
    fn port_resistance(&self) -> T {
        self.port_resistance
    }

    fn process_incident(&mut self, a: T) -> T {
        let v = self.solve_newton(a, self.port_resistance);
        let i = self.diode_equation(v);

        self.voltage = v;
        self.current = i;

        T::from_f32(2.0) * v - a
    }

    fn update_state(&mut self) {
        let g = self.diode_derivative(self.voltage);
        if g > T::ZERO {
            self.port_resistance = T::ONE / g;
        }
    }

    fn voltage(&self) -> T {
        self.voltage
    }

    fn current(&self) -> T {
        self.current
    }

    fn reset(&mut self) {
        self.voltage = T::ZERO;
        self.current = T::ZERO;
        self.last_b = T::ZERO;
    }
}

/// Operational amplifier model with slew-rate limiting, bandwidth roll-off,
/// and voltage rail clamping.
#[derive(Debug, Clone)]
pub struct OpAmp<T: Transcendental> {
    gain: T,
    slew_rate: T,
    bandwidth: T,
    pos_rail: T,
    neg_rail: T,
    state: T,
    output: T,
}

impl<T: Transcendental> OpAmp<T> {
    /// Create a new op-amp model.
    ///
    /// * `gain` — open-loop gain (V/V)
    /// * `slew_rate` — slew rate in V/µs
    /// * `bandwidth` — gain-bandwidth product (Hz)
    pub fn new(gain: f64, slew_rate: f64, bandwidth: f64) -> Self {
        Self {
            gain: T::from_f64(gain),
            slew_rate: T::from_f64(slew_rate * 1e6),
            bandwidth: T::from_f64(bandwidth),
            pos_rail: T::from_f64(15.0),
            neg_rail: T::from_f64(-15.0),
            state: T::ZERO,
            output: T::ZERO,
        }
    }

    /// Set output voltage rails.
    pub fn set_rails(&mut self, neg: T, pos: T) {
        self.neg_rail = neg;
        self.pos_rail = pos;
    }

    /// Process one sample.
    ///
    /// * `input` — differential input voltage
    /// * `dt` — sample period in seconds
    pub fn process(&mut self, input: T, dt: T) -> T {
        let ideal = input * self.gain;

        let max_change = self.slew_rate * dt;
        let diff = ideal - self.state;
        let limited = diff.clamp(-max_change, max_change);
        self.state += limited;

        let pole_freq = self.bandwidth / self.gain;
        let two_pi = T::from_f32(2.0) * T::PI;
        let alpha = T::ONE / (T::ONE + two_pi * pole_freq * dt);
        self.output = alpha * self.state + (T::ONE - alpha) * ideal;

        self.output = self.output.clamp(self.neg_rail, self.pos_rail);
        self.output
    }

    /// Reset internal state.
    pub fn reset(&mut self) {
        self.state = T::ZERO;
        self.output = T::ZERO;
    }

    /// Current output voltage.
    pub fn output_voltage(&self) -> T {
        self.output
    }
}

impl<T: Transcendental> WdfElement<T> for OpAmp<T> {
    fn port_resistance(&self) -> T {
        T::from_f32(100.0)
    }

    fn process_incident(&mut self, a: T) -> T {
        self.process(a, T::ONE / T::from_f32(44100.0))
    }

    fn update_state(&mut self) {}

    fn voltage(&self) -> T {
        self.output
    }

    fn current(&self) -> T {
        T::ZERO
    }

    fn reset(&mut self) {
        self.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resistor_wdf() {
        let mut resistor: Resistor<f64> = Resistor::new(1000.0);
        assert_eq!(resistor.port_resistance(), 1000.0);

        let b = resistor.process_incident(1.0);
        assert!((b - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_capacitor_wdf() {
        let sample_rate = 44100.0;
        let capacitance = 1e-6;
        let capacitor: Capacitor<f64> = Capacitor::new(capacitance, sample_rate);

        let expected_r = 1.0 / (sample_rate * 2.0 * capacitance);
        assert!((capacitor.port_resistance() - expected_r).abs() < 1e-10);
    }

    #[test]
    fn test_inductor_wdf() {
        let sample_rate = 44100.0;
        let inductance = 100e-6;
        let inductor: Inductor<f64> = Inductor::new(inductance, sample_rate);

        let t = 1.0 / sample_rate;
        let expected_r = 2.0 * inductance / t;
        assert!((inductor.port_resistance() - expected_r).abs() < 1e-10);
    }

    #[test]
    fn test_diode_thermal_voltage() {
        let diode: Diode<f64> = Diode::new(1e-9, 1.0, 300.0);
        let expected_vt = 1.380649e-23 * 300.0 / 1.60217662e-19;
        assert!((diode.thermal_voltage() - expected_vt).abs() < 1e-15);
    }
}
