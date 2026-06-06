use rill_core::{
    math::Transcendental,
    traits::algorithm::{Algorithm, AlgorithmMetadata},
    ProcessError, ProcessResult,
};

use crate::{Capacitor, Inductor, WdfElement};

/// Record head model for analog tape.
///
/// Applies bias oscillator mixing and magnetic tape nonlinearity
/// (saturation + hysteresis). Output is the magnetized signal
/// ready to be stored on tape. No op-amp stages — add those externally.
#[derive(Debug, Clone)]
pub struct RecordHead<T: Transcendental> {
    sample_rate: T,
    bias_oscillator: T,
    bias_phase: T,
    record_head: Inductor<T>,

    /// Tape speed in cm/s.
    pub tape_speed: T,
    /// Bias level (0.0–1.0).
    pub bias_level: T,
    /// Magnetic saturation strength (0.0–1.0).
    pub saturation: T,
    /// Hysteresis effect strength.
    pub hysteresis: T,

    tape_position: T,
}

impl<T: Transcendental> RecordHead<T> {
    /// Create a new record head model.
    pub fn new(sample_rate: f32) -> Self {
        let sr = T::from_f32(sample_rate);
        Self {
            sample_rate: sr,
            bias_oscillator: T::from_f32(100_000.0),
            bias_phase: T::ZERO,
            record_head: Inductor::<T>::new(T::from_f64(100e-6), sr),
            tape_speed: T::from_f64(4.76),
            bias_level: T::from_f64(0.8),
            saturation: T::from_f64(0.9),
            hysteresis: T::from_f64(0.1),
            tape_position: T::ZERO,
        }
    }

    /// Process one sample through the record chain.
    pub fn process_sample(&mut self, input: T) -> T {
        let dt = T::ONE / self.sample_rate;

        let two = T::from_f32(2.0);
        let bias_phase_inc = two * T::PI * self.bias_oscillator * dt;
        self.bias_phase += bias_phase_inc;
        let bias_signal = self.bias_level * self.bias_phase.sin();

        let record_signal = input + bias_signal;
        let recorded = self.tape_nonlinearity(record_signal);

        let _head_current = recorded / self.record_head.port_resistance();
        self.tape_position += self.tape_speed * dt;

        recorded
    }

    fn tape_nonlinearity(&self, signal: T) -> T {
        let saturated = signal.tanh() * self.saturation;
        let hysteresis_effect = self.hysteresis * signal.signum() * T::from_f64(0.01);
        saturated + hysteresis_effect
    }
}

impl<T: Transcendental> Algorithm<T> for RecordHead<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let src = input.ok_or_else(|| ProcessError::processing("RecordHead requires input"))?;
        let n = src.len().min(output.len());
        for i in 0..n {
            output[i] = self.process_sample(src[i]);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.bias_phase = T::ZERO;
        self.tape_position = T::ZERO;
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Tape Record Head",
            description: "Analog tape recording physics (bias + saturation)",
            ..AlgorithmMetadata::empty()
        }
    }
}

/// Playback head model for analog tape.
///
/// Applies wow & flutter, head differentiator, gap loss, playback EQ,
/// print-through, and tape noise. Output is the amplified audio signal.
/// No op-amp stages — add those externally.
#[derive(Debug, Clone)]
pub struct PlaybackHead<T: Transcendental> {
    sample_rate: T,
    playback_head: Inductor<T>,
    eq_filters: [Capacitor<T>; 2],

    /// Tape speed in cm/s.
    pub tape_speed: T,
    /// Tape width in mm (affects noise floor).
    pub tape_width: T,
    /// Tape noise floor amplitude.
    pub noise_floor: T,
    /// Wow & flutter intensity factor.
    pub wow_flutter: T,
    /// Print-through crosstalk factor.
    pub print_through: T,

    wow_phase: T,
    flutter_phase: T,
    playback_head_flux: T,
    playback_head_state: T,
}

impl<T: Transcendental> PlaybackHead<T> {
    /// Create a new playback head model.
    pub fn new(sample_rate: f32) -> Self {
        let sr = T::from_f32(sample_rate);
        Self {
            sample_rate: sr,
            playback_head: Inductor::<T>::new(T::from_f64(50e-6), sr),
            eq_filters: [
                Capacitor::<T>::new(T::from_f64(100e-9), sr),
                Capacitor::<T>::new(T::from_f64(1e-6), sr),
            ],
            tape_speed: T::from_f64(4.76),
            tape_width: T::from_f64(3.81),
            noise_floor: T::from_f64(0.0001),
            wow_flutter: T::from_f64(0.002),
            print_through: T::from_f64(0.01),
            wow_phase: T::ZERO,
            flutter_phase: T::ZERO,
            playback_head_flux: T::ZERO,
            playback_head_state: T::ZERO,
        }
    }

    /// Process one sample through the playback chain.
    /// `recorded_signal` is the magnetized signal read from tape.
    pub fn process_sample(&mut self, recorded_signal: T) -> T {
        let dt = T::ONE / self.sample_rate;

        let speed_variation = T::ONE + self.wow_and_flutter(dt);
        let tape_signal = recorded_signal * speed_variation;

        // Head differentiator: V ∝ dΦ/dt
        let flux_change = tape_signal - self.playback_head_flux;
        self.playback_head_flux = tape_signal;
        let head_signal = flux_change / dt;

        // Gap loss: first-order low-pass at ~18 kHz
        let gap_freq = T::from_f32(18000.0) * (self.tape_speed / T::from_f64(4.76));
        let two_pi = T::from_f32(2.0) * T::PI;
        let gap_alpha = (two_pi * gap_freq * dt) / (T::ONE + two_pi * gap_freq * dt);
        self.playback_head_state =
            gap_alpha * head_signal + (T::ONE - gap_alpha) * self.playback_head_state;

        // Head impedance voltage divider
        let head_z = self.playback_head.port_resistance();
        let head_output = self.playback_head_state * (head_z / (head_z + T::from_f32(1000.0)));

        // Playback EQ: two-stage capacitive network
        let mut eq_signal = head_output;
        for filter in &mut self.eq_filters {
            let alpha = T::ONE / (T::ONE + filter.port_resistance() * T::from_f32(1000.0));
            eq_signal *= alpha;
        }

        let print_through_signal = self.print_through * tape_signal;
        let noise = self.tape_noise();

        eq_signal + print_through_signal + noise
    }

    fn wow_and_flutter(&mut self, dt: T) -> T {
        let two_pi = T::from_f32(2.0) * T::PI;

        let wow_freq = T::from_f32(2.0);
        self.wow_phase += two_pi * wow_freq * dt;
        let wow = T::from_f64(0.01) * self.wow_flutter * self.wow_phase.sin();

        let flutter_freq = T::from_f32(30.0);
        self.flutter_phase += two_pi * flutter_freq * dt;
        let flutter = T::from_f64(0.005) * self.wow_flutter * self.flutter_phase.sin();

        wow + flutter
    }

    fn tape_noise(&self) -> T {
        let width_factor = (T::from_f64(3.81) / self.tape_width).sqrt();
        let white_noise = T::random();
        let pink_noise = white_noise * self.noise_floor * width_factor;

        let click = if T::random().abs() < T::from_f64(0.0001) {
            T::random() * T::from_f64(0.1)
        } else {
            T::ZERO
        };

        pink_noise + click
    }
}

impl<T: Transcendental> Algorithm<T> for PlaybackHead<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let src = input.ok_or_else(|| ProcessError::processing("PlaybackHead requires input"))?;
        let n = src.len().min(output.len());
        for i in 0..n {
            output[i] = self.process_sample(src[i]);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.wow_phase = T::ZERO;
        self.flutter_phase = T::ZERO;
        self.playback_head_flux = T::ZERO;
        self.playback_head_state = T::ZERO;
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Tape Playback Head",
            description: "Analog tape playback physics (wow/flutter, EQ, noise)",
            ..AlgorithmMetadata::empty()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_head_produces_output() {
        let mut head = RecordHead::<f64>::new(44100.0);
        let s = head.process_sample(0.5);
        assert!(s.abs() > 0.0, "record head should produce output");
    }

    #[test]
    fn test_playback_head_passes_signal() {
        let mut head = PlaybackHead::<f64>::new(44100.0);
        let s = head.process_sample(0.5);
        assert!(s.abs() > 0.0, "playback head should produce output");
    }

    #[test]
    fn test_tanh_saturation_is_bounded() {
        let mut head = RecordHead::<f64>::new(44100.0);
        let s = head.process_sample(10.0);
        assert!(s.abs() <= 1.0, "saturation should bound signal");
    }

    #[test]
    fn test_reset_clears_state() {
        let mut head = PlaybackHead::<f64>::new(44100.0);
        head.process_sample(0.5);
        head.reset();
        assert_eq!(head.wow_phase, 0.0);
        assert_eq!(head.flutter_phase, 0.0);
        assert_eq!(head.playback_head_flux, 0.0);
    }
}
