//! # Генераторы сигналов
//!
//! Этот модуль предоставляет различные генераторы для синтеза звука:
//! - Базовые осцилляторы (Sine, Saw, Square, Triangle, Pulse)
//! - Шумовые генераторы (White, Pink, Brown, Blue, Violet)
//! - Огибающие (ADSR, AR, ASR)
//! - LFO для модуляции
//! - FM синтез
//!
//! Все генераторы реализуют общий трейт [`Generator`] и параметризованы
//! типом `T: Transcendental` (f32 или f64).

// Импортируем необходимые типы и трейты
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::Transcendental;

// Объявляем подмодули
mod basic;
mod envelope;
mod fm;
mod lfo;
mod noise;

// Реэкспортируем всё из подмодулей
pub use basic::*;
pub use envelope::*;
pub use fm::*;
pub use lfo::*;
pub use noise::*;

/// Базовый трейт для всех генераторов
///
/// Предоставляет основные методы управления генератором:
/// - управление фазой
/// - изменение частоты
/// - изменение амплитуды
pub trait Generator<T: Transcendental>: Algorithm<T> {
    /// Получить текущую фазу (0.0 - 1.0)
    fn phase(&self) -> T;

    /// Установить фазу
    fn set_phase(&mut self, phase: T);

    /// Сбросить фазу в 0
    fn reset_phase(&mut self) {
        self.set_phase(T::ZERO);
    }

    /// Получить частоту в Hz
    fn frequency(&self) -> f32;

    /// Установить частоту
    fn set_frequency(&mut self, freq: f32);

    /// Получить амплитуду
    fn amplitude(&self) -> T;

    /// Установить амплитуду
    fn set_amplitude(&mut self, amp: T);
}

/// Генератор с синхронизацией
///
/// Позволяет синхронизировать несколько генераторов
/// по фазе или тактовому сигналу.
pub trait SyncableGenerator<T: Transcendental>: Generator<T> {
    /// Синхронизировать с внешним тактовым сигналом
    ///
    /// # Arguments
    /// * `reset` - если true, сбросить фазу в 0
    fn sync(&mut self, reset: bool);

    /// Получить количество периодов с последнего сброса
    fn periods(&self) -> u32;
}

/// Генератор с модуляцией частоты
///
/// Поддерживает частотную модуляцию (FM) для создания
/// сложных тембров.
pub trait ModulatableGenerator<T: Transcendental>: Generator<T> {
    /// Применить модуляцию частоты
    ///
    /// # Arguments
    /// * `amount` - величина модуляции
    fn modulate_frequency(&mut self, amount: T);

    /// Индекс модуляции (текущая величина FM)
    fn modulation_index(&self) -> T;

    /// Установить индекс модуляции
    fn set_modulation_index(&mut self, index: T);
}

// =============================================================================
// Сравнение генераторов
// =============================================================================

/// Сводка характеристик генераторов
#[derive(Debug)]
pub struct GeneratorComparison;

impl GeneratorComparison {
    /// Гармонический состав разных генераторов
    pub fn harmonic_content() -> &'static str {
        "Гармонический состав:\n\
         ┌────────────┬─────────────────────────────────┐\n\
         │ Генератор  │ Спектр                          │\n\
         ├────────────┼─────────────────────────────────┤\n\
         │ Sine       │ Одна гармоника (чистый тон)     │\n\
         │ Triangle   │ Нечётные, быстрое затухание     │\n\
         │ Saw        │ Все гармоники (1/n)             │\n\
         │ Square     │ Нечётные гармоники (1/n)        │\n\
         │ Pulse      │ Зависит от ширины импульса      │\n\
         │ White      │ Равномерный по всем частотам    │\n\
         │ Pink       │ Спад 3dB/октаву (1/f)           │\n\
         │ Brown      │ Спад 6dB/октаву (1/f²)          │\n\
         └────────────┴─────────────────────────────────┘"
    }

    /// Рекомендации по применению
    pub fn usage_guide() -> &'static str {
        "Как выбрать генератор:\n\n\
         🎵 **Субтрактивный синтез**:\n\
         → Saw, Square, Pulse - богатый спектр для фильтрации\n\n\
         🎵 **FM синтез**:\n\
         → Sine - чистый тон для модуляции\n\n\
         🎵 **Аддитивный синтез**:\n\
         → Sine (множество) - построение сложных тембров\n\n\
         🎵 **Шумовые эффекты**:\n\
         → White - ветер, snare drum\n\
         → Pink - естественные явления\n\
         → Brown - гром, рокот\n\n\
         🎵 **Огибающие**:\n\
         → ADSR - амплитудные огибающие\n\
         → AR - перкуссия\n\
         → ASR - орга́нные звуки\n\n\
         🎵 **Модуляция**:\n\
         → LFO - вибрато, тремоло, фильтр-свип"
    }

    /// Характеристики производительности
    pub fn performance_guide() -> &'static str {
        "Производительность (относительная):\n\
         ⚡ **Sine** - 1x (самый быстрый)\n\
         ⚡⚡ **Triangle, Square** - 1.5x\n\
         ⚡⚡⚡ **Saw** - 2x (с анти-алиасингом)\n\
         ⚡⚡⚡ **Noise** - 2x\n\
         ⚡⚡⚡⚡ **Envelope** - 3x\n\
         ⚡⚡⚡⚡ **FM Synth** - зависит от числа операторов"
    }
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generator_trait_bounds() {
        // Проверяем, что все генераторы реализуют нужные трейты
        fn assert_generator<T: Transcendental, G: Generator<T>>() {}
        fn assert_syncable<T: Transcendental, G: SyncableGenerator<T>>() {}
        fn assert_modulatable<T: Transcendental, G: ModulatableGenerator<T>>() {}

        assert_generator::<f32, BasicOscillator<f32>>();
        assert_generator::<f32, NoiseGenerator<f32>>();
        assert_generator::<f32, EnvelopeGenerator<f32>>();
        assert_generator::<f32, LFO<f32>>();
        assert_generator::<f32, SimpleFmSynth<f32>>();

        assert_syncable::<f32, BasicOscillator<f32>>();
        assert_modulatable::<f32, BasicOscillator<f32>>();
    }

    #[test]
    fn test_comparison_guides() {
        assert!(!GeneratorComparison::harmonic_content().is_empty());
        assert!(!GeneratorComparison::usage_guide().is_empty());
        assert!(!GeneratorComparison::performance_guide().is_empty());
    }
}
