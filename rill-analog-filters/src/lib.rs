//! Analog filters using WDF (Wave Digital Filter) modeling.

#![deny(unsafe_code)]

/// One-pole RC lowpass section using bilinear transform.
///
/// Implements the difference equation:
///   y[n] = b0*x[n] + b1*x[n-1] + a1*y[n-1]
///
/// where b0 = b1 = k/(1+k), a1 = (1-k)/(1+k), k = π·fc/fs.
#[derive(Debug, Clone)]
struct WdfRcPole {
    b0: f64,
    b1: f64,
    a1: f64,
    x1: f64,
    y1: f64,
}

impl WdfRcPole {
    fn new(cutoff: f64, sample_rate: f64) -> Self {
        let mut pole = Self {
            b0: 0.0,
            b1: 0.0,
            a1: 0.0,
            x1: 0.0,
            y1: 0.0,
        };
        pole.set_cutoff(cutoff, sample_rate);
        pole
    }

    fn set_cutoff(&mut self, cutoff: f64, sample_rate: f64) {
        let k = (std::f64::consts::PI * cutoff / sample_rate).clamp(0.0, 0.95);
        let kp1 = 1.0 + k;
        self.b0 = k / kp1;
        self.b1 = k / kp1;
        self.a1 = (1.0 - k) / kp1;
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.a1 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    fn output_voltage(&self) -> f64 {
        self.y1
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

mod nodes;

pub use nodes::WdfMoogLadderProcessor;

/// WDF-based Moog ladder 4-pole lowpass filter
///
/// Uses four WDF RC one-pole sections in series with one-sample-delayed
/// resonance feedback.
#[derive(Debug, Clone)]
pub struct WdfMoogLadder {
    poles: [WdfRcPole; 4],
    cutoff: f64,
    resonance: f64,
    drive: f64,
    sample_rate: f64,
    feedback_prev: f64,
}

impl WdfMoogLadder {
    pub fn new(sample_rate: f64) -> Self {
        let cutoff = 1000.0;
        Self {
            poles: [
                WdfRcPole::new(cutoff, sample_rate),
                WdfRcPole::new(cutoff, sample_rate),
                WdfRcPole::new(cutoff, sample_rate),
                WdfRcPole::new(cutoff, sample_rate),
            ],
            cutoff,
            resonance: 0.0,
            drive: 1.0,
            sample_rate,
            feedback_prev: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.set_cutoff(self.cutoff);
    }

    pub fn set_cutoff(&mut self, cutoff: f64) {
        self.cutoff = cutoff.max(20.0).min(self.sample_rate / 2.0);
        for pole in &mut self.poles {
            pole.set_cutoff(self.cutoff, self.sample_rate);
        }
    }

    pub fn set_resonance(&mut self, resonance: f64) {
        self.resonance = resonance.clamp(0.0, 1.0);
    }

    pub fn set_drive(&mut self, drive: f64) {
        self.drive = drive.clamp(0.1, 10.0);
    }

    pub fn reset(&mut self) {
        for pole in &mut self.poles {
            pole.reset();
        }
        self.feedback_prev = 0.0;
    }

    pub fn cutoff(&self) -> f64 {
        self.cutoff
    }

    pub fn resonance(&self) -> f64 {
        self.resonance
    }

    pub fn drive(&self) -> f64 {
        self.drive
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    pub fn process(&mut self, input: f64) -> f64 {
        let driven = (input * self.drive).tanh();
        let x = driven - self.feedback_prev * self.resonance * 4.0;
        let mut a = x;
        for pole in &mut self.poles {
            a = pole.process(a);
        }
        let out = self.poles[3].output_voltage();
        self.feedback_prev = out.clamp(-1.0, 1.0);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wdf_rc_pole_dc() {
        let mut pole = WdfRcPole::new(100.0, 44100.0);
        for _ in 0..1000 {
            pole.process(1.0);
        }
        let v = pole.output_voltage();
        assert!((v - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_wdf_moog_ladder_creation() {
        let filter = WdfMoogLadder::new(44100.0);
        assert_eq!(filter.cutoff(), 1000.0);
        assert_eq!(filter.resonance(), 0.0);
        assert_eq!(filter.drive(), 1.0);
    }

    #[test]
    fn test_wdf_moog_ladder_process() {
        let mut filter = WdfMoogLadder::new(44100.0);
        let mut out = 0.0;
        for _ in 0..100 {
            out = filter.process(0.5);
        }
        assert!(out.abs() > 0.0);
    }

    #[test]
    fn test_wdf_moog_ladder_set_params() {
        let mut filter = WdfMoogLadder::new(44100.0);
        filter.set_cutoff(5000.0);
        filter.set_resonance(0.7);
        filter.set_drive(2.0);
        assert!((filter.cutoff() - 5000.0).abs() < 1e-6);
        assert!((filter.resonance() - 0.7).abs() < 1e-6);
        assert!((filter.drive() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_wdf_moog_ladder_reset() {
        let mut filter = WdfMoogLadder::new(44100.0);
        for _ in 0..10 {
            filter.process(1.0);
        }
        filter.reset();
        for pole in &filter.poles {
            assert_eq!(pole.y1, 0.0);
        }
        assert_eq!(filter.feedback_prev, 0.0);
    }

    #[test]
    fn test_wdf_moog_ladder_cutoff_clamp() {
        let mut filter = WdfMoogLadder::new(44100.0);
        filter.set_cutoff(10.0);
        assert!((filter.cutoff() - 20.0).abs() < 1e-6);
        filter.set_cutoff(50000.0);
        assert!((filter.cutoff() - 22050.0).abs() < 1e-6);
    }

    #[test]
    fn test_wdf_moog_ladder_k_clamp() {
        let mut filter = WdfMoogLadder::new(44100.0);
        filter.set_cutoff(20000.0);
        assert!((filter.cutoff() - 20000.0).abs() < 1e-6);
        let mut out = 0.0;
        for _ in 0..100 {
            out = filter.process(0.5);
        }
        assert!(out.abs() > 0.0);
    }
}
