//! Контекст выполнения автоматов

use std::sync::Arc;
use kama_core_traits::time::{TimeProvider, SystemClock};
use crate::parameter::ParameterMap;
use crate::signal::SignalSender;

/// Контекст выполнения автоматов
#[derive(Debug, Clone)]
pub struct AutomationContext {
    pub time: Arc<dyn TimeProvider>,
    pub parameters: Arc<ParameterMap>,
    pub signal_sender: Option<Arc<dyn SignalSender>>,
}

impl AutomationContext {
    pub fn new(time: Arc<dyn TimeProvider>) -> Self {
        Self {
            time,
            parameters: Arc::new(ParameterMap::new()),
            signal_sender: None,
        }
    }
    
    pub fn set_signal_sender(&mut self, sender: Arc<dyn SignalSender>) {
        self.signal_sender = Some(sender);
    }
    
    pub fn with_signal_sender(mut self, sender: Arc<dyn SignalSender>) -> Self {
        self.signal_sender = Some(sender);
        self
    }
    
    /// Создать контекст для тестов
    #[cfg(test)]
    pub fn dummy() -> Self {
        use kama_core_traits::time::SystemClock;
        
        let clock = Arc::new(SystemClock::new(44100.0, 120.0));
        Self::new(clock)
    }
}