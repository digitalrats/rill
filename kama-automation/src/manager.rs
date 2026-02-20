// kama-automation/src/manager.rs
//! Менеджер автоматизации

use std::collections::HashMap;
use std::sync::Arc;
use kama_core_traits::time::{Clock, TimeProvider, SystemClock}; 
use crate::automaton::{LfoAutomaton};
use crate::context::AutomationContext;
use crate::servo::{Servo, AnyServo, ParameterMapping};
use crate::signal::SignalSender;

/// Менеджер автоматизации с обобщённым типом часов
pub struct AutomationManager<C: Clock> {
    pub(crate) servos: HashMap<String, Box<dyn AnyServo>>,
    pub(crate) context: AutomationContext,
    pub(crate) clock: C,
    time_provider: Arc<dyn TimeProvider>,
}

impl<C: Clock> AutomationManager<C> {
    pub fn new(time_provider: Arc<dyn TimeProvider>, clock: C) -> Self {
        Self {
            servos: HashMap::new(),
            context: AutomationContext::new(time_provider.clone()),
            clock,
            time_provider,
        }
    }
    
    pub fn with_signal_sender(mut self, sender: Arc<dyn SignalSender>) -> Self {
        self.context = self.context.with_signal_sender(sender);
        self
    }
    
    pub fn add_lfo(
        &mut self,
        id: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        target_node: &str,
        target_parameter: &str,
    ) {
        let automaton = Arc::new(LfoAutomaton::new(frequency, amplitude, offset)
            .with_envelope(0.01, 0.01));
        
        // Создаём контекст с тем же signal_sender, что и у менеджера
        let mut context = AutomationContext::new(self.time_provider.clone());
        if let Some(sender) = &self.context.signal_sender {
            context = context.with_signal_sender(sender.clone());
        }
        
        let servo = Servo::new(
            id.to_string(),
            automaton,
            target_node.to_string(),
            target_parameter.to_string(),
            ParameterMapping::Linear,
            context, // используем контекст с signal_sender
        );
        
        self.add_servo(servo);
    }
    
    // Также нужно исправить add_servo, чтобы он обновлял контекст с signal_sender
    pub fn add_servo<A>(&mut self, mut servo: Servo<A>) 
    where
        A: crate::automaton::Automaton<Time = f64, Context = AutomationContext> + 'static,
        A::Action: Default + 'static,
    {
        // Создаём новый контекст с правильным signal_sender
        let mut context = AutomationContext::new(self.time_provider.clone());
        if let Some(sender) = &self.context.signal_sender {
            context = context.with_signal_sender(sender.clone());
        }
        servo.context = context;
        
        self.servos.insert(servo.id.clone(), Box::new(servo));
    }
    
    pub fn update(&mut self, sample_count: usize) {
        // Приводим usize к u64
        let samples = sample_count as u64;
        let new_position = self.clock.advance(samples);
        
        // Вычисляем текущее время в секундах
        let current_time = new_position as f64 / self.clock.sample_rate();
        
        for servo in self.servos.values_mut() {
            let _ = servo.update(current_time);
        }
    }
    
    pub fn set_signal_sender(&mut self, sender: Arc<dyn SignalSender>) {
        self.context.set_signal_sender(sender); // Используем мутабельную версию
    }
    
    pub fn get_servo(&self, id: &str) -> Option<&dyn AnyServo> {
        self.servos.get(id).map(|b| b.as_ref())
    }
    
    pub fn get_servo_mut(&mut self, id: &str) -> Option<&mut Box<dyn AnyServo>> {
        self.servos.get_mut(id)
    }
    
    pub fn remove_servo(&mut self, id: &str) -> bool {
        self.servos.remove(id).is_some()
    }
    
    pub fn clear(&mut self) {
        self.servos.clear();
        self.clock.reset();
    }
    
    pub fn servos(&self) -> &HashMap<String, Box<dyn AnyServo>> {
        &self.servos
    }
    
    pub fn context(&self) -> &AutomationContext {
        &self.context
    }
}

// Type alias для удобства использования с SystemClock
pub type DefaultAutomationManager = AutomationManager<SystemClock>;

impl DefaultAutomationManager {
    pub fn new_default(time_provider: Arc<dyn TimeProvider>) -> Self {
        let clock = kama_core_traits::time::SystemClock::new(
            time_provider.sample_rate(), 
            120.0
        );
        AutomationManager::new(time_provider, clock)
    }
}