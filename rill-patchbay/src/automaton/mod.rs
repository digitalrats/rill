//! # Автоматы — генеративные источники управления
//!
//! Модуль предоставляет различные типы автоматов для генерации
//! управляющих сигналов в реальном времени.
//!
//! Автомат — это алгоритм с внутренним состоянием: он принимает
//! время и текущее состояние, возвращает новое состояние и значение.
//! Внешние воздействия на состояние не предполагаются.

pub mod cellular;
pub mod envelope;
pub mod function;
pub mod lfo;
pub mod random;
pub mod sequencer;

pub use cellular::*;
pub use envelope::*;
pub use function::*;
pub use lfo::*;
pub use random::*;
pub use sequencer::*;

use std::fmt::Debug;

// =============================================================================
// Вспомогательные типы
// =============================================================================

/// Режим синхронизации автомата
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncMode {
    Free,
    Sync,
    OneShot,
}

/// Диапазон значений автомата
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

impl Range {
    pub const fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }

    pub const fn unipolar() -> Self {
        Self { min: 0.0, max: 1.0 }
    }

    pub const fn bipolar() -> Self {
        Self {
            min: -1.0,
            max: 1.0,
        }
    }

    pub fn clamp(&self, value: f64) -> f64 {
        value.clamp(self.min, self.max)
    }

    pub fn normalize(&self, value: f64) -> f64 {
        (value - self.min) / (self.max - self.min)
    }

    pub fn denormalize(&self, norm: f64) -> f64 {
        self.min + norm * (self.max - self.min)
    }
}

// =============================================================================
// Сравнение автоматов
// =============================================================================

/// Сводка характеристик автоматов
#[derive(Debug)]
pub struct AutomatonComparison;

impl AutomatonComparison {
    /// Сравнение типов автоматов
    pub fn types() -> &'static str {
        "Типы автоматов:\n\
         ┌─────────────────┬─────────────────────────────┬─────────────────┐\n\
         │ Автомат         │ Характеристики              │ Применение      │\n\
         ├─────────────────┼─────────────────────────────┼─────────────────┤\n\
         │ LFO             │ Гармонические/релаксационные│ Вибрато, тремоло│\n\
         │ Envelope        │ ADSR, AR, ASR               │ Амплитудные ог. │\n\
         │ Function        │ Произвольная функция времени│ Сложные модуляции│\n\
         │ Sequencer       │ Паттерны, ступени           │ Ритмические     │\n\
         │ RandomWalk      │ Случайные блуждания         │ Генеративные    │\n\
         │ Chaos           │ Детерминированный хаос      │ Непредсказуемые │\n\
         │ Cellular        │ Клеточные автоматы          │ Органические    │\n\
         └─────────────────┴─────────────────────────────┴─────────────────┘"
    }

    /// Руководство по выбору автомата
    pub fn selection_guide() -> &'static str {
        "Как выбрать автомат:\n\n\
         🎯 **Периодическая модуляция**:\n\
         → LFO (Sine, Triangle, Saw, Square)\n\n\
         🎯 **Однократные события**:\n\
         → Envelope (ADSR, AR, ASR)\n\n\
         🎯 **Сложные функции**:\n\
         → Function с произвольным замыканием\n\n\
         🎯 **Ритмические паттерны**:\n\
         → Sequencer с шагами и длительностями\n\n\
         🎯 **Генеративные процессы**:\n\
         → RandomWalk, Chaos, Cellular\n\n\
         🎯 **Случайные значения**:\n\
         → Sample & Hold (LFO в режиме S&H)"
    }

    /// Производительность автоматов
    pub fn performance_guide() -> &'static str {
        "Производительность (относительная):\n\
         ⚡ **Function** - 1x (простые функции)\n\
         ⚡⚡ **LFO** - 2x (тригонометрия)\n\
         ⚡⚡⚡ **Envelope** - 3x (логика переходов)\n\
         ⚡⚡⚡ **RandomWalk** - 3x (RNG)\n\
         ⚡⚡⚡⚡ **Sequencer** - 4x (паттерны)\n\
         ⚡⚡⚡⚡ **Chaos** - 4x (итерации)\n\
         ⚡⚡⚡⚡⚡ **Cellular** - 5x (соседи)"
    }
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::control::Automaton as _;

    #[test]
    fn test_automaton_types_are_debug() {
        // Проверяем, что все типы автоматов реализуют Debug (нужно для трейта)
        fn assert_debug<T: std::fmt::Debug>(_: &T) {}
        let lfo = super::LfoAutomaton::new("test", 1.0, 1.0, 0.0, super::LfoWaveform::Sine);
        assert_debug(&lfo);
        let env = super::EnvelopeAutomaton::adsr("test", 0.1, 0.2, 0.7, 0.3);
        assert_debug(&env);
        let func = super::FunctionAutomaton::new("test", |t| t);
        assert_debug(&func);
    }

    #[test]
    fn test_comparison_guides() {
        assert!(!super::AutomatonComparison::types().is_empty());
        assert!(!super::AutomatonComparison::selection_guide().is_empty());
        assert!(!super::AutomatonComparison::performance_guide().is_empty());
    }
}
