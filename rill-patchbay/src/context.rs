//! Контекст выполнения автоматов

use std::sync::Arc;
use rill_core::time::TimeProvider;
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
    
    pub fn with_signal_sender(mut self, sender: Arc<dyn SignalSender>) -> Self {
        self.signal_sender = Some(sender);
        self
    }
}