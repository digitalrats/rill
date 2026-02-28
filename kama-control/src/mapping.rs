//! Маппинг событий на параметры
//!
//! Этот модуль содержит чистую логику преобразования
//! событий контроллеров в изменения параметров.

use kama_core::traits::{ParameterId, PortId};
use std::fmt;
use std::sync::Arc;

/// Событие контроллера (абстрактное, не привязанное к конкретному источнику)
#[derive(Debug, Clone, PartialEq)]
pub enum ControlEvent {
    /// Кнопка (нажата/отпущена)
    Button {
        id: u32,
        pressed: bool,
    },
    
    /// Поворотная ручка (энкодер)
    Knob {
        id: u32,
        value: f32,      // 0.0 - 1.0
    },
    
    /// Фейдер (линейный ползунок)
    Fader {
        id: u32,
        value: f32,      // 0.0 - 1.0
    },
    
    /// Абстрактное непрерывное значение
    Continuous {
        id: u32,
        value: f32,      // 0.0 - 1.0
    },
    
    /// Пользовательское событие (для расширения)
    Custom {
        source: String,
        data: Vec<f32>,
    },
}

impl ControlEvent {
    /// Получить нормализованное значение (0.0-1.0), если применимо
    pub fn normalized_value(&self) -> Option<f32> {
        match self {
            ControlEvent::Knob { value, .. } => Some(*value),
            ControlEvent::Fader { value, .. } => Some(*value),
            ControlEvent::Continuous { value, .. } => Some(*value),
            ControlEvent::Button { pressed, .. } => Some(if *pressed { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
    
    /// Получить ID элемента управления
    pub fn id(&self) -> Option<u32> {
        match self {
            ControlEvent::Button { id, .. } => Some(*id),
            ControlEvent::Knob { id, .. } => Some(*id),
            ControlEvent::Fader { id, .. } => Some(*id),
            ControlEvent::Continuous { id, .. } => Some(*id),
            _ => None,
        }
    }
}

/// Паттерн для сопоставления событий
#[derive(Debug, Clone, PartialEq)]
pub enum EventPattern {
    /// Любая кнопка
    AnyButton,
    /// Кнопка с конкретным ID
    Button(u32),
    
    /// Любая ручка
    AnyKnob,
    /// Ручка с конкретным ID
    Knob(u32),
    
    /// Любой фейдер
    AnyFader,
    /// Фейдер с конкретным ID
    Fader(u32),
    
    /// Любое непрерывное значение
    AnyContinuous,
    /// Непрерывное значение с конкретным ID
    Continuous(u32),
    
    /// Любое событие
    Any,
    
    /// Пользовательский паттерн (по источнику)
    Custom(String),
}

impl fmt::Display for EventPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventPattern::AnyButton => write!(f, "AnyButton"),
            EventPattern::Button(id) => write!(f, "Button({})", id),
            EventPattern::AnyKnob => write!(f, "AnyKnob"),
            EventPattern::Knob(id) => write!(f, "Knob({})", id),
            EventPattern::AnyFader => write!(f, "AnyFader"),
            EventPattern::Fader(id) => write!(f, "Fader({})", id),
            EventPattern::AnyContinuous => write!(f, "AnyContinuous"),
            EventPattern::Continuous(id) => write!(f, "Continuous({})", id),
            EventPattern::Any => write!(f, "Any"),
            EventPattern::Custom(src) => write!(f, "Custom({})", src),
        }
    }
}

/// Целевой параметр
#[derive(Debug, Clone)]
pub struct Target {
    /// Порт, к которому относится параметр
    pub port: PortId,
    /// Идентификатор параметра
    pub parameter: ParameterId,
    /// Минимальное значение
    pub min: f32,
    /// Максимальное значение
    pub max: f32,
}

impl Target {
    pub fn new(port: PortId, parameter: ParameterId, min: f32, max: f32) -> Self {
        Self {
            port,
            parameter,
            min,
            max,
        }
    }
}

/// Тип преобразования значения
#[derive(Clone)]
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

impl fmt::Debug for Transform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    pub fn apply(&self, normalized: f32, min: f32, max: f32) -> f32 {
        let range = max - min;
        let normalized = normalized.clamp(0.0, 1.0);
        
        let mapped = match self {
            Transform::Linear => min + normalized * range,
            Transform::Exponential => min + normalized * normalized * range,
            Transform::Logarithmic => {
                if normalized <= 0.0 {
                    min
                } else {
                    min + (1.0 + normalized * 9.0).log10() * range
                }
            }
            Transform::Inverted => max - normalized * range,
            Transform::Custom(f) => min + f(normalized) * range,
        };
        
        mapped.clamp(min, max)
    }
}

/// Полное описание маппинга
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
    /// Создать новый маппинг
    pub fn new(
        pattern: EventPattern,
        target: Target,
        transform: Transform,
    ) -> Self {
        let name = format!("{} -> {}", pattern, target.parameter);
        Self {
            pattern,
            target,
            transform,
            name,
            enabled: true,
        }
    }
    
    /// Проверить, подходит ли событие под этот маппинг
    pub fn matches(&self, event: &ControlEvent) -> bool {
        if !self.enabled {
            return false;
        }
        
        match (&self.pattern, event) {
            (EventPattern::AnyButton, ControlEvent::Button { .. }) => true,
            (EventPattern::Button(id), ControlEvent::Button { id: eid, .. }) => *id == *eid,
            
            (EventPattern::AnyKnob, ControlEvent::Knob { .. }) => true,
            (EventPattern::Knob(id), ControlEvent::Knob { id: eid, .. }) => *id == *eid,
            
            (EventPattern::AnyFader, ControlEvent::Fader { .. }) => true,
            (EventPattern::Fader(id), ControlEvent::Fader { id: eid, .. }) => *id == *eid,
            
            (EventPattern::AnyContinuous, ControlEvent::Continuous { .. }) => true,
            (EventPattern::Continuous(id), ControlEvent::Continuous { id: eid, .. }) => *id == *eid,
            
            (EventPattern::Any, _) => true,
            
            (EventPattern::Custom(src), ControlEvent::Custom { source, .. }) => src == source,
            
            _ => false,
        }
    }
    
    /// Применить событие и получить значение параметра
    pub fn apply(&self, event: &ControlEvent) -> Option<f32> {
        if !self.matches(event) {
            return None;
        }
        
        event.normalized_value().map(|norm| {
            self.transform.apply(norm, self.target.min, self.target.max)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kama_core::traits::{NodeId, ParameterId};
    
    fn test_param(name: &str) -> ParameterId {
        ParameterId::new(name).unwrap()
    }
    
    #[test]
    fn test_transform_linear() {
        let t = Transform::Linear;
        assert_eq!(t.apply(0.0, 0.0, 10.0), 0.0);
        assert_eq!(t.apply(0.5, 0.0, 10.0), 5.0);
        assert_eq!(t.apply(1.0, 0.0, 10.0), 10.0);
        assert_eq!(t.apply(0.3, -5.0, 5.0), -2.0);
    }
    
    #[test]
    fn test_transform_exponential() {
        let t = Transform::Exponential;
        assert_eq!(t.apply(0.0, 0.0, 10.0), 0.0);
        assert_eq!(t.apply(0.5, 0.0, 10.0), 2.5); // 0.5^2 * 10 = 2.5
        assert_eq!(t.apply(1.0, 0.0, 10.0), 10.0);
    }
    
    #[test]
    fn test_transform_logarithmic() {
        let t = Transform::Logarithmic;
        assert_eq!(t.apply(0.0, 0.0, 10.0), 0.0);
        assert!((t.apply(0.5, 0.0, 10.0) - 6.9).abs() < 0.1);
        assert_eq!(t.apply(1.0, 0.0, 10.0), 10.0);
    }
    
    #[test]
    fn test_transform_inverted() {
        let t = Transform::Inverted;
        assert_eq!(t.apply(0.0, 0.0, 10.0), 10.0);
        assert_eq!(t.apply(0.5, 0.0, 10.0), 5.0);
        assert_eq!(t.apply(1.0, 0.0, 10.0), 0.0);
    }
    
    #[test]
    fn test_mapping_matches() {
        let node = NodeId(1);
        let port = PortId::control_in(node, 0);
        let param = test_param("gain");
        let target = Target::new(port, param, 0.0, 1.0);
        
        let mapping = Mapping::new(
            EventPattern::Knob(7),
            target,
            Transform::Linear,
        );
        
        assert!(mapping.matches(&ControlEvent::Knob { id: 7, value: 0.5 }));
        assert!(!mapping.matches(&ControlEvent::Knob { id: 8, value: 0.5 }));
        assert!(!mapping.matches(&ControlEvent::Fader { id: 7, value: 0.5 }));
    }
    
    #[test]
    fn test_mapping_apply() {
        let node = NodeId(1);
        let port = PortId::control_in(node, 0);
        let param = test_param("gain");
        let target = Target::new(port, param, 0.0, 2.0);
        
        let mapping = Mapping::new(
            EventPattern::Knob(7),
            target,
            Transform::Linear,
        );
        
        let event = ControlEvent::Knob { id: 7, value: 0.5 };
        let value = mapping.apply(&event).unwrap();
        assert_eq!(value, 1.0); // 0.5 * 2.0 = 1.0
    }
    
    #[test]
    fn test_mapping_with_range() {
        let node = NodeId(1);
        let port = PortId::control_in(node, 0);
        let param = test_param("filter");
        let target = Target::new(port, param, 100.0, 1000.0);
        
        let mapping = Mapping::new(
            EventPattern::Fader(1),
            target,
            Transform::Exponential,
        );
        
        let event = ControlEvent::Fader { id: 1, value: 0.5 };
        let value = mapping.apply(&event).unwrap();
        
        // 100 + 0.5^2 * 900 = 100 + 0.25 * 900 = 100 + 225 = 325
        assert!((value - 325.0).abs() < 0.1);
    }
}