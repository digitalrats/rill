//! # Интерфейс отправки сигналов
//!
//! Этот модуль определяет, как сервоприводы общаются с внешним миром.
//! Когда значение параметра изменяется, сервопривод вызывает
//! [`SignalSender::send_parameter_changed`], и внешний код (обычно аудиограф)
//! может отреагировать на это изменение.
//!
//! ## Две реализации
//!
//! 1. [`TestSignalSender`] — для тестирования. Сохраняет отправленные сигналы
//!    в вектор для последующей проверки.
//! 2. `KamaSignalSender` (в `integration.rs`) — реальная реализация, которая
//!    отправляет сигналы в `kama-signal` систему.
//!
//! ## Расширяемость
//!
//! Вы можете реализовать свой `SignalSender`, например, для отправки
//! сигналов по OSC или в GUI.

// kama-automation/src/signal.rs
//! Интерфейс для отправки сигналов

use std::sync::RwLock;

/// Интерфейс для отправки сигналов в kama-core систему
pub trait SignalSender: std::fmt::Debug + Send + Sync {
    fn send_parameter_changed(&self, node_id: &str, param_id: &str, value: f32);
}

/// Тестовая реализация SignalSender
#[derive(Debug)]
pub struct TestSignalSender {
    pub sent_signals: RwLock<Vec<(String, String, f32)>>,
}

impl TestSignalSender {
    /// Создать новый тестовый отправитель.
    pub fn new() -> Self {
        Self {
            sent_signals: RwLock::new(Vec::new()),
        }
    }

    /// Очистить все сохранённые сигналы.
    pub fn clear_signals(&self) {
        let mut signals = self.sent_signals.write().unwrap();
        signals.clear();
    }

    /// Получить количество отправленных сигналов.
    pub fn get_signals_count(&self) -> usize {
        let signals = self.sent_signals.read().unwrap();
        signals.len()
    }

    /// Получить все значения для конкретного параметра.
    pub fn get_signals_for_param(&self, node_id: &str, param_id: &str) -> Vec<f32> {
        let signals = self.sent_signals.read().unwrap();
        println!(
            "TestSignalSender - searching for {}:{} in {:?}",
            node_id, param_id, signals
        );
        signals
            .iter()
            .filter(|(n, p, _)| n == node_id && p == param_id)
            .map(|(_, _, v)| *v)
            .collect()
    }

    /// Получить все сохранённые сигналы.
    pub fn get_all_signals(&self) -> Vec<(String, String, f32)> {
        let signals = self.sent_signals.read().unwrap();
        signals.clone()
    }
}

impl SignalSender for TestSignalSender {
    fn send_parameter_changed(&self, node_id: &str, param_id: &str, value: f32) {
        println!(
            "TestSignalSender - RECEIVED: {}:{} = {}",
            node_id, param_id, value
        );
        let mut signals = self.sent_signals.write().unwrap();
        println!(
            "TestSignalSender - current signals before push: {:?}",
            *signals
        );
        signals.push((node_id.to_string(), param_id.to_string(), value));
        println!("TestSignalSender - signals after push: {:?}", *signals);
    }
}

impl Default for TestSignalSender {
    fn default() -> Self {
        Self::new()
    }
}
