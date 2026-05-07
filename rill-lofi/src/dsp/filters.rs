//! Simple filters for sound coloration

use std::f32::consts::PI;

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
