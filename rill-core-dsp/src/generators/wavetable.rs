use crate::generators::{Generator, InterpolatedReader};
use crate::vector::prelude::*;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;

/// Wavetable oscillator built on [`InterpolatedReader`].
///
/// The compile-time constant `SIZE` determines table resolution but the
/// underlying storage is heap-allocated, sharing the same interpolation
/// engine with [`SamplePlayer`](crate::generators::SamplePlayer).
pub struct WavetableOscillator<T: Transcendental, const SIZE: usize> {
    reader: InterpolatedReader<T>,
    frequency: f32,
    amplitude: ScalarVector1<T>,
    sample_rate: f32,
}

impl<T: Transcendental, const SIZE: usize> WavetableOscillator<T, SIZE> {
    /// Create from an explicit table.
    pub fn new(table: [T; SIZE], frequency: f32) -> Self {
        let mut reader = InterpolatedReader::new(table.to_vec());
        reader.set_wrap(true);
        let mut osc = Self {
            reader,
            frequency,
            amplitude: ScalarVector1::splat(T::from_f32(1.0)),
            sample_rate: 44100.0,
        };
        osc.update_rate();
        osc
    }

    /// Create a sine wavetable.
    pub fn sine(frequency: f32) -> Self {
        let mut table = [T::ZERO; SIZE];
        for (i, tbl) in table.iter_mut().enumerate() {
            let phase = (i as f32 / SIZE as f32) * 2.0 * core::f32::consts::PI;
            *tbl = T::from_f32(phase.sin());
        }
        Self::new(table, frequency)
    }

    /// Create a sawtooth wavetable.
    pub fn saw(frequency: f32) -> Self {
        let mut table = [T::ZERO; SIZE];
        for (i, tbl) in table.iter_mut().enumerate() {
            *tbl = T::from_f32(2.0 * i as f32 / SIZE as f32 - 1.0);
        }
        Self::new(table, frequency)
    }

    /// Replace the wavetable data.
    pub fn set_table(&mut self, table: [T; SIZE]) {
        self.reader.set_buffer(table.to_vec());
    }

    /// Enable cubic interpolation (default: linear).
    pub fn set_cubic(&mut self, cubic: bool) {
        self.reader.set_cubic(cubic);
    }

    /// Whether cubic interpolation is enabled.
    pub fn is_cubic(&self) -> bool {
        self.reader.is_cubic()
    }

    fn update_rate(&mut self) {
        let rate = self.frequency as f64 * SIZE as f64 / self.sample_rate as f64;
        self.reader.set_rate(rate);
    }
}

impl<T: Transcendental, const SIZE: usize> Algorithm<T> for WavetableOscillator<T, SIZE> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_rate();
        self.reader.set_position(0.0);
    }

    fn reset(&mut self) {
        self.reader.set_position(0.0);
    }

    fn process(&mut self, _input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let amp = self.amplitude.extract(0);
        self.reader.render_block(output);
        if amp != T::from_f32(1.0) {
            for s in output.iter_mut() {
                *s *= amp;
            }
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Wavetable Oscillator",
            category: AlgorithmCategory::Generator,
            description: "Wavetable oscillator with linear / cubic interpolation",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental, const SIZE: usize> Generator<T> for WavetableOscillator<T, SIZE> {
    fn phase(&self) -> T {
        let pos = self.reader.position();
        let len = SIZE as f64;
        T::from_f64((pos % len) / len)
    }

    fn set_phase(&mut self, phase: T) {
        let p = phase.to_f64().clamp(0.0, 1.0);
        self.reader.set_position(p * SIZE as f64);
    }

    fn reset_phase(&mut self) {
        self.reader.set_position(0.0);
    }

    fn frequency(&self) -> f32 {
        self.frequency
    }

    fn set_frequency(&mut self, freq: f32) {
        self.frequency = freq;
        self.update_rate();
    }

    fn amplitude(&self) -> T {
        self.amplitude.extract(0)
    }

    fn set_amplitude(&mut self, amp: T) {
        self.amplitude = ScalarVector1::splat(amp.clamp(T::ZERO, T::from_f32(1.0)));
    }
}
