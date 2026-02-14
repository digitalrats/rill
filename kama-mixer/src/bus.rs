//! Шины и маршрутизация для микшера

use kama_core::mixer::basic::*;
use crate::filters::FilterConfig;

/// Тип посыла
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SendType {
    PreFader,
    PostFader,
}

/// Конфигурация посыла
#[derive(Debug, Clone)]
pub struct SendConfig {
    pub to_bus: usize,
    pub level: f64,
    pub send_type: SendType,
}

/// Конфигурация шины
#[derive(Debug, Clone)]
pub struct BusConfig {
    pub id: usize,
    pub name: String,
    pub level: f64,
    pub filters: Vec<FilterConfig>,
}

/// Состояние шины
#[derive(Debug, Clone)]
pub struct BusState {
    pub config: BusConfig,
    pub mix_buffer: Vec<f64>,
}

impl BusState {
    pub fn new(config: BusConfig, buffer_size: usize) -> Self {
        Self {
            config,
            mix_buffer: vec![0.0; buffer_size],
        }
    }
}

/// Микшер с шинами
#[cfg(feature = "buses")]
pub struct BusingMixer {
    base_mixer: BasicMixer,
    buses: Vec<BusState>,
    sends: Vec<SendConfig>,
    buffer_size: usize,
}

#[cfg(feature = "buses")]
impl BusingMixer {
    pub fn new(base_mixer: BasicMixer, buffer_size: usize) -> Self {
        Self {
            base_mixer,
            buses: Vec::new(),
            sends: Vec::new(),
            buffer_size,
        }
    }
    
    pub fn add_bus(&mut self, config: BusConfig) {
        self.buses.push(BusState::new(config, self.buffer_size));
    }
    
    pub fn add_send(&mut self, send: SendConfig) {
        self.sends.push(send);
    }
    
    pub fn process(&mut self, inputs: &[f64]) -> (f64, f64) {
        // Сначала обрабатываем основной микшер
        let (main_left, main_right) = self.base_mixer.process(inputs);
        
        // Обрабатываем посылы на шины
        for send in &self.sends {
            if let Some(bus) = self.buses.get_mut(send.to_bus) {
                // Упрощённо: добавляем сигнал в шину
                bus.mix_buffer[0] += inputs.get(0).copied().unwrap_or(0.0) * send.level;
            }
        }
        
        (main_left, main_right)
    }
}

#[cfg(not(feature = "buses"))]
pub struct BusingMixer;

#[cfg(not(feature = "buses"))]
impl BusingMixer {
    pub fn new(_base_mixer: BasicMixer, _buffer_size: usize) -> Self {
        panic!("BusingMixer requires the 'buses' feature");
    }
}