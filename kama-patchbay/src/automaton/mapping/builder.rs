//! Строитель для удобного создания правил маппинга

use super::core::{MappingRule, Transform};
use kama_core::traits::{ParameterId, PortId};

/// Строитель для создания правил маппинга
pub struct MappingRuleBuilder {
    input_name: String,
    input_channel: usize,
    transform: Transform,
    output_name: String,
    target_port: Option<PortId>,
    target_parameter: Option<ParameterId>,
    output_range: (f32, f32),
}

impl MappingRuleBuilder {
    /// Создать новый строитель
    pub fn new(input_name: impl Into<String>, output_name: impl Into<String>) -> Self {
        Self {
            input_name: input_name.into(),
            input_channel: 0,
            transform: Transform::Linear,
            output_name: output_name.into(),
            target_port: None,
            target_parameter: None,
            output_range: (0.0, 1.0),
        }
    }
    
    /// Линейное преобразование
    pub fn linear(mut self) -> Self {
        self.transform = Transform::Linear;
        self
    }
    
    /// Экспоненциальное (быстрый старт)
    pub fn exponential(mut self) -> Self {
        self.transform = Transform::Exponential;
        self
    }
    
    /// Логарифмическое (медленный старт)
    pub fn logarithmic(mut self) -> Self {
        self.transform = Transform::Logarithmic;
        self
    }
    
    /// Инвертированное
    pub fn inverted(mut self) -> Self {
        self.transform = Transform::Inverted;
        self
    }
    
    /// Масштабирование
    pub fn scaled(mut self, scale: f32, offset: f32) -> Self {
        self.transform = Transform::Scale { scale, offset };
        self
    }
    
    /// Пороговое (gate)
    pub fn threshold(mut self, level: f32, hysteresis: f32) -> Self {
        self.transform = Transform::Threshold { level, hysteresis };
        self
    }
    
    /// Сглаживание
    pub fn smooth(mut self, coefficient: f32) -> Self {
        self.transform = Transform::Smooth { coefficient };
        self
    }
    
    /// RMS (для аудио)
    pub fn rms(mut self, window_size: usize) -> Self {
        self.transform = Transform::Rms { window_size };
        self
    }
    
    /// Пиковый детектор
    pub fn peak(mut self, decay: f32) -> Self {
        self.transform = Transform::Peak { decay };
        self
    }
    
    /// Envelope follower
    pub fn envelope(mut self, attack: f32, release: f32) -> Self {
        self.transform = Transform::Envelope { attack, release };
        self
    }
    
    /// Частотный детектор
    pub fn frequency(mut self, min_freq: f32, max_freq: f32) -> Self {
        self.transform = Transform::Frequency { min_freq, max_freq };
        self
    }
    
    /// Установить входной канал (для аудио)
    pub fn channel(mut self, channel: usize) -> Self {
        self.input_channel = channel;
        self
    }
    
    /// Установить диапазон выхода
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.output_range = (min, max);
        self
    }
    
    /// Установить целевой параметр (для микро-контроля)
    pub fn target(mut self, port: PortId, parameter: ParameterId) -> Self {
        self.target_port = Some(port);
        self.target_parameter = Some(parameter);
        self
    }
    
    /// Построить правило
    pub fn build(self) -> MappingRule {
        MappingRule {
            input_name: self.input_name,
            input_channel: self.input_channel,
            transform: self.transform,
            output_name: self.output_name,
            target_port: self.target_port,
            target_parameter: self.target_parameter,
            output_range: self.output_range,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kama_core::traits::{NodeId, PortId};

    #[test]
    fn test_builder_linear() {
        let rule = MappingRuleBuilder::new("knob", "param")
            .linear()
            .range(0.0, 10.0)
            .build();

        assert!(matches!(rule.transform, Transform::Linear));
        assert_eq!(rule.output_range, (0.0, 10.0));
    }

    #[test]
    fn test_builder_logarithmic() {
        let rule = MappingRuleBuilder::new("knob", "filter")
            .logarithmic()
            .range(20.0, 20000.0)
            .build();

        assert!(matches!(rule.transform, Transform::Logarithmic));
    }

    #[test]
    fn test_builder_with_target() {
        let port = PortId::control_in(NodeId(1), 0);
        let param = ParameterId::new("cutoff").unwrap();

        let rule = MappingRuleBuilder::new("knob", "filter")
            .linear()
            .range(0.0, 1.0)
            .target(port, param.clone())
            .build();

        assert_eq!(rule.target_port, Some(port));
        assert_eq!(rule.target_parameter, Some(param));
    }

    #[test]
    fn test_builder_complex() {
        let rule = MappingRuleBuilder::new("audio_in", "envelope_out")
            .envelope(0.01, 0.1)
            .range(0.0, 5.0)
            .channel(0)
            .build();

        assert!(matches!(rule.transform, Transform::Envelope { .. }));
        assert_eq!(rule.input_channel, 0);
        assert_eq!(rule.output_range, (0.0, 5.0));
    }
}