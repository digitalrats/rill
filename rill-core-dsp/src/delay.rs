//! # Алгоритмы задержки
//!
//! Этот модуль предоставляет различные реализации задержки:
//! - Простая задержка (Delay)
//! - Задержка с обратной связью (Feedback Delay)
//! - Многоголовая задержка (Multi-tap Delay)
//! - Диффузионная задержка (для реверберации)

use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm};
use crate::buffer::{DelayLine, RingBuffer};
use rill_core::AudioNum;

// -----------------------------------------------------------------------------
// Базовые параметры задержки
// -----------------------------------------------------------------------------

/// Параметры задержки
#[derive(Debug, Clone)]
pub struct DelayParams {
    /// Время задержки в секундах
    pub delay_time: f32,
    /// Коэффициент обратной связи (0.0 - 1.0)
    pub feedback: f32,
    /// Соотношение dry/wet (0.0 = только dry, 1.0 = только wet)
    pub mix: f32,
    /// Инвертировать обратную связь (для создания особых эффектов)
    pub invert_feedback: bool,
}

impl Default for DelayParams {
    fn default() -> Self {
        Self {
            delay_time: 0.5,
            feedback: 0.3,
            mix: 0.5,
            invert_feedback: false,
        }
    }
}

impl DelayParams {
    /// Создать параметры для простой задержки (без обратной связи)
    pub fn simple(delay_time: f32, mix: f32) -> Self {
        Self {
            delay_time,
            feedback: 0.0,
            mix,
            invert_feedback: false,
        }
    }

    /// Создать параметры для задержки с обратной связью
    pub fn feedback(delay_time: f32, feedback: f32, mix: f32) -> Self {
        Self {
            delay_time,
            feedback: feedback.clamp(0.0, 1.0),
            mix,
            invert_feedback: false,
        }
    }
}

// -----------------------------------------------------------------------------
// Базовая задержка (Simple Delay)
// -----------------------------------------------------------------------------

/// Алгоритм простой задержки
///
/// # Type Parameters
/// - `T`: Тип данных (f32/f64)
/// - `MAX_DELAY`: Максимальная задержка в семплах
pub struct Delay<T: AudioNum, const MAX_DELAY: usize> {
    /// Параметры задержки
    params: DelayParams,
    /// Линия задержки
    delay_line: DelayLine<T, MAX_DELAY>,
    /// Текущая задержка в семплах
    delay_samples: usize,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum, const MAX_DELAY: usize> Delay<T, MAX_DELAY> {
    /// Создать новый алгоритм задержки
    pub fn new(params: DelayParams) -> Self {
        Self {
            params,
            delay_line: DelayLine::new(),
            delay_samples: 0,
            sample_rate: 44100.0,
        }
    }

    /// Обновить задержку в семплах при изменении параметров
    fn update_delay_samples(&mut self) {
        self.delay_samples =
            crate::math::seconds_to_samples(self.params.delay_time, self.sample_rate);
        debug_assert!(
            self.delay_samples < MAX_DELAY,
            "Delay time too long for this delay line"
        );
    }

    /// Установить время задержки (в секундах)
    pub fn set_delay_time(&mut self, time: f32) {
        self.params.delay_time = time;
        self.update_delay_samples();
    }

    /// Установить обратную связь
    pub fn set_feedback(&mut self, feedback: f32) {
        self.params.feedback = feedback.clamp(0.0, 1.0);
    }

    /// Установить соотношение dry/wet
    pub fn set_mix(&mut self, mix: f32) {
        self.params.mix = mix.clamp(0.0, 1.0);
    }

    /// Получить текущую задержку в семплах
    pub fn delay_samples(&self) -> usize {
        self.delay_samples
    }
}

impl<T: AudioNum, const MAX_DELAY: usize> Algorithm<T> for Delay<T, MAX_DELAY> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_delay_samples();
        self.delay_line.reset();
    }

    fn reset(&mut self) {
        self.delay_line.reset();
    }

    fn process_block(&mut self, input: &[T], output: &mut [T]) {
        let len = input.len().min(output.len());
        for i in 0..len {
            // Читаем задержанный сигнал
            let delayed = self.delay_line.read();

            // Вычисляем выход с учётом dry/wet
            let wet = delayed;
            let dry = input[i];
            let mix = T::from_f32(self.params.mix);
            let one_minus_mix = T::from_f32(1.0 - self.params.mix);

            output[i] = dry.mul(one_minus_mix).add(wet.mul(mix));

            // Вычисляем сигнал для записи в линию задержки
            let feedback = T::from_f32(self.params.feedback);
            let write_signal = if self.params.invert_feedback {
                input[i].sub(delayed.mul(feedback))
            } else {
                input[i].add(delayed.mul(feedback))
            };

            // Записываем в линию задержки
            let _ = self.delay_line.write(write_signal);
        }
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Delay",
            category: AlgorithmCategory::Effect,
            description: "Simple delay with feedback",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: AudioNum, const MAX_DELAY: usize> ParameterizedAlgorithm<T> for Delay<T, MAX_DELAY> {
    type Params = DelayParams;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.update_delay_samples();
    }
}

// -----------------------------------------------------------------------------
// Многоголовая задержка (Multi-tap Delay)
// -----------------------------------------------------------------------------

/// Параметры для каждой головки
#[derive(Debug, Clone)]
pub struct TapParams {
    /// Время задержки для этой головки (в секундах)
    pub delay_time: f32,
    /// Коэффициент усиления для этой головки
    pub gain: f32,
    /// Панорама (-1.0 = лево, 1.0 = право)
    pub pan: f32,
}

/// Многоголовая задержка
///
/// Позволяет создать несколько независимых задержек
/// с разными временами и усилением.
pub struct MultiTapDelay<T: AudioNum, const MAX_DELAY: usize, const MAX_TAPS: usize> {
    /// Основная линия задержки
    delay_line: DelayLine<T, MAX_DELAY>,
    /// Параметры каждой головки
    taps: [TapParams; MAX_TAPS],
    /// Задержки в семплах для каждой головки (кэш)
    tap_samples: [usize; MAX_TAPS],
    /// Количество активных головок
    active_taps: usize,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum, const MAX_DELAY: usize, const MAX_TAPS: usize>
    MultiTapDelay<T, MAX_DELAY, MAX_TAPS>
{
    /// Создать новую многоголовую задержку
    pub fn new() -> Self {
        Self {
            delay_line: DelayLine::new(),
            taps: [TapParams {
                delay_time: 0.0,
                gain: 0.0,
                pan: 0.0,
            }; MAX_TAPS],
            tap_samples: [0; MAX_TAPS],
            active_taps: 0,
            sample_rate: 44100.0,
        }
    }

    /// Добавить головку
    pub fn add_tap(&mut self, params: TapParams) -> Result<(), &'static str> {
        if self.active_taps >= MAX_TAPS {
            return Err("Maximum number of taps reached");
        }

        self.taps[self.active_taps] = params;
        self.update_tap_samples(self.active_taps);
        self.active_taps += 1;

        Ok(())
    }

    /// Изменить параметры головки
    pub fn set_tap(&mut self, index: usize, params: TapParams) -> Result<(), &'static str> {
        if index >= self.active_taps {
            return Err("Tap index out of range");
        }

        self.taps[index] = params;
        self.update_tap_samples(index);

        Ok(())
    }

    /// Обновить задержку в семплах для головки
    fn update_tap_samples(&mut self, index: usize) {
        self.tap_samples[index] =
            crate::math::seconds_to_samples(self.taps[index].delay_time, self.sample_rate);
        debug_assert!(self.tap_samples[index] < MAX_DELAY, "Tap delay too long");
    }

    /// Получить количество активных головок
    pub fn active_taps(&self) -> usize {
        self.active_taps
    }
}

impl<T: AudioNum, const MAX_DELAY: usize, const MAX_TAPS: usize> Algorithm<T>
    for MultiTapDelay<T, MAX_DELAY, MAX_TAPS>
{
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;

        // Обновляем все задержки
        for i in 0..self.active_taps {
            self.update_tap_samples(i);
        }

        self.delay_line.reset();
    }

    fn reset(&mut self) {
        self.delay_line.reset();
    }

    fn process_block(&mut self, input: &[T], output: &mut [T]) {
        let len = input.len().min(output.len());
        for i in 0..len {
            // Записываем вход в линию задержки
            let _ = self.delay_line.write(input[i]);

            // Собираем все головки
            let mut out = T::ZERO;

            for tap_idx in 0..self.active_taps {
                let tap = &self.taps[tap_idx];

                // Читаем из линии задержки с нужной задержкой
                // В реальной реализации нужно уметь читать с произвольной задержкой
                // Здесь упрощённо - используем read_delayed из RingBuffer
                // TODO: добавить метод read_delayed в DelayLine
                let delayed = input[i]; // Заглушка

                // Применяем усиление
                let sample = delayed.mul(T::from_f32(tap.gain));

                // Добавляем к выходу (учитывая панораму)
                out = out.add(sample);
            }
            output[i] = out;
        }
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Multi-Tap Delay",
            category: AlgorithmCategory::Effect,
            description: "Delay with multiple independent taps",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

// -----------------------------------------------------------------------------
// Диффузионная задержка (для реверберации)
// -----------------------------------------------------------------------------

/// Диффузионная секция (комбинация allpass фильтров)
pub struct DiffusionDelay<T: AudioNum, const STAGES: usize, const MAX_DELAY: usize> {
    /// Линии задержки для каждого этапа
    delays: [DelayLine<T, MAX_DELAY>; STAGES],
    /// Времена задержки для каждого этапа
    delay_times: [f32; STAGES],
    /// Коэффициенты обратной связи
    feedback: [T; STAGES],
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum, const STAGES: usize, const MAX_DELAY: usize>
    DiffusionDelay<T, STAGES, MAX_DELAY>
{
    /// Создать новую диффузионную задержку
    pub fn new(delay_times: [f32; STAGES], feedback: [f32; STAGES]) -> Self {
        let mut feedback_typed = [T::ZERO; STAGES];
        for i in 0..STAGES {
            feedback_typed[i] = T::from_f32(feedback[i]);
        }

        Self {
            delays: [DelayLine::new(); STAGES],
            delay_times,
            feedback: feedback_typed,
            sample_rate: 44100.0,
        }
    }

    /// Обработать семпл через диффузионную сеть
    pub fn process_diffusion(&mut self, input: T) -> T {
        let mut x = input;

        for i in 0..STAGES {
            // Читаем из линии задержки
            let delayed = self.delays[i].read();

            // Allpass структура: y = -g*x + x_d + g*y_d
            let g = self.feedback[i];
            let y = x
                .mul(g.neg())
                .add(delayed)
                .add(self.delays[i].read().mul(g));

            // Записываем в линию задержки
            let _ = self.delays[i].write(x.add(delayed.mul(g.neg())));

            x = y;
        }

        x
    }
}

impl<T: AudioNum, const STAGES: usize, const MAX_DELAY: usize> Algorithm<T>
    for DiffusionDelay<T, STAGES, MAX_DELAY>
{
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;

        // Устанавливаем задержки
        for i in 0..STAGES {
            let delay_samples = crate::math::seconds_to_samples(self.delay_times[i], sample_rate);
            // TODO: установить задержку в DelayLine
        }

        self.reset();
    }

    fn reset(&mut self) {
        for delay in &mut self.delays {
            delay.reset();
        }
    }

    fn process_block(&mut self, input: &[T], output: &mut [T]) {
        let len = input.len().min(output.len());
        for i in 0..len {
            output[i] = self.process_diffusion(input[i]);
        }
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Diffusion Delay",
            category: AlgorithmCategory::Effect,
            description: "Diffusion network for reverb",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

// -----------------------------------------------------------------------------
// Модуляционная задержка (Chorus/Flanger)
// -----------------------------------------------------------------------------

/// Параметры модуляционной задержки
#[derive(Debug, Clone)]
pub struct ModulatedDelayParams {
    /// Базовая задержка (в секундах)
    pub base_delay: f32,
    /// Глубина модуляции (в секундах)
    pub depth: f32,
    /// Частота модуляции (в Hz)
    pub rate: f32,
    /// Обратная связь
    pub feedback: f32,
    /// Соотношение dry/wet
    pub mix: f32,
}

/// Модуляционная задержка (Chorus/Flanger)
pub struct ModulatedDelay<T: AudioNum, const MAX_DELAY: usize> {
    /// Параметры
    params: ModulatedDelayParams,
    /// Линия задержки
    delay_line: DelayLine<T, MAX_DELAY>,
    /// Текущая фаза LFO
    lfo_phase: T,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum, const MAX_DELAY: usize> ModulatedDelay<T, MAX_DELAY> {
    /// Создать новую модуляционную задержку
    pub fn new(params: ModulatedDelayParams) -> Self {
        Self {
            params,
            delay_line: DelayLine::new(),
            lfo_phase: T::ZERO,
            sample_rate: 44100.0,
        }
    }

    /// Вычислить текущую задержку с учётом модуляции
    fn current_delay(&self) -> f32 {
        let lfo = self.lfo_phase.to_f32().sin() * 0.5 + 0.5; // 0..1
        self.params.base_delay + lfo * self.params.depth
    }
}

impl<T: AudioNum, const MAX_DELAY: usize> Algorithm<T> for ModulatedDelay<T, MAX_DELAY> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.lfo_phase = T::ZERO;
        self.delay_line.reset();
    }

    fn reset(&mut self) {
        self.lfo_phase = T::ZERO;
        self.delay_line.reset();
    }

    fn process_block(&mut self, input: &[T], output: &mut [T]) {
        let len = input.len().min(output.len());
        for i in 0..len {
            // Обновляем фазу LFO
            let phase_inc = T::from_f32(self.params.rate / self.sample_rate);
            self.lfo_phase = self.lfo_phase.add(phase_inc);
            if self.lfo_phase.to_f32() >= 1.0 {
                self.lfo_phase = self.lfo_phase.sub(T::from_f32(1.0));
            }

            // Вычисляем текущую задержку
            let current_delay = self.current_delay();
            let delay_samples = crate::math::seconds_to_samples(current_delay, self.sample_rate);

            // Здесь нужно читать с плавающей задержкой (с интерполяцией)
            // Для простоты используем read_delayed с целой задержкой
            let delayed = self.delay_line.read(); // Заглушка

            // Обратная связь
            let feedback = T::from_f32(self.params.feedback);
            let write_signal = input[i].add(delayed.mul(feedback));
            let _ = self.delay_line.write(write_signal);

            // Dry/wet mix
            let dry = input[i];
            let wet = delayed;
            let mix = T::from_f32(self.params.mix);
            let one_minus_mix = T::from_f32(1.0 - self.params.mix);

            output[i] = dry.mul(one_minus_mix).add(wet.mul(mix));
        }
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Modulated Delay",
            category: AlgorithmCategory::Effect,
            description: "Delay with LFO modulation (Chorus/Flanger)",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

// -----------------------------------------------------------------------------
// Тесты
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delay_basic() {
        let mut delay = Delay::<f32, 1024>::new(DelayParams::simple(0.1, 0.5));
        delay.init(44100.0);

        // Первый семпл - задержки ещё нет
        let mut output = [0.0];
        delay.process_block(&[1.0], &mut output);
        assert_eq!(output[0], 0.5);

        // Второй семпл - задержка ещё не появилась
        delay.process_block(&[1.0], &mut output);
        assert_eq!(output[0], 0.5);
    }

    #[test]
    fn test_delay_feedback() {
        let mut delay = Delay::<f32, 1024>::new(DelayParams::feedback(0.1, 0.5, 0.7));
        delay.init(44100.0);

        // Проверяем, что обратная связь работает
        let mut output = [0.0];
        delay.process_block(&[1.0], &mut output);
        let out1 = output[0];
        delay.process_block(&[0.0], &mut output);
        let out2 = output[0];

        assert!(out2 > 0.0); // Должен быть сигнал от обратной связи
    }

    #[test]
    fn test_multi_tap() {
        let mut multitap = MultiTapDelay::<f32, 1024, 4>::new();
        multitap.init(44100.0);

        multitap
            .add_tap(TapParams {
                delay_time: 0.1,
                gain: 0.5,
                pan: 0.0,
            })
            .unwrap();

        assert_eq!(multitap.active_taps(), 1);
    }
}
