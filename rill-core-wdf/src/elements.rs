use crate::WdfElement;

/// Resistor WDF element
#[derive(Debug, Clone)]
pub struct Resistor {
    resistance: f64,
    port_resistance: f64,
    voltage: f64,
    current: f64,
}

impl Resistor {
    /// Create a new resistor with given resistance in ohms
    pub fn new(resistance: f64) -> Self {
        Self {
            resistance,
            port_resistance: resistance,
            voltage: 0.0,
            current: 0.0,
        }
    }

    /// Get resistance value
    pub fn resistance(&self) -> f64 {
        self.resistance
    }
}

impl WdfElement for Resistor {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }

    fn process_incident(&mut self, _a: f64) -> f64 {
        0.0
    }

    fn update_state(&mut self) {
        self.voltage = self.current * self.resistance;
    }

    fn voltage(&self) -> f64 {
        self.voltage
    }

    fn current(&self) -> f64 {
        self.current
    }

    fn reset(&mut self) {
        self.voltage = 0.0;
        self.current = 0.0;
    }
}

/// Capacitor WDF element (trapezoidal integration)
#[derive(Debug, Clone)]
pub struct Capacitor {
    capacitance: f64,
    sample_rate: f64,
    port_resistance: f64,
    voltage: f64,
    current: f64,
    state: f64,
}

impl Capacitor {
    /// Create a new capacitor with given capacitance in farads and sample rate
    pub fn new(capacitance: f64, sample_rate: f64) -> Self {
        let t = 1.0 / sample_rate;
        let port_resistance = t / (2.0 * capacitance);

        Self {
            capacitance,
            sample_rate,
            port_resistance,
            voltage: 0.0,
            current: 0.0,
            state: 0.0,
        }
    }

    /// Get capacitance value
    pub fn capacitance(&self) -> f64 {
        self.capacitance
    }

    /// Set capacitance and recompute port resistance
    pub fn set_capacitance(&mut self, capacitance: f64) {
        self.capacitance = capacitance;
        let t = 1.0 / self.sample_rate;
        self.port_resistance = t / (2.0 * capacitance);
    }

    /// Set sample rate and recompute port resistance
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        let t = 1.0 / sample_rate;
        self.port_resistance = t / (2.0 * self.capacitance);
    }
}

impl WdfElement for Capacitor {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }

    fn process_incident(&mut self, a: f64) -> f64 {
        self.state - a
    }

    fn update_state(&mut self) {
        self.state = -self.current * self.port_resistance;

        let t = 1.0 / self.sample_rate;
        self.voltage += self.current * t / self.capacitance;
    }

    fn voltage(&self) -> f64 {
        self.voltage
    }

    fn current(&self) -> f64 {
        self.current
    }

    fn reset(&mut self) {
        self.voltage = 0.0;
        self.current = 0.0;
        self.state = 0.0;
    }
}

/// Inductor WDF element (trapezoidal integration)
#[derive(Debug, Clone)]
pub struct Inductor {
    inductance: f64,
    sample_rate: f64,
    port_resistance: f64,
    voltage: f64,
    current: f64,
    state: f64,
}

impl Inductor {
    /// Create a new inductor with given inductance in henries and sample rate
    pub fn new(inductance: f64, sample_rate: f64) -> Self {
        let t = 1.0 / sample_rate;
        let port_resistance = 2.0 * inductance / t;

        Self {
            inductance,
            sample_rate,
            port_resistance,
            voltage: 0.0,
            current: 0.0,
            state: 0.0,
        }
    }
}

impl WdfElement for Inductor {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }

    fn process_incident(&mut self, _a: f64) -> f64 {
        -self.state
    }

    fn update_state(&mut self) {
        self.state = self.current * self.port_resistance;

        let t = 1.0 / self.sample_rate;
        self.current += self.voltage * t / self.inductance;
    }

    fn voltage(&self) -> f64 {
        self.voltage
    }

    fn current(&self) -> f64 {
        self.current
    }

    fn reset(&mut self) {
        self.voltage = 0.0;
        self.current = 0.0;
        self.state = 0.0;
    }
}

/// Diode WDF element (nonlinear, Newton-Raphson solution)
#[derive(Debug, Clone)]
pub struct Diode {
    saturation_current: f64,
    thermal_voltage: f64,
    ideality_factor: f64,
    port_resistance: f64,
    voltage: f64,
    current: f64,
    last_b: f64,
}

impl Diode {
    /// Create a new diode with Shockley parameters
    ///
    /// * `saturation_current` - Reverse saturation current Is (amperes)
    /// * `ideality_factor` - Ideality factor n (1-2)
    /// * `temperature_k` - Temperature in Kelvin
    pub fn new(saturation_current: f64, ideality_factor: f64, temperature_k: f64) -> Self {
        let k = 1.380649e-23;
        let q = 1.60217662e-19;
        let thermal_voltage = (k * temperature_k) / q;

        let port_resistance = thermal_voltage / saturation_current;

        Self {
            saturation_current,
            thermal_voltage,
            ideality_factor,
            port_resistance,
            voltage: 0.0,
            current: 0.0,
            last_b: 0.0,
        }
    }

    /// Get saturation current
    pub fn saturation_current(&self) -> f64 {
        self.saturation_current
    }

    /// Get thermal voltage
    pub fn thermal_voltage(&self) -> f64 {
        self.thermal_voltage
    }

    fn diode_equation(&self, v: f64) -> f64 {
        let vt = self.thermal_voltage * self.ideality_factor;
        self.saturation_current * ((v / vt).exp() - 1.0)
    }

    fn diode_derivative(&self, v: f64) -> f64 {
        let vt = self.thermal_voltage * self.ideality_factor;
        self.saturation_current * (v / vt).exp() / vt
    }

    fn solve_newton(&self, a: f64, r: f64) -> f64 {
        let mut v = 0.0;
        let tolerance = 1e-9;

        for _ in 0..10 {
            let i = self.diode_equation(v);
            let g = self.diode_derivative(v);

            let f = v + r * i - a;

            if f.abs() < tolerance {
                break;
            }

            let df = 1.0 + r * g;
            v -= f / df;
        }

        v
    }
}

impl WdfElement for Diode {
    fn port_resistance(&self) -> f64 {
        self.port_resistance
    }

    fn process_incident(&mut self, a: f64) -> f64 {
        let v = self.solve_newton(a, self.port_resistance);
        let i = self.diode_equation(v);

        self.voltage = v;
        self.current = i;

        2.0 * v - a
    }

    fn update_state(&mut self) {
        let g = self.diode_derivative(self.voltage);
        if g > 0.0 {
            self.port_resistance = 1.0 / g;
        }
    }

    fn voltage(&self) -> f64 {
        self.voltage
    }

    fn current(&self) -> f64 {
        self.current
    }

    fn reset(&mut self) {
        self.voltage = 0.0;
        self.current = 0.0;
        self.last_b = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resistor_wdf() {
        let mut resistor = Resistor::new(1000.0);
        assert_eq!(resistor.port_resistance(), 1000.0);

        let b = resistor.process_incident(1.0);
        assert!((b - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_capacitor_wdf() {
        let sample_rate = 44100.0;
        let capacitance = 1e-6;
        let capacitor = Capacitor::new(capacitance, sample_rate);

        let expected_r = 1.0 / (sample_rate * 2.0 * capacitance);
        assert!((capacitor.port_resistance() - expected_r).abs() < 1e-10);
    }

    #[test]
    fn test_inductor_wdf() {
        let sample_rate = 44100.0;
        let inductance = 100e-6;
        let inductor = Inductor::new(inductance, sample_rate);

        let t = 1.0 / sample_rate;
        let expected_r = 2.0 * inductance / t;
        assert!((inductor.port_resistance() - expected_r).abs() < 1e-10);
    }

    #[test]
    fn test_diode_thermal_voltage() {
        let diode = Diode::new(1e-9, 1.0, 300.0);
        let expected_vt = 1.380649e-23 * 300.0 / 1.60217662e-19;
        assert!((diode.thermal_voltage() - expected_vt).abs() < 1e-27);
    }
}
