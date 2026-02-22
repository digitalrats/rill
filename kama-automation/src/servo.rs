//! Сервопривод для управления параметрами
//! 
//! Сервопривод получает сигнал от автомата, преобразует его согласно
//! заданному маппингу и применяет к конкретному параметру узла.
//! 
//! ## Что делает сервопривод?
//! 
//! 1. **Получает сигнал** от автомата (LFO, огибающей и т.д.)
//! 2. **Трансформирует** его согласно маппингу (линейно, экспоненциально)
//! 3. **Применяет** к конкретному параметру узла
//! 4. **Отправляет сигнал** об изменении через `SignalSender`
//! 
//! ## Жизненный цикл
//! 
//! 1. Создаётся с автоматом, целью и контекстом
//! 2. Добавляется в [`AutomationManager`](crate::AutomationManager)
//! 3. Менеджер регулярно вызывает [`update`](Servo::update)
//! 4. Серво получает от автомата новое значение, применяет маппинг и,
//!    если значение изменилось, отправляет сигнал

use std::sync::Arc;
use crate::automaton::Automaton;
use crate::context::AutomationContext;
use crate::signal::SignalSender;

/// Тип маппинга значений — как преобразовать сигнал от автомата
/// в значение, подходящее для целевого параметра.
///
/// Автомат генерирует значения в некотором условном диапазоне (обычно -1..1 или 0..1),
/// а маппинг приводит их к физическому диапазону параметра (например, 20..20000 Гц).
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

/// Сервопривод — связующее звено между автоматом и параметром узла.
///
/// # Жизненный цикл
/// 1. Создаётся с автоматом, целью и контекстом.
/// 2. Добавляется в [`AutomationManager`](crate::AutomationManager).
/// 3. Менеджер регулярно вызывает [`update`](Servo::update), передавая текущее время.
/// 4. Серво получает от автомата новое значение, применяет маппинг и,
///    если значение изменилось, отправляет сигнал.
///
/// # Сглаживание
/// Серво не выполняет сглаживание само — это задача автомата или параметра узла.
/// Однако он хранит `last_value`, чтобы не отправлять одинаковые сигналы.
///
/// # Безопасность
/// Все методы сервопривода могут вызываться из любого потока,
/// так как внутреннее состояние защищено типом `Servo<A>`, а автомат
/// требует `Send + Sync`.
pub struct Servo<A>
where
    A: Automaton<Time = f64, Context = AutomationContext>,
    A::Action: Default,
{
    /// Уникальное имя сервопривода (для идентификации в менеджере).
    pub id: String,
    /// Автомат, генерирующий сигнал.
    pub automaton: Arc<A>,
    /// ID целевого узла в аудиографе.
    pub target_node: String,
    /// Имя целевого параметра.
    pub target_parameter: String,
    /// Преобразование значения.
    pub mapping: ParameterMapping,
    /// Минимальное значение параметра (после маппинга).
    pub min_value: f64,
    /// Максимальное значение параметра.
    pub max_value: f64,
    /// Включён ли сервопривод.
    pub enabled: bool,
    
    /// Текущее состояние автомата.
    pub(crate) state: A::State,
    /// Последнее отправленное значение.
    pub(crate) last_value: f64,
    /// Время последнего обновления.
    pub(crate) last_update_time: f64,
    /// Контекст выполнения.
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

    /// Включить или выключить сервопривод.
    /// Если выключен, значения не генерируются и не отправляются.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    /// Установить диапазон выходных значений.
    /// После маппинга значение будет ограничено этим диапазоном.
    pub fn set_range(&mut self, min: f64, max: f64) {
        self.min_value = min;
        self.max_value = max;
    }
    
    /// Получить целевой узел и параметр.
    pub fn target(&self) -> (&str, &str) {
        (&self.target_node, &self.target_parameter)
    }
}

/// # Пример\
/// ```\
/// # use kama_automation::{AnyServo, Servo, automaton::LfoAutomaton, AutomationContext, ParameterMapping};\
/// # use std::sync::Arc;\
/// # let context = AutomationContext::dummy();\
/// # let lfo = Arc::new(LfoAutomaton::lfo(1.0, 0.5, 0.0, "test", "param"));\
/// # let servo = Servo::new("test".to_string(), lfo, "node".to_string(), "param".to_string(), ParameterMapping::Linear, context);\
/// let servos: Vec<Box<dyn AnyServo>> = vec![\
///     Box::new(servo),  // Любой сервопривод, реализующий AnyServo\
/// ];\
/// ```
/// ```
pub trait AnyServo: Send + Sync {
    /// Обновить сервопривод. Возвращает новое значение, если оно изменилось.
    fn update(&mut self, time: f64) -> Option<f64>;
    /// Получить идентификатор.
    fn id(&self) -> &str;
    /// Включить/выключить.
    fn set_enabled(&mut self, enabled: bool);
    /// Получить цель (узел, параметр).
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