//! Вейвтейбл генераторы

use super::Generator;
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use crate::math::{lerp, AudioNum};
use crate::vector::{ScalarVector1, Vector};
use rill_core::traits::{ActionContext, ProcessResult};

/// Вейвтейбл осциллятор
pub struct WavetableOscillator<T: AudioNum, const SIZE: usize> {
    /// Вейвтейбл (таблица волны)
    table: [T; SIZE],
    /// Частота
    frequency: f32,
    /// Амплитуда
    amplitude: ScalarVector1<T>,
    /// Текущая фаза (в индексах таблицы)
    phase: ScalarVector1<T>,
    /// Инкремент фазы
    phase_inc: ScalarVector1<T>,
    /// Интерполяция (true = кубическая, false = линейная)
    cubic_interp: bool,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum, const SIZE: usize> WavetableOscillator<T, SIZE> {
    /// Создать новый вейвтейбл осциллятор
    pub fn new(table: [T; SIZE], frequency: f32) -> Self {
        let mut osc = Self {
            table,
            frequency,
            amplitude: ScalarVector1::splat(T::from_f32(1.0)),
            phase: ScalarVector1::splat(T::ZERO),
            phase_inc: ScalarVector1::splat(T::ZERO),
            cubic_interp: false,
            sample_rate: 44100.0,
        };
        osc.update_phase_inc();
        osc
    }

    /// Создать из синусоиды
    pub fn sine(frequency: f32) -> Self {
        let mut table = [T::ZERO; SIZE];
        for i in 0..SIZE {
            let phase = (i as f32 / SIZE as f32) * 2.0 * std::f32::consts::PI;
            table[i] = T::from_f32(phase.sin());
        }
        Self::new(table, frequency)
    }

    /// Создать из пилообразной волны
    pub fn saw(frequency: f32) -> Self {
        let mut table = [T::ZERO; SIZE];
        for i in 0..SIZE {
            table[i] = T::from_f32(2.0 * i as f32 / SIZE as f32 - 1.0);
        }
        Self::new(table, frequency)
    }

    fn update_phase_inc(&mut self) {
        self.phase_inc = ScalarVector1::splat(
            T::from_f32(self.frequency / self.sample_rate).mul(T::from_f32(SIZE as f32)),
        );
    }

    /// Линейная интерполяция
    #[inline(always)]
    fn read_linear(&self, idx: T) -> T {
        let idx_f = idx.to_f32();
        let i0 = idx_f.floor() as usize % SIZE;
        let i1 = (i0 + 1) % SIZE;
        let frac = T::from_f32(idx_f.fract());

        lerp(self.table[i0], self.table[i1], frac)
    }

    /// Кубическая интерполяция (Hermite)
    #[inline(always)]
    fn read_cubic(&self, idx: T) -> T {
        let idx_f = idx.to_f32();
        let i = idx_f.floor() as usize;

        let i0 = (i + SIZE - 1) % SIZE;
        let i1 = i % SIZE;
        let i2 = (i + 1) % SIZE;
        let i3 = (i + 2) % SIZE;
        let frac = T::from_f32(idx_f.fract());

        // Hermite interpolation
        let c0 = self.table[i1];
        let c1 = self.table[i2].sub(self.table[i0]).mul(T::from_f32(0.5));
        let c2 = self.table[i0]
            .sub(self.table[i1])
            .mul(T::from_f32(1.5))
            .add(self.table[i2].sub(self.table[i3]).mul(T::from_f32(0.5)));
        let c3 = self.table[i2]
            .sub(self.table[i1])
            .mul(T::from_f32(0.5))
            .add(self.table[i3].sub(self.table[i0]).mul(T::from_f32(0.5)))
            .sub(self.table[i1].sub(self.table[i2]).mul(T::from_f32(1.5)));

        let f2 = frac.mul(frac);
        let f3 = f2.mul(frac);

        c0.add(c1.mul(frac)).add(c2.mul(f2)).add(c3.mul(f3))
    }
}

impl<T: AudioNum, const SIZE: usize> Algorithm<T> for WavetableOscillator<T, SIZE> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_phase_inc();
        self.phase = ScalarVector1::splat(T::ZERO);
    }

    fn reset(&mut self) {
        self.phase = ScalarVector1::splat(T::ZERO);
    }

    fn process(&mut self, input: Option<&[T]>, output: &mut [T], _ctx: &ActionContext) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());

        for i in 0..len {
            // Игнорируем входной сигнал (генератор)
            let _ = input[i];

            let sample = if self.cubic_interp {
                self.read_cubic(self.phase.extract(0))
            } else {
                self.read_linear(self.phase.extract(0))
            }
            .mul(self.amplitude.extract(0));

            output[i] = sample;

            // Обновляем фазу
            self.phase = self.phase + self.phase_inc;
            while self.phase.extract(0).to_f32() >= SIZE as f32 {
                self.phase = self.phase - ScalarVector1::splat(T::from_f32(SIZE as f32));
            }
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Wavetable Oscillator",
            category: AlgorithmCategory::Generator,
            description: "Wavetable oscillator with interpolation".to_string(),
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: AudioNum, const SIZE: usize> Generator<T> for WavetableOscillator<T, SIZE> {
    fn phase(&self) -> T {
        self.phase.extract(0).div(T::from_f32(SIZE as f32))
    }

    fn set_phase(&mut self, phase: T) {
        self.phase = ScalarVector1::splat(
            phase
                .mul(T::from_f32(SIZE as f32))
                .clamp(T::ZERO, T::from_f32(SIZE as f32)),
        );
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
        self.amplitude = ScalarVector1::splat(amp.clamp(T::ZERO, T::from_f32(1.0)));
    }
}
