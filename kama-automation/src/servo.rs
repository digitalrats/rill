//! Сервопривод для управления параметрами

use std::sync::Arc;
use crate::automaton::Automaton;
use crate::context::AutomationContext;
use crate::signal::SignalSender;

/// Тип маппинга значений
pub enum ParameterMapping {
    Linear,
    Exponential,
    Logarithmic,
    Custom(Box<dyn Fn(f64) -> f64 + Send + Sync>),
}

impl std::fmt::Debug for ParameterMapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParameterMapping::Linear => write!(f, "Linear"),
            ParameterMapping::Exponential => write!(f, "Exponential"),
            ParameterMapping::Logarithmic => write!(f, "Logarithmic"),
            ParameterMapping::Custom(_) => write!(f, "Custom"),
        }
    }
}

impl Clone for ParameterMapping {
    fn clone(&self) -> Self {
        match self {
            ParameterMapping::Linear => ParameterMapping::Linear,
            ParameterMapping::Exponential => ParameterMapping::Exponential,
            ParameterMapping::Logarithmic => ParameterMapping::Logarithmic,
            ParameterMapping::Custom(_) => ParameterMapping::Linear, // fallback
        }
    }
}

/// Сервопривод для управления параметром через автомат
pub struct Servo<A>
where
    A: Automaton<Time = f64, Context = AutomationContext>,
    A::Action: Default,
{
    pub id: String,
    pub automaton: Arc<A>,
    pub target_node: String,
    pub target_parameter: String,
    pub mapping: ParameterMapping,
    pub min_value: f64,
    pub max_value: f64,
    pub enabled: bool,
    
    pub(crate) state: A::State,
    pub(crate) last_value: f64,
    pub(crate) last_update_time: f64,
    pub(crate) context: AutomationContext,
}

impl<A> Servo<A>
where
    A: Automaton<Time = f64, Context = AutomationContext>,
    A::Action: Default,
{
    pub fn new(
        id: String,
        automaton: Arc<A>,
        target_node: String,
        target_parameter: String,
        mapping: ParameterMapping,
        context: AutomationContext,
    ) -> Self {
        Self {
            id,
            automaton: automaton.clone(),
            target_node,
            target_parameter,
            mapping,
            min_value: 0.0,
            max_value: 1.0,
            enabled: true,
            state: automaton.initial_state(),
            last_value: 0.0,
            last_update_time: context.time.position_seconds(),
            context,
        }
    }
    
// kama-automation/src/servo.rs

pub fn update(&mut self, _time: f64) -> Option<f64> {
    if !self.enabled {
        println!("Servo[{}] - disabled", self.id);
        return None;
    }
    
    let current_time = self.context.time.position_seconds();
    let delta_time = current_time - self.last_update_time;
    
    println!("Servo[{}] - current_time: {:.6}, last_update: {:.6}, delta: {:.6}", 
             self.id, current_time, self.last_update_time, delta_time);
    
    if delta_time <= 0.0 {
        println!("Servo[{}] - delta_time <= 0, skipping", self.id);
        return None;
    }
    
    println!("Servo[{}] - calling automaton.step()", self.id);
    let (new_state, _next_action) = self.automaton.step(
        current_time,
        &self.context,
        A::Action::default(),
        &self.state,
    );
    
    self.state = new_state;
    let raw_value = self.automaton.extract_value(&self.state);
    println!("Servo[{}] - raw_value: {:.6}", self.id, raw_value);
    
    let mapped_value = match &self.mapping {
        ParameterMapping::Linear => raw_value,
        ParameterMapping::Exponential => raw_value.exp(),
        ParameterMapping::Logarithmic => raw_value.abs().ln_1p(),
        ParameterMapping::Custom(func) => func(raw_value),
    };
    
    let clamped_value = mapped_value.clamp(self.min_value, self.max_value);
    println!("Servo[{}] - mapped_value: {:.6}, clamped: {:.6}", 
             self.id, mapped_value, clamped_value);
    
    if (clamped_value - self.last_value).abs() > 1e-6 {
        println!("Servo[{}] - sending value: {:.6}", self.id, clamped_value);
        
        if let Some(sender) = &self.context.signal_sender {
            println!("Servo[{}] - signal_sender exists, sending...", self.id);
            let sender_ref: &dyn SignalSender = sender.as_ref();
            sender_ref.send_parameter_changed(
                &self.target_node,
                &self.target_parameter,
                clamped_value as f32,
            );
            println!("Servo[{}] - send_parameter_changed called", self.id);
        } else {
            println!("Servo[{}] - WARNING: signal_sender is None", self.id);
        }
        self.last_value = clamped_value;
    } else {
        println!("Servo[{}] - value unchanged, skipping send", self.id);
    }
    
    self.last_update_time = current_time;
    Some(clamped_value)
}

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    pub fn set_range(&mut self, min: f64, max: f64) {
        self.min_value = min;
        self.max_value = max;
    }
    
    pub fn target(&self) -> (&str, &str) {
        (&self.target_node, &self.target_parameter)
    }
}

/// Трейт для типа-стирания сервоприводов
pub trait AnyServo: Send + Sync {
    fn update(&mut self, time: f64) -> Option<f64>;
    fn id(&self) -> &str;
    fn set_enabled(&mut self, enabled: bool);
    fn target(&self) -> (&str, &str);
}

impl<A> AnyServo for Servo<A>
where
    A: Automaton<Time = f64, Context = AutomationContext> + 'static,
    A::Action: Default + 'static,
{
    fn update(&mut self, time: f64) -> Option<f64> {
        Servo::update(self, time)
    }
    
    fn id(&self) -> &str {
        &self.id
    }
    
    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    fn target(&self) -> (&str, &str) {
        (&self.target_node, &self.target_parameter)
    }
}