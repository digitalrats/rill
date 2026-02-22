// kama-automation/src/automaton/mod.rs
//! # Автоматы — генераторы управляющих сигналов
//!
//! Автоматы — это источник данных для автоматизации. Каждый автомат
//! специализируется на одной задаче: генерация LFO, огибающей, случайного
//! сигнала или любой другой функции времени.
//!
//! ## Принцип работы
//!
//! Автомат получает на вход текущее время и внешнее действие (например,
//! "сбросить фазу"). На основе своего внутреннего состояния он вычисляет
//! новое состояние и, опционально, следующее действие. Текущее значение
//! можно извлечь через [`Automaton::extract_value`].
//!
//! ## Основные компоненты
//!
//! - [`FunctionAutomaton`] — автомат из замыкания (stateless)
//! - [`StatefulFunctionAutomaton`] — автомат с пользовательским состоянием
//! - [`LfoAutomaton`] — специализированные конструкторы для LFO
//! - [`LfoWithEnvelopeAutomaton`] — LFO с огибающей
//!
//! ## Пример
//! 
//! ```
//! use kama_automation::automaton::{FunctionAutomaton, Automaton};
//! use kama_automation::AutomationContext;
//! use std::sync::Arc;
//! use kama_core_traits::time::SystemClock;
//! 
//! // Создаём контекст вручную (dummy доступен только в тестах)
//! let clock = Arc::new(SystemClock::new(44100.0, 120.0));
//! let ctx = AutomationContext::new(clock);
//! 
//! // Простой LFO через замыкание
//! let lfo = FunctionAutomaton::new(
//!     "Sine LFO",
//!     |time| (time * 2.0 * std::f64::consts::PI).sin(),
//!     "filter",
//!     "cutoff"
//! );
//! 
//! let state = lfo.initial_state();
//! let (new_state, _) = lfo.step(0.5, &ctx, (), &state);
//! println!("Value at t=0.5: {}", lfo.extract_value(&new_state));
//! ```
//! use kama_automation::automaton::{FunctionAutomaton, Automaton};
//! use kama_automation::AutomationContext;
//!
//! // Простой LFO через замыкание
//! let lfo = FunctionAutomaton::new(
//!     "Sine LFO",
//!     |time| (time * 2.0 * std::f64::consts::PI).sin(),
//!     "filter",
//!     "cutoff"
//! );
//!
//! let state = lfo.initial_state();
//! let ctx = AutomationContext::dummy();
//! let (new_state, _) = lfo.step(0.5, &ctx, (), &state);
//! println!("Value at t=0.5: {}", lfo.extract_value(&new_state));
//! ```

mod function;
mod lfo;

pub use function::{FunctionAutomaton, StatefulFunctionAutomaton, FunctionState, GeneratorFn};
pub use lfo::{LfoAutomaton, LfoWithEnvelopeAutomaton};

// Реэкспортируем из kama-oscillators для удобства
pub use kama_oscillators::control::LfoWaveform as Waveform;

use crate::context::AutomationContext;

/// Базовый трейт для всех автоматов.
///
/// Автомат — это конечный автомат, который генерирует значения во времени.
/// Он может получать внешние действия и обновлять своё состояние.
///
/// # Типовые параметры
///
/// - `Time`: тип времени (обычно `f64` для секунд)
/// - `Context`: контекст выполнения ([`AutomationContext`])
/// - `Action`: внешнее действие, которое можно применить к автомату
/// - `State`: внутреннее состояние автомата
///
/// # Пример реализации для счётчика
///
/// ```
/// use kama_automation::{Automaton, AutomationContext};
///
/// struct Counter;
///
/// impl Automaton for Counter {
///     type Time = f64;
///     type Context = AutomationContext;
///     type Action = ();
///     type State = u32;
///
///     fn step(&self, _time: f64, _ctx: &AutomationContext,
///             _action: (), state: &u32) -> (u32, Option<()>) {
///         (state + 1, None)
///     }
///
///     fn initial_state(&self) -> u32 { 0 }
///     fn name(&self) -> &str { "Counter" }
///     fn extract_value(&self, state: &u32) -> f64 { *state as f64 }
/// }
/// ```
pub trait Automaton: Send + Sync {
    /// Тип времени (обычно `f64` для секунд).
    type Time;
    /// Тип контекста (обычно [`AutomationContext`]).
    type Context;
    /// Тип внешнего действия.
    type Action: Clone + Default + Send + Sync + 'static;
    /// Тип внутреннего состояния.
    type State: Clone + Send + Sync + 'static;
    
    /// Выполнить один шаг автомата.
    ///
    /// # Аргументы
    /// * `time` — текущее время
    /// * `context` — контекст выполнения
    /// * `action` — внешнее действие
    /// * `state` — текущее состояние
    ///
    /// # Возвращает
    /// Кортеж из нового состояния и, опционально, следующего действия.
    fn step(
        &self,
        time: Self::Time,
        context: &Self::Context,
        action: Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<Self::Action>);
    
    /// Начальное состояние автомата.
    fn initial_state(&self) -> Self::State;
    /// Имя автомата (для отладки и метаданных).
    fn name(&self) -> &str;
    /// Извлечь текущее значение из состояния.
    ///
    /// Это значение будет отправлено в целевой параметр через сервопривод.
    fn extract_value(&self, state: &Self::State) -> f64;
}

/// Вспомогательный трейт для создания автоматов из замыканий.
///
/// Позволяет писать лаконичный код:
///
/// ```
/// use kama_automation::automaton::IntoAutomaton;
///
/// let automaton = (|t: f64| (t * 2.0).sin()).into_automaton("filter", "cutoff");
/// ```
pub trait IntoAutomaton {
    /// Тип создаваемого автомата.
    type Automaton: Automaton + 'static;
    
    /// Преобразовать замыкание в автомат.
    ///
    /// # Аргументы
    /// * `target_node` — ID целевого узла (для метаданных)
    /// * `target_param` — имя целевого параметра (для метаданных)
    fn into_automaton(self, target_node: &str, target_param: &str) -> Self::Automaton;
}

impl<F> IntoAutomaton for F
where
    F: Fn(f64) -> f64 + Send + Sync + 'static,
{
    type Automaton = FunctionAutomaton;
    
    fn into_automaton(self, target_node: &str, target_param: &str) -> Self::Automaton {
        FunctionAutomaton::new("Custom", self, target_node, target_param)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_into_automaton() {
        let automaton = (|t: f64| t.sin()).into_automaton("test", "param");
        assert_eq!(automaton.name(), "Custom");
    }
}