//! Базовые типы для маппинга

use kama_core::traits::{ParameterId, PortId};
use std::sync::Arc;

/// Тип преобразования значения
#[derive(Debug, Clone)]
pub enum Transform {
    /// Линейное: y = x
    Linear,
    
    /// Экспоненциальное: y = x²
    Exponential,
    
    /// Логарифмическое: y = log10(1 + 9x)
    Logarithmic,
    
    /// Инвертированное: y = 1 - x
    Inverted,
    
    /// Масштабирование: y = x * scale + offset
    Scale { scale: f32, offset: f32 },
    
    /// Порог: y = 1 if x > threshold else 0
    Threshold { level: f32, hysteresis: f32 },
    
    /// Сглаживание (экспоненциальное)
    Smooth { coefficient: f32 },
    
    /// RMS (для аудио)
    Rms { window_size: usize },
    
    /// Пиковый детектор
    Peak { decay: f32 },
    
    /// Envelope follower
    Envelope { attack: f32, release: f32 },
    
    /// Частота (zero-crossing)
    Frequency { min_freq: f32, max_freq: f32 },
    
    /// Пользовательская функция
    Custom(Arc<dyn Fn(f32) -> f32 + Send + Sync>),
}

impl Transform {
    /// Применить преобразование к нормализованному значению (0-1)
    pub fn apply(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        
        match self {
            Transform::Linear => x,
            Transform::Exponential => x * x,
            Transform::Logarithmic => {
                if x <= 0.0 { 0.0 } else { (1.0 + 9.0 * x).log10() }
            }
            Transform::Inverted => 1.0 - x,
            Transform::Scale { scale, offset } => x * scale + offset,
            Transform::Threshold { level, hysteresis } => {
                static mut STATE: bool = false;
                unsafe {
                    if x > *level + hysteresis {
                        STATE = true;
                        1.0
                    } else if x < *level - hysteresis {
                        STATE = false;
                        0.0
                    } else if STATE {
                        1.0
                    } else {
                        0.0
                    }
                }
            }
            Transform::Smooth { coefficient } => {
                static mut LAST: f32 = 0.0;
                unsafe {
                    LAST = LAST * (1.0 - coefficient) + x * coefficient;
                    LAST
                }
            }
            _ => x, // Для остальных пока заглушка
        }
    }
}

/// Правило маппинга
#[derive(Debug, Clone)]
pub struct MappingRule {
    /// Имя входного сигнала (для мира автоматов)
    pub input_name: String,
    
    /// Индекс входного канала (для аудиографа)
    pub input_channel: usize,
    
    /// Преобразование
    pub transform: Transform,
    
    /// Имя выходного сигнала (для мира автоматов)
    pub output_name: String,
    
    /// Целевой порт (для прямого управления в аудиографе)
    pub target_port: Option<PortId>,
    
    /// Целевой параметр
    pub target_parameter: Option<ParameterId>,
    
    /// Диапазон выходных значений
    pub output_range: (f32, f32),
}

impl MappingRule {
    /// Создать новое правило
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
    
    /// Установить преобразование
    pub fn with_transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }
    
    /// Установить диапазон выхода
    pub fn with_range(mut self, min: f32, max: f32) -> Self {
        self.output_range = (min, max);
        self
    }
    
    /// Установить входной канал (для аудио)
    pub fn with_channel(mut self, channel: usize) -> Self {
        self.input_channel = channel;
        self
    }
    
    /// Установить целевой параметр (для микро-контроля)
    pub fn with_target(mut self, port: PortId, parameter: ParameterId) -> Self {
        self.target_port = Some(port);
        self.target_parameter = Some(parameter);
        self
    }
    
    /// Применить правило к значению
    pub fn apply(&self, x: f32) -> f32 {
        let transformed = self.transform.apply(x);
        self.output_range.0 + transformed * (self.output_range.1 - self.output_range.0)
    }
}