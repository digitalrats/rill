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

use kama_core::signal::ParameterChanged;
use std::sync::RwLock;

/// Интерфейс для отправки сигналов в kama-core систему
pub trait SignalSender: std::fmt::Debug + Send + Sync {
    fn send_parameter_changed(&self, signal: ParameterChanged);
}

/// Тестовая реализация SignalSender
#[derive(Debug)]
pub struct TestSignalSender {
    pub sent_signals: RwLock<Vec<ParameterChanged>>,
}

impl TestSignalSender {
    pub fn new() -> Self {
        Self {
            sent_signals: RwLock::new(Vec::new()),
        }
    }

    pub fn clear_signals(&self) {
        self.sent_signals.write().unwrap().clear();
    }

    pub fn get_signals_count(&self) -> usize {
        self.sent_signals.read().unwrap().len()
    }

    pub fn get_signals_for_port(&self, port: PortId) -> Vec<ParameterChanged> {
        self.sent_signals
            .read()
            .unwrap()
            .iter()
            .filter(|s| s.port == port)
            .cloned()
            .collect()
    }

    pub fn get_all_signals(&self) -> Vec<ParameterChanged> {
        self.sent_signals.read().unwrap().clone()
    }
}

impl SignalSender for TestSignalSender {
    fn send_parameter_changed(&self, signal: ParameterChanged) {
        self.sent_signals.write().unwrap().push(signal);
    }
}

impl Default for TestSignalSender {
    fn default() -> Self {
        Self::new()
    }
}