//! # Функциональные автоматы
//!
//! Автоматы, построенные на произвольных функциях времени.
//! Позволяют реализовать любую математическую зависимость.

use super::{Automaton, Time, Range, NoAction};
use std::sync::Arc;

/// Состояние функционального автомата
#[derive(Debug, Clone)]
pub struct FunctionState {
    /// Последнее вычисленное значение
    pub value: f64,
    /// Время последнего обновления
    pub last_time: Time,
    /// Пользовательское состояние (для stateful функций)
    pub user_state: Arc<dyn std::any::Any + Send + Sync>,
}

/// Функциональный автомат (stateless)
#[derive(Debug, Clone)]
pub struct FunctionAutomaton {
    /// Имя автомата
    name: String,
    /// Функция-генератор
    generator: Arc<dyn Fn(Time) -> f64 + Send + Sync>,
    /// Диапазон выходных значений
    range: Range,
}

impl FunctionAutomaton {
    /// Создать новый функциональный автомат
    pub fn new<F>(name: &str, generator: F) -> Self
    where
        F: Fn(Time) -> f64 + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            generator: Arc::new(generator),
            range: Range::bipolar(),
        }
    }
    
    /// Установить диапазон
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }
}

impl Automaton for FunctionAutomaton {
    type State = FunctionState;
    type Action = NoAction;
    
    fn step(
        &self,
        time: Time,
        _action: Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<f64>, Option<Self::Action>) {
        let value = (self.generator)(time);
        let clamped = self.range.clamp(value);
        
        let new_state = FunctionState {
            value: clamped,
            last_time: time,
            user_state: state.user_state.clone(),
        };
        
        (new_state, Some(clamped), None)
    }
    
    fn initial_state(&self) -> Self::State {
        FunctionState {
            value: 0.0,
            last_time: 0.0,
            user_state: Arc::new(()),
        }
    }
    
    fn name(&self) -> &str {
        &self.name
    }
    
    fn extract_value(&self, state: &Self::State) -> f64 {
        state.value
    }
}

/// Функциональный автомат с состоянием
#[derive(Debug, Clone)]
pub struct StatefulFunctionAutomaton<S> {
    /// Имя автомата
    name: String,
    /// Функция-генератор с состоянием
    generator: Arc<dyn Fn(Time, &mut S) -> f64 + Send + Sync>,
    /// Начальное состояние
    initial_state: S,
    /// Диапазон выходных значений
    range: Range,
}

impl<S: Send + Sync + Clone + 'static> StatefulFunctionAutomaton<S> {
    /// Создать новый автомат с состоянием
    pub fn new<F>(name: &str, generator: F, initial_state: S) -> Self
    where
        F: Fn(Time, &mut S) -> f64 + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            generator: Arc::new(generator),
            initial_state,
            range: Range::bipolar(),
        }
    }
    
    /// Установить диапазон
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }
}

impl<S: Send + Sync + Clone + 'static> Automaton for StatefulFunctionAutomaton<S> {
    type State = (f64, S);  // (значение, пользовательское состояние)
    type Action = NoAction;
    
    fn step(
        &self,
        time: Time,
        _action: Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<f64>, Option<Self::Action>) {
        let mut user_state = state.1.clone();
        let value = (self.generator)(time, &mut user_state);
        let clamped = self.range.clamp(value);
        
        ((clamped, user_state), Some(clamped), None)
    }
    
    fn initial_state(&self) -> Self::State {
        let value = (self.generator)(0.0, &mut self.initial_state.clone());
        (self.range.clamp(value), self.initial_state.clone())
    }
    
    fn name(&self) -> &str {
        &self.name
    }
    
    fn extract_value(&self, state: &Self::State) -> f64 {
        state.0
    }
}

/// Функция-генератор для LFO (удобная обёртка)
pub fn lfo_function(
    freq: f64,
    phase: f64,
    waveform: &'static str,
) -> impl Fn(Time) -> f64 {
    move |t| {
        let p = (t * freq + phase).fract();
        match waveform {
            "sine" => (p * 2.0 * std::f64::consts::PI).sin(),
            "saw" => 2.0 * p - 1.0,
            "square" => if p < 0.5 { 1.0 } else { -1.0 },
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_function_automaton() {
        let automaton = FunctionAutomaton::new("Test", |t| (t * 2.0).sin());
        let state = automaton.initial_state();
        
        let (state, value, _) = automaton.step(1.0, NoAction, &state);
        assert!(value.is_some());
    }
    
    #[test]
    fn test_stateful_automaton() {
        let automaton = StatefulFunctionAutomaton::new(
            "Counter",
            |_t, counter: &mut i32| {
                *counter += 1;
                *counter as f64
            },
            0,
        );
        
        let state = automaton.initial_state();
        assert_eq!(state.0, 1.0);
        
        let (state, _, _) = automaton.step(1.0, NoAction, &state);
        assert_eq!(state.0, 2.0);
    }
}