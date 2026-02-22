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

//! Обобщённый автомат на основе функции-генератора

use crate::automaton::Automaton;
use crate::context::AutomationContext;
use crate::signal::SignalSender;
use std::sync::Arc;
use std::fmt;

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
    /// Создать новое состояние
    /// Создать новый функциональный автомат.
    ///
    /// # Аргументы
    /// * `name` — имя автомата
    /// * `generator` — замыкание времени
    /// * `target_node` — ID целевого узла
    /// * `target_param` — имя параметра
    pub fn new(value: f64, time: f64) -> Self {
        Self {
            value,
            last_time: time,
            user_state: Arc::new(()),
        }
    }
    
    /// Создать состояние с пользовательскими данными
    pub fn with_user_state<T: Send + Sync + 'static>(value: f64, time: f64, state: T) -> Self {
        Self {
            value,
            last_time: time,
            user_state: Arc::new(state),
        }
    }
    
    /// Получить ссылку на пользовательское состояние
    pub fn get_user_state<T: 'static>(&self) -> Option<&T> {
        self.user_state.downcast_ref::<T>()
    }
}

impl fmt::Debug for FunctionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FunctionState")
            .field("value", &self.value)
            .field("last_time", &self.last_time)
            .field("user_state", &self.user_state.type_id())
            .finish()
    }
}

/// Тип функции-генератора без состояния (может быть FnMut)
pub type GeneratorFn = Arc<dyn FnMut(f64) -> f64 + Send + Sync>;

/// Тип функции-генератора с состоянием
pub type StatefulGeneratorFn<S> = Arc<dyn FnMut(f64, &mut S) -> f64 + Send + Sync>;

/// Обобщённый автомат на основе функции (stateless)
pub struct FunctionAutomaton {
    /// Имя автомата
    pub(crate) name: String,
    /// Генерирующая функция
    pub(crate) generator: Arc<std::sync::Mutex<Box<dyn FnMut(f64) -> f64 + Send + Sync>>>,
    /// ID целевого узла
    target_node: String,
    /// Имя целевого параметра
    target_param: String,
    /// Отправитель сигналов
    signal_sender: Option<Arc<dyn SignalSender>>,
    /// Порог для отправки сигналов
    threshold: f64,
}

impl FunctionAutomaton {
    /// Создать новый функциональный автомат
    pub fn new<F>(
        name: &str,
        mut generator: F,
        target_node: &str,
        target_param: &str,
    ) -> Self
    where
        F: FnMut(f64) -> f64 + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            generator: Arc::new(std::sync::Mutex::new(Box::new(generator))),
            target_node: target_node.to_string(),
            target_param: target_param.to_string(),
            signal_sender: None,
            threshold: 1e-6,
        }
    }
    
    /// Установить отправитель сигналов
    /// Установить отправитель сигналов.
    pub fn with_signal_sender(mut self, sender: Arc<dyn SignalSender>) -> Self {
        self.signal_sender = Some(sender);
        self
    }
    
    /// Установить порог для отправки сигналов
    /// Установить порог для отправки сигналов.
    /// Значение по умолчанию: 1e-6.
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold;
        self
    }
    
    /// Отправить сигнал об изменении параметра
    fn send_signal(&self, value: f64) {
        if let Some(sender) = &self.signal_sender {
            sender.send_parameter_changed(
                &self.target_node,
                &self.target_param,
                value as f32,
            );
        }
    }
}

impl Automaton for FunctionAutomaton {
    type Time = f64;
    type Context = AutomationContext;
    type Action = ();
    type State = FunctionState;
    
    fn step(
        &self,
        time: Self::Time,
        _context: &Self::Context,
        _action: Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<Self::Action>) {
        let value = {
            let mut guard = self.generator.lock().unwrap();
            guard(time)
        };
        
        // Отправляем сигнал если значение изменилось значительно
        if (value - state.value).abs() > self.threshold {
            self.send_signal(value);
        }
        
        let new_state = FunctionState::new(value, time);
        
        (new_state, None)
    }
    
    fn initial_state(&self) -> Self::State {
        let value = {
            let mut guard = self.generator.lock().unwrap();
            guard(0.0)
        };
        FunctionState::new(value, 0.0)
    }
    
    fn name(&self) -> &str {
        &self.name
    }
    
    fn extract_value(&self, state: &Self::State) -> f64 {
        state.value
    }
}

/// Автомат с состоянием на основе функции
pub struct StatefulFunctionAutomaton<S> {
    /// Имя автомата
    name: String,
    /// Генерирующая функция с состоянием
    generator: Arc<std::sync::Mutex<Box<dyn FnMut(f64, &mut S) -> f64 + Send + Sync>>>,
    /// Начальное состояние
    initial_state: S,
    /// ID целевого узла
    target_node: String,
    /// Имя целевого параметра
    target_param: String,
    /// Отправитель сигналов
    signal_sender: Option<Arc<dyn SignalSender>>,
    /// Порог для отправки сигналов
    threshold: f64,
}

impl<S: Send + Sync + Clone + 'static> StatefulFunctionAutomaton<S> {
    /// Создать новый функциональный автомат с состоянием
    pub fn new<F>(
        name: &str,
        mut generator: F,
        initial_state: S,
        target_node: &str,
        target_param: &str,
    ) -> Self
    where
        F: FnMut(f64, &mut S) -> f64 + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            generator: Arc::new(std::sync::Mutex::new(Box::new(generator))),
            initial_state,
            target_node: target_node.to_string(),
            target_param: target_param.to_string(),
            signal_sender: None,
            threshold: 1e-6,
        }
    }
    
    /// Установить отправитель сигналов
    /// Установить отправитель сигналов.
    pub fn with_signal_sender(mut self, sender: Arc<dyn SignalSender>) -> Self {
        self.signal_sender = Some(sender);
        self
    }
    
    /// Установить порог для отправки сигналов
    /// Установить порог для отправки сигналов.
    /// Значение по умолчанию: 1e-6.
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold;
        self
    }
    
    fn send_signal(&self, value: f64) {
        if let Some(sender) = &self.signal_sender {
            sender.send_parameter_changed(
                &self.target_node,
                &self.target_param,
                value as f32,
            );
        }
    }
}

impl<S: Send + Sync + Clone + 'static> Automaton for StatefulFunctionAutomaton<S> {
    type Time = f64;
    type Context = AutomationContext;
    type Action = ();
    type State = FunctionState;
    
    fn step(
        &self,
        time: Self::Time,
        _context: &Self::Context,
        _action: Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<Self::Action>) {
        // Получаем или создаём пользовательское состояние
        let mut user_state = if let Some(s) = state.get_user_state::<S>() {
            s.clone()
        } else {
            self.initial_state.clone()
        };
        
        let value = {
            let mut guard = self.generator.lock().unwrap();
            guard(time, &mut user_state)
        };
        
        // Отправляем сигнал если значение изменилось значительно
        if (value - state.value).abs() > self.threshold {
            self.send_signal(value);
        }
        
        let new_state = FunctionState::with_user_state(value, time, user_state);
        
        (new_state, None)
    }
    
    fn initial_state(&self) -> Self::State {
        let mut user_state = self.initial_state.clone();
        let value = {
            let mut guard = self.generator.lock().unwrap();
            guard(0.0, &mut user_state)
        };
        FunctionState::with_user_state(value, 0.0, user_state)
    }
    
    fn name(&self) -> &str {
        &self.name
    }
    
    fn extract_value(&self, state: &Self::State) -> f64 {
        state.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    
    #[derive(Debug)]
    struct TestSender {
        last_value: Arc<Mutex<Option<f32>>>,
    }
    
    impl TestSender {
        fn new() -> Self {
            Self {
                last_value: Arc::new(Mutex::new(None)),
            }
        }
    }
    
    impl SignalSender for TestSender {
        fn send_parameter_changed(&self, _node_id: &str, _param_id: &str, value: f32) {
            *self.last_value.lock().unwrap() = Some(value);
        }
    }
    
    #[test]
    fn test_function_automaton_basic() {
        let automaton = FunctionAutomaton::new(
            "Test",
            |time| time * 2.0,
            "node",
            "param",
        );
        
        let state = automaton.initial_state();
        assert_eq!(state.value, 0.0);
        
        let (new_state, _) = automaton.step(1.0, &AutomationContext::dummy(), (), &state);
        assert_eq!(new_state.value, 2.0);
    }
    
    #[test]
    fn test_function_automaton_signal() {
        let sender = Arc::new(TestSender::new());
        let last_value = sender.last_value.clone();
        
        let automaton = FunctionAutomaton::new(
            "Test",
            |time| time,
            "node",
            "param",
        ).with_signal_sender(sender);
        
        let state = automaton.initial_state();
        let (new_state, _) = automaton.step(1.0, &AutomationContext::dummy(), (), &state);
        
        assert_eq!(*last_value.lock().unwrap(), Some(1.0));
    }
    
    #[test]
    fn test_stateful_automaton() {
        let automaton = StatefulFunctionAutomaton::new(
            "Counter",
            |_time, counter| {
                *counter += 1;
                *counter as f64
            },
            0,
            "node",
            "param",
        );
        
        let state = automaton.initial_state();
        assert_eq!(state.value, 1.0);
        
        let (new_state, _) = automaton.step(1.0, &AutomationContext::dummy(), (), &state);
        assert_eq!(new_state.value, 2.0);
    }
}