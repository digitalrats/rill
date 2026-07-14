//! Simple filters for sound coloration

use std::f32::consts::PI;

use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;

/// Simple low-pass filter (one-pole)
pub struct LowPass {
    /// Cutoff frequency in Hz.
    pub cutoff: f32,
    /// Sample rate in Hz.
    pub sample_rate: f32,
    state: f32,
}

impl LowPass {
    /// Create a new `LowPass` filter with the given cutoff frequency and sample rate.
    pub fn new(cutoff: f32, sample_rate: f32) -> Self {
        Self {
            cutoff,
            sample_rate,
            state: 0.0,
        }
    }

    /// Process a single sample through the low-pass filter, returning the filtered output.
    pub fn process(&mut self, input: f32) -> f32 {
        let rc = 1.0 / (2.0 * PI * self.cutoff);
        let dt = 1.0 / self.sample_rate;
        let alpha = dt / (rc + dt);

        self.state = self.state + alpha * (input - self.state);
        self.state
    }
}

/// DC blocking filter — digital equivalent of an AC coupling capacitor.
///
/// A passive, one-pole highpass that dynamically removes the DC component
/// from a signal. This is the digital model of the single capacitor found
/// at the output of virtually every audio circuit built before the 1990s.
///
/// Unlike a static `x - offset` subtraction, this adapts to the changing DC
/// level — silence maps to `0`, and the active signal stays centred regardless
/// of history.
///
/// # Transfer function
///
/// `H(z) = (1 - z⁻¹) / (1 - R·z⁻¹)` where `R = 1 - 2π·f_c / f_s`.
///
/// At `f_s = 44100` and `f_c = 10`, `R ≈ 0.99857` — a gentle highpass that
/// preserves all audible frequencies while removing DC.
///
/// # Algorithm
///
/// Implements [`Algorithm<T>`] so it can be used as a standalone graph node.
pub struct DcBlocker<T: Transcendental> {
    x_prev: T,
    y_prev: T,
    r: T,
    sample_rate: f32,
    cutoff_hz: f32,
}

impl<T: Transcendental> DcBlocker<T> {
    /// Create a new DC blocker with the given cutoff frequency and sample rate.
    ///
    /// Typical values: `cutoff_hz = 10.0`, `sample_rate = 44100.0`.
    pub fn new(sample_rate: f32, cutoff_hz: f32) -> Self {
        let r = 1.0 - 2.0 * PI * cutoff_hz / sample_rate;
        Self {
            x_prev: T::ZERO,
            y_prev: T::ZERO,
            r: T::from_f32(r),
            sample_rate,
            cutoff_hz,
        }
    }

    /// Process a single sample, returning the DC-free output.
    pub fn process_sample(&mut self, x: T) -> T {
        let y = x - self.x_prev + self.r * self.y_prev;
        self.x_prev = x;
        self.y_prev = y;
        y
    }

    /// Reset internal state.
    pub fn reset(&mut self) {
        self.x_prev = T::ZERO;
        self.y_prev = T::ZERO;
    }
}

impl<T: Transcendental> Algorithm<T> for DcBlocker<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.r = T::from_f32(1.0 - 2.0 * PI * self.cutoff_hz / sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        self.reset();
    }

    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());

        for i in 0..len {
            output[i] = self.process_sample(input[i]);
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "DC Blocker",
            category: AlgorithmCategory::Filter,
            description:
                "Passive one-pole DC-blocking filter — digital model of an AC coupling capacitor",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

/// Filter for telephone voice emulation (300Hz - 3.4kHz)
pub fn telephone_filter(input: f32, sample_rate: f32) -> f32 {
    static mut LP_STATE: f32 = 0.0;
    static mut HP_STATE: f32 = 0.0;

    unsafe {
        // Low-pass filter 3.4kHz
        let lp_cutoff = 3400.0 / sample_rate;
        LP_STATE = LP_STATE + lp_cutoff * (input - LP_STATE);

        // High-pass filter 300Hz (via subtracting LPF)
        let hp_cutoff = 300.0 / sample_rate;
        HP_STATE = HP_STATE + hp_cutoff * (LP_STATE - HP_STATE);

        LP_STATE - HP_STATE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dc_blocker_removes_constant_offset() {
        let mut dc = DcBlocker::<f32>::new(44100.0, 10.0);
        for _ in 0..4096 {
            dc.process_sample(0.5);
        }
        let y = dc.process_sample(0.5);
        assert!(y.abs() < 0.01, "DC should be removed, got {}", y);
    }

    #[test]
    fn test_dc_blocker_preserves_ac() {
        let mut dc = DcBlocker::<f32>::new(44100.0, 10.0);
        let half = 5;
        let warmup_samples = 2000;
        let measure_samples = 2000;
        let total = warmup_samples + measure_samples;
        let mut sum = 0.0f64;
        let mut peak = 0.0f32;
        for i in 0..total {
            let x = if (i / half) % 2 == 0 { 1.0 } else { 0.0 };
            let y = dc.process_sample(x);
            if i >= warmup_samples {
                sum += y as f64;
                peak = peak.max(y.abs());
            }
        }
        let mean = sum / measure_samples as f64;
        assert!(mean.abs() < 0.01, "DC should be removed, mean = {}", mean);
        assert!(peak > 0.3, "signal should be preserved, peak = {}", peak);
    }

    #[test]
    fn test_dc_blocker_reset() {
        let mut dc = DcBlocker::<f32>::new(44100.0, 10.0);
        for _ in 0..100 {
            dc.process_sample(0.8_f32);
        }
        dc.reset();
        assert_eq!(dc.x_prev, 0.0);
        assert_eq!(dc.y_prev, 0.0);
    }

    #[test]
    fn test_dc_blocker_algorithm_process() {
        let mut dc = DcBlocker::<f32>::new(44100.0, 10.0);
        let mut output = [0.0f32; 4];
        Algorithm::process(&mut dc, Some(&[0.5, 0.5, 0.5, 0.5]), &mut output).unwrap();
        // First sample: y = 0.5 - 0.0 + r * 0.0 = 0.5
        // Subsequent: x_prev = x = 0.5, so y = 0.5 - 0.5 + r * y_prev = r * y_prev → decay
        // Some output should be non-zero (not silent)
        assert!(
            output.iter().any(|&v| v.abs() > 0.0),
            "Algorithm::process should produce output"
        );
    }
}
