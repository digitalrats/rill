//! Автоматы — разумные существа, генерирующие сигналы

use crate::core::{WorldTime, WorldSignal};
use std::fmt;

/// Базовый трейт для всех автоматов
pub trait Automaton: Send + 'static {
    /// Имя автомата
    fn name(&self) -> &str;
    
    /// Обработать один тик времени
    fn process(&mut self, time: WorldTime, inputs: &[WorldSignal]) -> Vec<WorldSignal>;
    
    /// Получить текущее значение (для отладки)
    fn peek(&self) -> f32;
    
    /// Сбросить состояние
    fn reset(&mut self);
}

/// Автомат-источник (не имеет входов)
pub trait SourceAutomaton: Automaton {
    /// Установить параметр
    fn set_parameter(&mut self, name: &str, value: f32);
}

/// Автомат-процессор (имеет входы)
pub trait ProcessorAutomaton: Automaton {
    /// Количество входов
    fn num_inputs(&self) -> usize;
    
    /// Получить значение на входе (для отладки)
    fn input(&self, idx: usize) -> Option<f32>;
}

/// Тип сигнала в мире автоматов
#[derive(Debug, Clone, Copy)]
pub enum SignalType {
    Continuous,  // Непрерывный (0-1)
    Gate,        // Вкл/Выкл
    Trigger,     // Импульс
    Pulse,       // Пульс
}

/// Значение сигнала
#[derive(Debug, Clone)]
pub struct SignalValue {
    pub normalized: f32,  // 0.0 - 1.0
    pub typ: SignalType,
}

impl SignalValue {
    pub fn continuous(value: f32) -> Self {
        Self {
            normalized: value.clamp(0.0, 1.0),
            typ: SignalType::Continuous,
        }
    }
    
    pub fn gate(active: bool) -> Self {
        Self {
            normalized: if active { 1.0 } else { 0.0 },
            typ: SignalType::Gate,
        }
    }
    
    pub fn trigger() -> Self {
        Self {
            normalized: 1.0,
            typ: SignalType::Trigger,
        }
    }
}