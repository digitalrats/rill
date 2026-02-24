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

use crate::automaton::Automaton;
use crate::context::AutomationContext;
use crate::signal::SignalSender;
use kama_core::signal::ParameterChanged;
use kama_core::traits::{NodeId, ParameterId, PortId, SignalSource};
use std::sync::Arc;

/// Тип маппинга значений
#[derive(Debug, Clone)]
pub enum ParameterMapping {
    Linear,
    Exponential,
    Logarithmic,
    Inverted,
    Custom(Arc<dyn Fn(f64) -> f64 + Send + Sync>),
}

impl ParameterMapping {
    pub fn apply(&self, raw: f64) -> f64 {
        match self {
            ParameterMapping::Linear => raw,
            ParameterMapping::Exponential => raw * raw,
            ParameterMapping::Logarithmic => (1.0 + raw * 9.0).log10(),
            ParameterMapping::Inverted => 1.0 - raw,
            ParameterMapping::Custom(f) => f(raw),
        }
    }
}

/// Сервопривод — связующее звено между автоматом и параметром порта
pub struct Servo<A>
where
    A: Automaton<Time = f64, Context = AutomationContext>,
    A::Action: Default,
{
    pub id: String,
    pub automaton: Arc<A>,
    pub target_port: PortId,
    pub target_parameter: ParameterId,
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
        target_port: PortId,
        target_parameter: ParameterId,
        mapping: ParameterMapping,
        context: AutomationContext,
    ) -> Self {
        Self {
            id,
            automaton: automaton.clone(),
            target_port,
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

    pub fn update(&mut self, current_time: f64) -> Option<ParameterChanged> {
        if !self.enabled {
            return None;
        }

        let delta_time = current_time - self.last_update_time;
        if delta_time <= 0.0 {
            return None;
        }

        let (new_state, _next_action) = self.automaton.step(
            current_time,
            &self.context,
            A::Action::default(),
            &self.state,
        );

        self.state = new_state;
        let raw_value = self.automaton.extract_value(&self.state);

        let mapped_value = self.mapping.apply(raw_value);
        let clamped_value = mapped_value.clamp(self.min_value, self.max_value);

        if (clamped_value - self.last_value).abs() > 1e-6 {
            self.last_value = clamped_value;

            let signal = ParameterChanged::new(
                self.target_port,
                self.target_parameter.clone(),
                clamped_value as f32,
                (clamped_value - self.min_value) / (self.max_value - self.min_value) as f32,
                SignalSource::Automation,
            );

            if let Some(sender) = &self.context.signal_sender {
                sender.send_parameter_changed(signal.clone());
            }

            self.last_update_time = current_time;
            Some(signal)
        } else {
            self.last_update_time = current_time;
            None
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn set_range(&mut self, min: f64, max: f64) {
        self.min_value = min;
        self.max_value = max;
    }

    pub fn target(&self) -> PortId {
        self.target_port
    }
}

pub trait AnyServo: Send + Sync {
    fn update(&mut self, time: f64) -> Option<ParameterChanged>;
    fn id(&self) -> &str;
    fn set_enabled(&mut self, enabled: bool);
    fn target(&self) -> PortId;
}

impl<A> AnyServo for Servo<A>
where
    A: Automaton<Time = f64, Context = AutomationContext> + 'static,
    A::Action: Default + 'static,
{
    fn update(&mut self, time: f64) -> Option<ParameterChanged> {
        Servo::update(self, time)
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn target(&self) -> PortId {
        self.target_port
    }
}