//! Базовые осцилляторы (Sine, Saw, Square, Triangle)

use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use crate::generators::{Generator, ModulatableGenerator, SyncableGenerator};
use crate::vector::prelude::*;
use rill_core::traits::{ActionContext, ProcessResult};
use rill_core::Transcendental;
use std::f32::consts::PI;

/// Тип волны
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Waveform {
    /// Чистая синусоида
    Sine,
    /// Пилообразная волна
    Saw,
    /// Квадратная волна
    Square,
    /// Треугольная волна
    Triangle,
    /// Прямоугольная волна с регулируемой скважностью
    Pulse(f32), // с шириной импульса (0.0 - 1.0)
}

impl Waveform {
    /// Получить название формы волны
    pub fn name(&self) -> &'static str {
        match self {
            Waveform::Sine => "Sine",
            Waveform::Saw => "Saw",
            Waveform::Square => "Square",
            Waveform::Triangle => "Triangle",
            Waveform::Pulse(_) => "Pulse",
        }
    }

    /// Получить описание формы волны
    pub fn description(&self) -> &'static str {
        match self {
            Waveform::Sine => "Pure sine wave - single harmonic",
            Waveform::Saw => "Sawtooth wave - all harmonics (1/n)",
            Waveform::Square => "Square wave - odd harmonics (1/n)",
            Waveform::Triangle => "Triangle wave - odd harmonics (1/n²)",
            Waveform::Pulse(_) => "Pulse wave with variable width",
        }
    }
}

/// Базовый осциллятор
///
/// Генерирует различные формы волн с возможностью:
/// - Изменения частоты в реальном времени
/// - Модуляции частоты (FM)
/// - Анти-алиасинга для пилообразной волны
/// - Синхронизации фазы
#[derive(Clone, Copy)]
pub struct BasicOscillator<T: Transcendental> {
    /// Тип волны
    waveform: Waveform,
    /// Частота (Hz)
    frequency: f32,
    /// Амплитуда (0.0 - 1.0)
    amplitude: ScalarVector1<T>,
    /// Текущая фаза (0..1)
    phase: ScalarVector1<T>,
    /// Инкремент фазы за семпл
    phase_inc: ScalarVector1<T>,
    /// Частота дискретизации
    sample_rate: f32,
    /// Количество завершённых периодов
    periods: u32,
    /// Модуляция частоты (FM)
    fm_amount: ScalarVector1<T>,
}

impl<T: Transcendental> BasicOscillator<T> {
    /// Создать новый осциллятор
    ///
    /// # Arguments
    /// * `waveform` - форма волны
    /// * `frequency` - частота в Hz
    /// * `amplitude` - амплитуда (0.0 - 1.0)
    pub fn new(waveform: Waveform, frequency: f32, amplitude: T) -> Self {
        let mut osc = Self {
            waveform,
            frequency,
            amplitude: ScalarVector1::splat(amplitude),
            phase: ScalarVector1::splat(T::ZERO),
            phase_inc: ScalarVector1::splat(T::ZERO),
            sample_rate: 44100.0,
            periods: 0,
            fm_amount: ScalarVector1::splat(T::ZERO),
        };
        osc.update_phase_inc();
        osc
    }

    /// Обновить инкремент фазы на основе текущей частоты
    #[inline(always)]
    fn update_phase_inc(&mut self) {
        self.phase_inc = ScalarVector1::splat(T::from_f32(self.frequency / self.sample_rate));
    }

    /// Генерировать синусоиду
    #[inline(always)]
    fn generate_sine(&self) -> ScalarVector1<T> {
        let phase_rad = self.phase.mul(&ScalarVector1::splat(T::from_f32(2.0 * PI)));
        phase_rad.sin().mul(&self.amplitude)
    }

    /// Генерировать пилообразную волну (без анти-алиасинга)
    #[inline(always)]
    fn generate_saw_raw(&self) -> ScalarVector1<T> {
        // 2 * phase - 1
        self.phase
            .mul(&ScalarVector1::splat(T::from_f32(2.0)))
            .sub(&ScalarVector1::splat(T::from_f32(1.0)))
            .mul(&self.amplitude)
    }

    /// Генерировать пилообразную волну с анти-алиасингом
    #[inline(always)]
    fn generate_saw_bandlimited(&mut self) -> ScalarVector1<T> {
        let raw = self.generate_saw_raw();
        // Проверка на переход через 0 (discontinuity)
        let next_phase = self.phase.add(&self.phase_inc).extract(0);
        let one = T::from_f32(1.0);

        if next_phase >= one {
            // Вычисляем позицию discontinuity
            let one_vec = ScalarVector1::splat(one);
            let t = (one_vec - self.phase) / self.phase_inc;
            // Простая Blep коррекция
            let blep =
                t * ScalarVector1::splat(T::from_f32(2.0)) - ScalarVector1::splat(T::from_f32(1.0));
            raw - blep * self.amplitude
        } else {
            raw
        }
    }

    /// Генерировать квадратную волну
    #[inline(always)]
    fn generate_square(&self) -> ScalarVector1<T> {
        let half = T::from_f32(0.5);
        if self.phase.extract(0) < half {
            self.amplitude
        } else {
            -self.amplitude
        }
    }

    /// Генерировать треугольную волну
    #[inline(always)]
    fn generate_triangle(&self) -> ScalarVector1<T> {
        // 4 * |phase - 0.5| - 1
        let half = ScalarVector1::splat(T::from_f32(0.5));
        let p = self.phase - half;
        (p.abs() * ScalarVector1::splat(T::from_f32(4.0)) - ScalarVector1::splat(T::from_f32(1.0)))
            * self.amplitude
    }

    /// Генерировать прямоугольную волну с переменной скважностью
    #[inline(always)]
    fn generate_pulse(&self, width: f32) -> ScalarVector1<T> {
        let width_t = T::from_f32(width.clamp(0.01, 0.99));
        if self.phase.extract(0) < width_t {
            self.amplitude
        } else {
            -self.amplitude
        }
    }

    /// Основной метод генерации семпла
    pub(crate) fn generate(&mut self) -> ScalarVector1<T> {
        // Применяем FM модуляцию если есть
        let effective_inc = self.phase_inc + self.fm_amount;

        // Генерируем семпл в зависимости от формы волны
        let output_vec = match self.waveform {
            Waveform::Sine => self.generate_sine(),
            Waveform::Saw => self.generate_saw_bandlimited(),
            Waveform::Square => self.generate_square(),
            Waveform::Triangle => self.generate_triangle(),
            Waveform::Pulse(width) => self.generate_pulse(width),
        };

        // Обновляем фазу
        self.phase = self.phase + effective_inc;
        let one = ScalarVector1::splat(T::from_f32(1.0));
        if self.phase.extract(0) >= one.extract(0) {
            self.phase = self.phase - one;
            self.periods += 1;
        }

        output_vec
    }

    /// Сбросить фазу в 0
    pub fn reset_phase(&mut self) {
        self.phase = ScalarVector1::splat(T::ZERO);
        self.periods = 0;
    }

    /// Получить текущую фазу (0..1)
    pub fn current_phase(&self) -> T {
        self.phase.extract(0)
    }

    /// Получить количество завершённых периодов
    pub fn period_count(&self) -> u32 {
        self.periods
    }

    /// Установить ширину импульса (для Pulse волны)
    pub fn set_pulse_width(&mut self, width: f32) {
        if let Waveform::Pulse(_) = self.waveform {
            self.waveform = Waveform::Pulse(width.clamp(0.01, 0.99));
        }
    }
}

// ==================== Реализация трейта Algorithm ====================

impl<T: Transcendental> Algorithm<T> for BasicOscillator<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_phase_inc();
        self.phase = ScalarVector1::splat(T::ZERO);
        self.periods = 0;
    }

    fn reset(&mut self) {
        self.phase = ScalarVector1::splat(T::ZERO);
        self.periods = 0;
        self.fm_amount = ScalarVector1::splat(T::ZERO);
    }

    fn process(
        &mut self,
        _input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        for out in output.iter_mut() {
            *out = self.generate().extract(0);
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: self.waveform.name(),
            category: AlgorithmCategory::Generator,
            description: self.waveform.description(),
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

// ==================== Реализация трейта Generator ====================

impl<T: Transcendental> Generator<T> for BasicOscillator<T> {
    fn phase(&self) -> T {
        self.phase.extract(0)
    }

    fn set_phase(&mut self, phase: T) {
        let one = T::from_f32(1.0);
        let zero = T::ZERO;
        self.phase = ScalarVector1::splat(if phase > one {
            one
        } else if phase < zero {
            zero
        } else {
            phase
        });
    }

    fn frequency(&self) -> f32 {
        self.frequency
    }

    fn set_frequency(&mut self, freq: f32) {
        self.frequency = freq.clamp(0.1, 20000.0);
        self.update_phase_inc();
    }

    fn amplitude(&self) -> T {
        self.amplitude.extract(0)
    }

    fn set_amplitude(&mut self, amp: T) {
        let one = T::from_f32(1.0);
        let zero = T::ZERO;
        self.amplitude = ScalarVector1::splat(if amp > one {
            one
        } else if amp < zero {
            zero
        } else {
            amp
        });
    }
}

// ==================== Реализация трейта SyncableGenerator ====================

impl<T: Transcendental> SyncableGenerator<T> for BasicOscillator<T> {
    fn sync(&mut self, reset: bool) {
        if reset {
            self.phase = ScalarVector1::splat(T::ZERO);
        }
    }

    fn periods(&self) -> u32 {
        self.periods
    }
}

// ==================== Реализация трейта ModulatableGenerator ====================

impl<T: Transcendental> ModulatableGenerator<T> for BasicOscillator<T> {
    fn modulate_frequency(&mut self, amount: T) {
        self.fm_amount = ScalarVector1::splat(amount);
    }

    fn modulation_index(&self) -> T {
        self.fm_amount.extract(0)
    }

    fn set_modulation_index(&mut self, index: T) {
        self.fm_amount = ScalarVector1::splat(index);
    }
}

// ==================== Тесты ====================

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;

    #[test]
    fn test_sine_oscillator() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 0.5);
        osc.init(44100.0);

        // Первый семпл должен быть 0
        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample1 = output[0];
        assert!(approx_eq!(f32, sample1, 0.0, epsilon = 1e-6));

        // Второй семпл должен быть не 0
        osc.process(None, &mut output, &ctx).unwrap();
        let sample2 = output[0];
        assert!(sample2 != 0.0);
        assert!(sample2 >= -0.5 && sample2 <= 0.5);
    }

    #[test]
    fn test_saw_oscillator() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Saw, 440.0, 0.5);
        osc.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample >= -0.5 && sample <= 0.5);
    }

    #[test]
    fn test_square_oscillator() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Square, 440.0, 0.5);
        osc.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample == 0.5 || sample == -0.5);
    }

    #[test]
    fn test_triangle_oscillator() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Triangle, 440.0, 0.5);
        osc.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample >= -0.5 && sample <= 0.5);
    }

    #[test]
    fn test_pulse_oscillator() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Pulse(0.25), 440.0, 0.5);
        osc.init(44100.0);

        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample == 0.5); // При фазе 0 должен быть положительный импульс
    }

    #[test]
    fn test_frequency_change() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 0.5);
        osc.init(44100.0);

        assert_eq!(osc.frequency(), 440.0);

        osc.set_frequency(880.0);
        assert_eq!(osc.frequency(), 880.0);
    }

    #[test]
    fn test_amplitude_change() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 0.5);
        osc.init(44100.0);

        assert_eq!(osc.amplitude(), 0.5);

        osc.set_amplitude(0.8);
        assert_eq!(osc.amplitude(), 0.8);
    }

    #[test]
    fn test_phase_manipulation() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 1.0);
        osc.init(44100.0);

        osc.set_phase(0.25); // π/2
        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(approx_eq!(f32, sample, 1.0, epsilon = 1e-4)); // sin(π/2) = 1
    }

    #[test]
    fn test_fm_modulation() {
        let mut osc = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 1.0);
        osc.init(44100.0);

        osc.modulate_frequency(0.5);
        assert_eq!(osc.modulation_index(), 0.5);

        // Проверяем, что модуляция применяется
        let mut output = [0.0f32; 1];
        let tick = rill_core::time::ClockTick::new(0, 1, 44100.0);
        let ctx = rill_core::traits::ActionContext::new(&tick);
        osc.process(None, &mut output, &ctx).unwrap();
        let sample = output[0];
        assert!(sample >= -1.0 && sample <= 1.0);
    }

    #[test]
    fn test_clone_copy() {
        let osc1 = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 0.5);
        let osc2 = osc1; // Копирование благодаря Copy
        let osc3 = osc1.clone(); // Явное клонирование

        assert_eq!(osc1.frequency(), osc2.frequency());
        assert_eq!(osc1.frequency(), osc3.frequency());
    }
}
