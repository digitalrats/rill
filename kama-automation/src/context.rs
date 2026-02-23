//! # Контекст выполнения автоматов
//!
//! Контекст предоставляет автоматам доступ к глобальным ресурсам:
//! - **Время** — через [`TimeProvider`]
//! - **Параметры** — через [`ParameterMap`] (для чтения других параметров)
//! - **Сигналы** — через [`SignalSender`] для отправки уведомлений
//!
//! ## Важно
//! Контекст — это разделяемое состояние, поэтому он обёрнут в `Arc`
//! и клонируется при передаче между сервоприводами.
//!
//! [`TimeProvider`]: kama_core_traits::time::TimeProvider

//! Контекст выполнения автоматов

use crate::parameter::ParameterMap;
use crate::signal::SignalSender;
use kama_core_traits::time::{SystemClock, TimeProvider};
use std::sync::Arc;

/// Контекст выполнения автоматов
#[derive(Debug, Clone)]
pub struct AutomationContext {
    pub time: Arc<dyn TimeProvider>,
    pub parameters: Arc<ParameterMap>,
    pub signal_sender: Option<Arc<dyn SignalSender>>,
}

impl AutomationContext {
    /// Создать новый контекст с указанным источником времени.
    pub fn new(time: Arc<dyn TimeProvider>) -> Self {
        Self {
            time,
            parameters: Arc::new(ParameterMap::new()),
            signal_sender: None,
        }
    }

    /// Установить отправитель сигналов (мутабельно).
    pub fn set_signal_sender(&mut self, sender: Arc<dyn SignalSender>) {
        self.signal_sender = Some(sender);
    }

    /// Создать новый контекст с отправителем сигналов (builder-стиль).
    pub fn with_signal_sender(mut self, sender: Arc<dyn SignalSender>) -> Self {
        self.signal_sender = Some(sender);
        self
    }

    /// Создать контекст для тестов
    #[cfg(test)]
    /// Создать контекст для тестов с заглушками.
    /// Использует [`SystemClock`] с частотой 44.1 kHz и BPM=120.
    pub fn dummy() -> Self {
        use kama_core_traits::time::SystemClock;

        let clock = Arc::new(SystemClock::new(44100.0, 120.0));
        Self::new(clock)
    }
}
