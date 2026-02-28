//! # Функциональные автоматы
//!
//! Универсальные автоматы, построенные на Rust-замыканиях.
//! Это самый гибкий способ создания автоматов — вы можете использовать любую
//! математическую функцию или алгоритм.
//!
//! ## Два варианта
//!
//! 1. [`FunctionAutomaton`] — без сохранения состояния (stateless).
//!    Подходит для чистых функций времени: `f(t) -> значение`.
//!
//! 2. [`StatefulFunctionAutomaton`] — с пользовательским состоянием.
//!    Позволяет создавать генераторы, которым нужно помнить информацию
//!    между вызовами: счётчики, интеграторы, генераторы случайных блужданий.
//!
//! ## Состояние
//!
//! [`FunctionState`] хранит текущее значение, время последнего обновления и,
//! опционально, пользовательские данные любого типа (через `Arc<dyn Any>`).
//!
//! ## Примеры
//!
//! ### Stateless: генератор синуса
//! ```
//! use kama_automation::automaton::FunctionAutomaton;
//!
//! let sine = FunctionAutomaton::new(
//!     "Sine",
//!     |t| (t * 2.0 * std::f64::consts::PI).sin(),
//!     "osc",
//!     "fm_input"
//! );
//! ```
//!
//! ### Stateful: счётчик
//! ```
//! use kama_automation::automaton::StatefulFunctionAutomaton;
//!
//! let counter = StatefulFunctionAutomaton::new(
//!     "Counter",
//!     |_time, count: &mut u32| {
//!         *count += 1;
//!         *count as f64
//!     },
//!     0,
//!     "seq",
//!     "step"
//! );
//! ```

use crate::automaton::Automaton;
use crate::context::AutomationContext;
use crate::signal::SignalSender;
use kama_core::traits::ParameterId;
use std::fmt;
use std::sync::Arc;

/// Состояние функционального автомата
#[derive(Clone)]
pub struct FunctionState {
    /// Текущее значение
    pub value: f64,
    /// Время последнего обновления
    pub last_time: f64,
    /// Пользовательское состояние (для stateful функций)
    pub user_state: Arc<dyn std::any::Any + Send + Sync>,
}

impl FunctionState {
    pub fn new(value: f64, time: f64) -> Self {
        Self {
            value,
            last_time: time,
            user_state: Arc::new(()),
        }
    }

    pub fn with_user_state<T: Send + Sync + 'static>(value: f64, time: f64, state: T) -> Self {
        Self {
            value,
            last_time: time,
            user_state: Arc::new(state),
        }
    }

    pub fn get_user_state<T: 'static>(&self) -> Option<&T> {
        self.user_state.downcast_ref::<T>()
    }
}

/// Обобщённый автомат на основе функции (stateless)
pub struct FunctionAutomaton {
    /// Имя автомата
    pub(crate) name: String,
    /// Генерирующая функция
    pub(crate) generator: Arc<std::sync::Mutex<Box<dyn FnMut(f64) -> f64 + Send + Sync>>>,
    /// Целевой параметр
    target_parameter: ParameterId,
    /// Отправитель сигналов
    signal_sender: Option<Arc<dyn SignalSender>>,
    /// Порог для отправки сигналов
    threshold: f64,
}

impl FunctionAutomaton {
    /// Создать новый функциональный автомат
    pub fn new<F>(name: &str, mut generator: F, target_parameter: ParameterId) -> Self
    where
        F: FnMut(f64) -> f64 + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            generator: Arc::new(std::sync::Mutex::new(Box::new(generator))),
            target_parameter,
            signal_sender: None,
            threshold: 1e-6,
        }
    }

    /// Установить отправитель сигналов
    pub fn with_signal_sender(mut self, sender: Arc<dyn SignalSender>) -> Self {
        self.signal_sender = Some(sender);
        self
    }

    /// Установить порог для отправки сигналов
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold;
        self
    }

    /// Отправить сигнал об изменении параметра
    fn send_signal(&self, value: f64) {
        if let Some(sender) = &self.signal_sender {
            // Временно используем заглушку для порта
            // В реальном использовании порт будет передан через контекст
            let dummy_port = PortId::node(0.into()); // Временное решение
            sender.send_parameter_changed(
                dummy_port,
                self.target_parameter.clone(),
                value as f32
            );
        }
    }
}