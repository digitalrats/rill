//! # Генераторы сигналов
//!
//! Этот модуль предоставляет различные генераторы для синтеза звука:
//! - Базовые осцилляторы (Sine, Saw, Square, Triangle)
//! - Шумовые генераторы (White, Pink, Brown, Blue, Violet)
//! - Огибающие (ADSR, AR, ASR)
//! - LFO для модуляции
//! - Pulse wave с PWM
//! - Вейвтейбл генераторы
//! - FM синтез
//!
//! Все генераторы параметризованы типом `T: AudioNum` и RT-safe.

mod basic;
mod noise;
mod envelope;
mod lfo;
mod pulse;
mod wavetable;
mod fm;

pub use basic::*;
pub use noise::*;
pub use envelope::*;
pub use lfo::*;
pub use pulse::*;
pub use wavetable::*;
pub use fm::*;

use crate::math::AudioNum;
use crate::algorithm::{Algorithm, AlgorithmMetadata, AlgorithmCategory};

/// Базовый трейт для всех генераторов
pub trait Generator<T: AudioNum>: Algorithm<T> {
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
pub trait SyncableGenerator<T: AudioNum>: Generator<T> {
    /// Синхронизировать с внешним тактовым сигналом
    fn sync(&mut self, reset: bool);
    
    /// Получить количество периодов с последнего сброса
    fn periods(&self) -> u32;
}

/// Генератор с модуляцией частоты
pub trait ModulatableGenerator<T: AudioNum>: Generator<T> {
    /// Применить модуляцию частоты
    fn modulate_frequency(&mut self, amount: T);
    
    /// Индекс модуляции
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
         → LFO - вибрато, тремоло, фильтр-свип\n\
         → Random - генеративные патчи"
    }
}