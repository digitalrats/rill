//! # Автоматы — генеративные источники управления
//!
//! Модуль предоставляет различные типы автоматов для генерации
//! управляющих сигналов в реальном времени.

mod lfo;
mod envelope;
mod function;
mod sequencer;
mod random;
mod cellular;

pub use lfo::*;
pub use envelope::*;
pub use function::*;
pub use sequencer::*;
pub use random::*;
pub use cellular::*;

use std::fmt::Debug;
use std::sync::Arc;

/// Тип времени для автоматов (секунды)
pub type Time = f64;

// =============================================================================
// Базовый трейт Automaton
// =============================================================================

/// Базовый трейт для всех автоматов
pub trait Automaton: Send + Sync + Debug {
    /// Тип внутреннего состояния
    type State: Clone + Send + Sync + 'static + Debug;
    
    /// Тип внешних действий
    type Action: Clone + Default + Send + Sync + 'static;
    
    /// Выполнить один шаг автомата
    fn step(
        &self,
        time: Time,
        action: Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<f64>, Option<Self::Action>);
    
    /// Начальное состояние автомата
    fn initial_state(&self) -> Self::State;
    
    /// Имя автомата (для отладки и GUI)
    fn name(&self) -> &str;
    
    /// Извлечь текущее значение из состояния
    fn extract_value(&self, state: &Self::State) -> f64;
    
    /// Сбросить автомат (создать новое начальное состояние)
    fn reset(&self) -> Self::State {
        self.initial_state()
    }
}

// =============================================================================
// Вспомогательные типы
// =============================================================================

/// Тип действия для автоматов, не поддерживающих внешние действия
#[derive(Debug, Clone, Default)]
pub struct NoAction;

/// Режим синхронизации автомата
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncMode {
    Free,
    Sync,
    OneShot,
}

/// Диапазон значений автомата
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
        Self { min: -1.0, max: 1.0 }
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
// Тип-обёртка для хранения разнородных автоматов
// =============================================================================

/// Универсальный тип для хранения любого автомата
pub type BoxedAutomaton = Box<dyn DynAutomaton>;

/// Динамический автомат (стирание типа)
pub trait DynAutomaton: Send + Sync + Debug {
    fn step_dyn(
        &self,
        time: Time,
        action: Box<dyn std::any::Any + Send>,
        state: Box<dyn std::any::Any + Send>,
    ) -> (Box<dyn std::any::Any + Send>, Option<f64>, Option<Box<dyn std::any::Any + Send>>);
    
    fn initial_state_dyn(&self) -> Box<dyn std::any::Any + Send>;
    fn name(&self) -> &str;
    fn extract_value_dyn(&self, state: &dyn std::any::Any) -> f64;
    fn clone_box(&self) -> Box<dyn DynAutomaton>;
}

impl<A> DynAutomaton for A
where
    A: Automaton + Clone + 'static,
    A::State: 'static,
    A::Action: 'static,
{
    fn step_dyn(
        &self,
        time: Time,
        action: Box<dyn std::any::Any + Send>,
        state: Box<dyn std::any::Any + Send>,
    ) -> (Box<dyn std::any::Any + Send>, Option<f64>, Option<Box<dyn std::any::Any + Send>>) {
        let action = *action.downcast::<A::Action>().unwrap();
        let state = *state.downcast::<A::State>().unwrap();
        let (new_state, value, next_action) = self.step(time, action, &state);
        (
            Box::new(new_state),
            value,
            next_action.map(|a| Box::new(a) as Box<dyn std::any::Any + Send>)
        )
    }
    
    fn initial_state_dyn(&self) -> Box<dyn std::any::Any + Send> {
        Box::new(self.initial_state())
    }
    
    fn name(&self) -> &str {
        self.name()
    }
    
    fn extract_value_dyn(&self, state: &dyn std::any::Any) -> f64 {
        let state = state.downcast_ref::<A::State>().unwrap();
        self.extract_value(state)
    }
    
    fn clone_box(&self) -> Box<dyn DynAutomaton> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn DynAutomaton> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

// =============================================================================
// Менеджер автоматов
// =============================================================================

/// Менеджер для коллекции автоматов
#[derive(Debug, Default, Clone)]
pub struct AutomatonManager {
    automata: Vec<BoxedAutomaton>,
    states: Vec<Box<dyn std::any::Any + Send>>,
    actions: Vec<Box<dyn std::any::Any + Send>>,
}

impl AutomatonManager {
    /// Создать новый менеджер
    pub fn new() -> Self {
        Self {
            automata: Vec::new(),
            states: Vec::new(),
            actions: Vec::new(),
        }
    }
    
    /// Добавить автомат
    pub fn add<A: Automaton + Clone + 'static>(&mut self, automaton: A)
    where
        A::State: 'static,
        A::Action: 'static,
    {
        let state = automaton.initial_state();
        let action = A::Action::default();
        self.automata.push(Box::new(automaton));
        self.states.push(Box::new(state));
        self.actions.push(Box::new(action));
    }
    
    /// Обновить все автоматы
    pub fn update(&mut self, time: Time) -> Vec<(usize, f64)> {
        let mut results = Vec::new();
        
        for i in 0..self.automata.len() {
            let automaton = &self.automata[i];
            let action = std::mem::replace(&mut self.actions[i], Box::new(NoAction));
            let state = std::mem::replace(&mut self.states[i], Box::new(()));
            
            let (new_state, value, next_action) = automaton.step_dyn(time, action, state);
            
            self.states[i] = new_state;
            if let Some(next) = next_action {
                self.actions[i] = next;
            }
            
            if let Some(val) = value {
                results.push((i, val));
            }
        }
        
        results
    }
    
    /// Отправить действие конкретному автомату
    pub fn send_action<A: Automaton + 'static>(
        &mut self,
        index: usize,
        action: A::Action,
    ) -> Result<(), &'static str>
    where
        A::Action: 'static,
    {
        if index >= self.automata.len() {
            return Err("Invalid automaton index");
        }
        
        self.actions[index] = Box::new(action);
        Ok(())
    }
    
    /// Получить значение автомата
    pub fn get_value(&self, index: usize) -> Option<f64> {
        if index >= self.automata.len() {
            return None;
        }
        
        Some(self.automata[index].extract_value_dyn(&*self.states[index]))
    }
    
    /// Получить имя автомата
    pub fn get_name(&self, index: usize) -> Option<&str> {
        self.automata.get(index).map(|a| a.name())
    }
    
    /// Количество автоматов
    pub fn len(&self) -> usize {
        self.automata.len()
    }
    
    /// Проверить, пуст ли менеджер
    pub fn is_empty(&self) -> bool {
        self.automata.is_empty()
    }
    
    /// Очистить все автоматы
    pub fn clear(&mut self) {
        self.automata.clear();
        self.states.clear();
        self.actions.clear();
    }
    
    /// Сбросить все автоматы
    pub fn reset_all(&mut self) {
        for i in 0..self.automata.len() {
            let automaton = &self.automata[i];
            self.states[i] = automaton.initial_state_dyn();
            self.actions[i] = Box::new(NoAction);
        }
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
    use super::*;
    
    #[test]
    fn test_automaton_manager() {
        let mut manager = AutomatonManager::new();
        
        let automaton = function::FunctionAutomaton::new("Test", |t| (t * 2.0).sin());
        manager.add(automaton);
        assert_eq!(manager.len(), 1);
        
        let results = manager.update(1.0);
        assert!(!results.is_empty());
    }
    
    #[test]
    fn test_comparison_guides() {
        assert!(!AutomatonComparison::types().is_empty());
        assert!(!AutomatonComparison::selection_guide().is_empty());
        assert!(!AutomatonComparison::performance_guide().is_empty());
    }
}