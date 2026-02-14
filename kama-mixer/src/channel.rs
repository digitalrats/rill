use serde::{Serialize, Deserialize};
use crate::filter::FilterConfig;
use crate::bus::SendConfig;

/// Тип канала микшера
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ChannelType {
    Mono,      // Моно канал
    Stereo,    // Стерео канал (L/R)
    DualMono,  // Два независимых моно канала
}

/// Конфигурация канала
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub id: usize,
    pub name: String,
    pub channel_type: ChannelType,
    pub level: f64,           // 0.0 - 1.0
    pub pan: f64,             // -1.0 (L) до 1.0 (R)
    pub mute: bool,
    pub solo: bool,
    pub filters: Vec<FilterConfig>,
    pub sends: Vec<SendConfig>,
}

/// Состояние канала
#[derive(Debug, Clone)]
pub struct ChannelState {
    pub config: ChannelConfig,
    pub meter_level: (f64, f64), // Пиковый уровень для VU метра
    pub last_level: f64,
}

impl ChannelState {
    pub fn new(config: ChannelConfig, _sample_rate: f64, _channel_index: usize) -> Self {
        let last_level = config.level;
        
        Self {
            config,
            meter_level: (0.0, 0.0),
            last_level,
        }
    }
    
    pub fn process(&mut self, input: f64) -> (f64, f64) {
        if self.config.mute {
            return (0.0, 0.0);
        }
        
        // Моно в стерео с панорамой
        let (left, right) = self.mono_to_stereo(input, self.config.pan);
        
        // Применяем уровень с плавным изменением
        let smoothed_level = self.last_level + (self.config.level - self.last_level) * 0.1;
        self.last_level = self.config.level;
        
        let left_out = left * smoothed_level;
        let right_out = right * smoothed_level;
        
        // Обновляем meter
        self.meter_level.0 = self.meter_level.0.max(left_out.abs());
        self.meter_level.1 = self.meter_level.1.max(right_out.abs());
        
        (left_out, right_out)
    }
    
    fn mono_to_stereo(&self, input: f64, pan: f64) -> (f64, f64) {
        let pan = pan.clamp(-1.0, 1.0);
        let left_gain = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
        let right_gain = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
        
        (input * left_gain, input * right_gain)
    }
    
    pub fn reset_meter(&mut self) {
        self.meter_level = (0.0, 0.0);
    }
}