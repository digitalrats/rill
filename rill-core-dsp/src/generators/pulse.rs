//! Pulse wave generator with PWM (Pulse Width Modulation)

use super::Generator;
use rill_core::traits::algorithm::{ActionContext, Algorithm, AlgorithmCategory, AlgorithmMetadata};
use crate::vector::{ScalarVector1, Vector};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;

/// Pulse wave generator with PWM
pub struct PulseOscillator<T: Transcendental> {
    /// Base frequency
    frequency: f32,
    /// Amplitude
    amplitude: ScalarVector1<T>,
    /// Pulse width (0..1)
    pulse_width: ScalarVector1<T>,
    /// Pulse width modulation (PWM)
    pwm_amount: ScalarVector1<T>,
    /// Current phase
    phase: ScalarVector1<T>,
    /// Phase increment
    phase_inc: ScalarVector1<T>,
    /// Sample rate
    sample_rate: f32,
}

impl<T: Transcendental> PulseOscillator<T> {
    /// Create a new pulse generator
    pub fn new(frequency: f32, pulse_width: T) -> Self {
        let mut osc = Self {
            frequency,
            amplitude: ScalarVector1::splat(T::from_f32(1.0)),
            pulse_width: ScalarVector1::splat(pulse_width.clamp(T::ZERO, T::from_f32(1.0))),
            pwm_amount: ScalarVector1::splat(T::ZERO),
            phase: ScalarVector1::splat(T::ZERO),
            phase_inc: ScalarVector1::splat(T::ZERO),
            sample_rate: 44100.0,
        };
        osc.update_phase_inc();
        osc
    }

    fn update_phase_inc(&mut self) {
        self.phase_inc = ScalarVector1::splat(T::from_f32(self.frequency / self.sample_rate));
    }

    /// Set pulse width
    pub fn set_pulse_width(&mut self, width: T) {
        self.pulse_width = ScalarVector1::splat(width.clamp(T::from_f32(0.01), T::from_f32(0.99)));
    }

    /// Set PWM depth
    pub fn set_pwm_amount(&mut self, amount: T) {
        self.pwm_amount = amount.clamp(T::ZERO, T::from_f32(1.0));
    }

    /// Apply external modulation to pulse width
    pub fn modulate_pulse_width(&mut self, modulation: T) -> T {
        let modulated = self.pulse_width.extract(0) + modulation * self.pwm_amount.extract(0);
        modulated.clamp(T::from_f32(0.01), T::from_f32(0.99))
    }

    /// Anti-aliased pulse wave
    fn generate_pulse(&mut self, width: T) -> T {
        let phase = self.phase.extract(0);
        let amplitude = self.amplitude.extract(0);
        let inc = self.phase_inc.extract(0);

        let raw = if phase.to_f32() < width.to_f32() {
            amplitude
        } else {
            amplitude.neg()
        };

        // BLEP correction for both edges
        let next_phase = phase + inc;

        let mut blep = T::ZERO;

        // Rising edge
        if phase < width && next_phase >= width {
            let t = (width - phase) / inc;
            blep = blep + T::from_f32(2.0) * t - T::from_f32(1.0);
        }

        // Falling edge (on phase overflow)
        if next_phase.to_f32() >= 1.0 {
            let t = (T::from_f32(1.0) - phase) / inc;
            blep = blep - (T::from_f32(2.0) * t - T::from_f32(1.0));
        }

        raw + blep * amplitude
    }
}

impl<T: Transcendental> Algorithm<T> for PulseOscillator<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_phase_inc();
        self.phase = ScalarVector1::splat(T::ZERO);
    }

    fn reset(&mut self) {
        self.phase = ScalarVector1::splat(T::ZERO);
    }

    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());

        for i in 0..len {
            let modulation = input[i];
            let width = self.modulate_pulse_width(modulation);
            let sample = self.generate_pulse(width);

            output[i] = sample;

            // Update phase
            self.phase = self.phase + self.phase_inc;
            if self.phase.extract(0).to_f32() >= 1.0 {
                self.phase = self.phase - ScalarVector1::splat(T::from_f32(1.0));
            }
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Pulse Oscillator",
            category: AlgorithmCategory::Generator,
            description: "Pulse wave oscillator with PWM".to_string(),
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental> Generator<T> for PulseOscillator<T> {
    fn phase(&self) -> T {
        self.phase.extract(0)
    }
    fn set_phase(&mut self, phase: T) {
        self.phase = ScalarVector1::splat(phase);
    }
    fn frequency(&self) -> f32 {
        self.frequency
    }
    fn set_frequency(&mut self, freq: f32) {
        self.frequency = freq;
        self.update_phase_inc();
    }
    fn amplitude(&self) -> T {
        self.amplitude.extract(0)
    }
    fn set_amplitude(&mut self, amp: T) {
        self.amplitude = ScalarVector1::splat(amp);
    }
}
