use std::sync::Arc;
use kama_core::graph::NodeId;
use std::fmt;

/// Паттерн для сопоставления событий
#[derive(Debug, Clone, PartialEq)]
pub enum EventPattern {
    /// Любая кнопка
    AnyButton,
    /// Кнопка с конкретным ID
    ButtonId(u32),
    
    /// Любая ручка
    AnyKnob,
    /// Ручка с конкретным ID
    KnobId(u32),
    
    /// Любой фейдер
    AnyFader,
    /// Фейдер с конкретным ID
    FaderId(u32),
    
    /// Любое MIDI сообщение
    AnyMidi,
    /// MIDI Control Change
    MidiControl {
        channel: Option<u8>,
        controller: u8,
    },
    /// MIDI Note
    MidiNote {
        channel: Option<u8>,
        note: Option<u8>,
    },
    
    /// OSC сообщение по адресу
    OscAddress(String),
    
    /// OSC с паттерном (содержит)
    OscPattern(String),
}

impl fmt::Display for EventPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventPattern::AnyButton => write!(f, "AnyButton"),
            EventPattern::ButtonId(id) => write!(f, "Button({})", id),
            EventPattern::AnyKnob => write!(f, "AnyKnob"),
            EventPattern::KnobId(id) => write!(f, "Knob({})", id),
            EventPattern::AnyFader => write!(f, "AnyFader"),
            EventPattern::FaderId(id) => write!(f, "Fader({})", id),
            EventPattern::AnyMidi => write!(f, "AnyMidi"),
            EventPattern::MidiControl { channel, controller } => {
                if let Some(ch) = channel {
                    write!(f, "MidiControl(ch:{}, cc:{})", ch, controller)
                } else {
                    write!(f, "MidiControl(cc:{})", controller)
                }
            }
            EventPattern::MidiNote { channel, note } => {
                match (channel, note) {
                    (Some(ch), Some(n)) => write!(f, "MidiNote(ch:{}, note:{})", ch, n),
                    (Some(ch), None) => write!(f, "MidiNote(ch:{})", ch),
                    (None, Some(n)) => write!(f, "MidiNote(note:{})", n),
                    (None, None) => write!(f, "MidiNote"),
                }
            }
            EventPattern::OscAddress(addr) => write!(f, "OSC({})", addr),
            EventPattern::OscPattern(pat) => write!(f, "OSC Pattern({})", pat),
        }
    }
}

/// Целевой параметр
#[derive(Debug, Clone)]
pub struct Target {
    /// ID узла в графе
    pub node_id: NodeId,
    /// Имя параметра
    pub param_name: String,
    /// Минимальное значение
    pub min: f32,
    /// Максимальное значение
    pub max: f32,
}

/// Тип преобразования
#[derive(Clone)]  // Убрали Debug
pub enum Transform {
    /// Линейное: out = min + value * (max - min)
    Linear,
    
    /// Экспоненциальное: out = min + value^2 * (max - min)
    Exponential,
    
    /// Логарифмическое: out = min + log(1 + value * 9) / log(10) * (max - min)
    Logarithmic,
    
    /// Инвертированное: out = max - value * (max - min)
    Inverted,
    
    /// Пользовательское
    Custom(Arc<dyn Fn(f32) -> f32 + Send + Sync>),
}

impl std::fmt::Debug for Transform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transform::Linear => write!(f, "Linear"),
            Transform::Exponential => write!(f, "Exponential"),
            Transform::Logarithmic => write!(f, "Logarithmic"),
            Transform::Inverted => write!(f, "Inverted"),
            Transform::Custom(_) => write!(f, "Custom"),
        }
    }
}

impl Transform {
    /// Применить преобразование к нормализованному значению (0-1)
    pub fn apply(&self, value: f32, min: f32, max: f32) -> f32 {
        let range = max - min;
        let normalized = value.clamp(0.0, 1.0);
        
        let mapped = match self {
            Transform::Linear => min + normalized * range,
            Transform::Exponential => min + normalized * normalized * range,
            Transform::Logarithmic => min + (1.0 + normalized * 9.0).log10() * range,
            Transform::Inverted => max - normalized * range,
            Transform::Custom(f) => min + f(normalized) * range,
        };
        
        mapped.clamp(min, max)
    }
}

/// Маппинг события на параметр
#[derive(Debug, Clone)]
pub struct Mapping {
    /// Паттерн события
    pub pattern: EventPattern,
    /// Целевой параметр
    pub target: Target,
    /// Преобразование
    pub transform: Transform,
    /// Название (для отладки)
    pub name: String,
    /// Активен ли маппинг
    pub enabled: bool,
}

impl Mapping {
    pub fn new(pattern: EventPattern, target: Target, transform: Transform) -> Self {
        let name = format!("{} -> {}", pattern, target.param_name);
        Self {
            pattern,
            target,
            transform,
            name,
            enabled: true,
        }
    }
    
    /// Проверить, подходит ли событие под этот маппинг
    pub fn matches(&self, event: &crate::backend::ControlEvent) -> bool {
        if !self.enabled {
            return false;
        }
        
        match (&self.pattern, event) {
            (EventPattern::AnyButton, crate::backend::ControlEvent::Button { .. }) => true,
            (EventPattern::ButtonId(id), crate::backend::ControlEvent::Button { id: eid, .. }) => *id == *eid,
            
            (EventPattern::AnyKnob, crate::backend::ControlEvent::Knob { .. }) => true,
            (EventPattern::KnobId(id), crate::backend::ControlEvent::Knob { id: eid, .. }) => *id == *eid,
            
            (EventPattern::AnyFader, crate::backend::ControlEvent::Fader { .. }) => true,
            (EventPattern::FaderId(id), crate::backend::ControlEvent::Fader { id: eid, .. }) => *id == *eid,
            
            (EventPattern::MidiControl { channel, controller }, 
             crate::backend::ControlEvent::MidiControl { channel: ech, controller: ectr, .. }) => {
                (channel.is_none() || channel.unwrap() == *ech) && *controller == *ectr
            }
            
            (EventPattern::OscAddress(addr), crate::backend::ControlEvent::Osc { address, .. }) => addr == address,
            
            (EventPattern::OscPattern(pat), crate::backend::ControlEvent::Osc { address, .. }) => {
                address.contains(pat)
            }
            
            _ => false,
        }
    }
    
    /// Применить событие и получить значение параметра
    pub fn apply(&self, event: &crate::backend::ControlEvent) -> Option<f32> {
        if !self.matches(event) {
            return None;
        }
        
        event.normalized_value().map(|v| {
            self.transform.apply(v, self.target.min, self.target.max)
        })
    }
}