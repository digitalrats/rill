//! Восприятие - как автоматы видят и слышат мир

use std::collections::HashMap;

/// Восприятие мира (то, что автоматы могут чувствовать)
pub struct Perception {
    /// Аудиовыходы узлов (что можно услышать)
    audio_outputs: HashMap<String, Vec<f32>>,
    
    /// Текущие значения параметров (что можно измерить)
    parameters: HashMap<String, f32>,
    
    /// Время
    time: crate::core::WorldTime,
}

impl Perception {
    pub fn new() -> Self {
        Self {
            audio_outputs: HashMap::new(),
            parameters: HashMap::new(),
            time: crate::core::WorldTime::new(),
        }
    }
    
    /// Услышать аудиовыход узла
    pub fn hear(&self, node_id: &str) -> Option<&[f32]> {
        self.audio_outputs.get(node_id).map(|v| v.as_slice())
    }
    
    /// Измерить значение параметра
    pub fn measure(&self, param_id: &str) -> Option<f32> {
        self.parameters.get(param_id).copied()
    }
    
    /// Получить текущее время
    pub fn time(&self) -> crate::core::WorldTime {
        self.time
    }
    
    /// Обновить аудиовыход (вызывается из AudioGraph)
    pub fn update_audio(&mut self, node_id: String, audio: Vec<f32>) {
        self.audio_outputs.insert(node_id, audio);
    }
    
    /// Обновить параметр (вызывается из AudioGraph)
    pub fn update_parameter(&mut self, param_id: String, value: f32) {
        self.parameters.insert(param_id, value);
    }
    
    /// Обновить время
    pub fn update_time(&mut self, delta: f64) {
        self.time.advance(delta);
    }
}