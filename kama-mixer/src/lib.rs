//! Продвинутый микшер и система маршрутизации для Kama Audio

#![warn(missing_docs)]

use std::sync::Arc;
use serde::{Serialize, Deserialize};
use parking_lot::RwLock;

pub mod channel;
pub mod filter;
pub mod bus;
pub mod routing;
pub mod dsp;

// Re-exports
pub use channel::{ChannelConfig, ChannelType, ChannelState};
pub use filter::{FilterConfig, FilterType, FilterParams};
pub use bus::{BusConfig, MasterConfig};
pub use routing::{RoutingMatrix, MixerMode};
pub use dsp::{AudioProcessor, StereoProcessor};

/// Конфигурация микшера
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerConfig {
    /// Имя микшера
    pub name: String,
    /// Каналы микшера
    pub channels: Vec<ChannelConfig>,
    /// Мастер секция
    pub master: MasterConfig,
    /// Дополнительные шины
    pub buses: Vec<BusConfig>,
    /// Режим работы
    pub mode: MixerMode,
    /// Частота дискретизации
    pub sample_rate: f64,
}

/// Основной микшер
pub struct FunctionalMixer {
    config: MixerConfig,
    channels: Vec<ChannelState>,
    master: MasterState,
    #[cfg(feature = "high_precision")]
    hp_buffer: Option<kama_hp::HighPrecisionBuffer>,
}

impl FunctionalMixer {
    /// Создаёт новый микшер
    pub fn new(config: MixerConfig) -> Result<Self, String> {
        // Валидация
        if config.channels.len() > 32 {
            return Err("Maximum 32 channels allowed".to_string());
        }
        
        // Инициализируем каналы
        let channels = config.channels.iter()
            .cloned()
            .enumerate()
            .map(|(i, channel_config)| {
                ChannelState::new(channel_config, config.sample_rate, i)
            })
            .collect();
        
        // Инициализируем мастер
        let master = MasterState::new(config.master.clone(), config.sample_rate);
        
        Ok(Self {
            config,
            channels,
            master,
            #[cfg(feature = "high_precision")]
            hp_buffer: None,
        })
    }
    
    /// Обрабатывает входные данные
    pub fn process(&mut self, inputs: &[f64]) -> (f64, f64) {
        // Проверяем solo состояния
        let any_solo = self.channels.iter().any(|c| c.config.solo);
        
        // Обрабатываем каждый канал
        let mut channel_outputs = Vec::new();
        
        for (i, channel) in self.channels.iter_mut().enumerate() {
            let input = inputs.get(i).copied().unwrap_or(0.0);
            
            // Пропускаем muted каналы или каналы не в solo режиме
            if (any_solo && !channel.config.solo) || channel.config.mute {
                channel.reset_meter();
                continue;
            }
            
            let output = channel.process(input);
            channel_outputs.push(output);
        }
        
        // Суммируем все каналы
        let summed = dsp::sum_stereo(&channel_outputs);
        
        // Применяем мастер обработку
        self.master.process(summed)
    }
    
    /// Экспортирует конфигурацию
    pub fn export_config(&self) -> MixerConfig {
        self.config.clone()
    }
    
    /// Импортирует конфигурацию
    pub fn import_config(&mut self, config: MixerConfig) -> Result<(), String> {
        *self = Self::new(config)?;
        Ok(())
    }
}

/// Фабрика для создания типовых конфигураций микшеров
pub struct MixerFactory;

impl MixerFactory {
    /// Создаёт конфигурацию 5-канального стерео микшера
    pub fn five_channel_stereo() -> MixerConfig {
        let mut channels = Vec::new();
        
        // 5 стерео каналов
        for i in 0..5 {
            channels.push(ChannelConfig {
                id: i,
                name: format!("Channel {}", i + 1),
                channel_type: ChannelType::Stereo,
                level: if i == 0 { 0.8 } else { 0.7 },
                pan: match i {
                    0 => -0.3,
                    1 => -0.1,
                    2 => 0.0,
                    3 => 0.1,
                    4 => 0.3,
                    _ => 0.0,
                },
                mute: false,
                solo: false,
                filters: Vec::new(),
                sends: Vec::new(),
            });
        }
        
        MixerConfig {
            name: "5-Channel Stereo Mixer".to_string(),
            channels,
            master: MasterConfig {
                level: 0.8,
                pan: 0.0,
                filters: Vec::new(),
                limiter_enabled: true,
                limiter_threshold: 0.9,
            },
            buses: Vec::new(),
            mode: MixerMode::Normal,
            sample_rate: 44100.0,
        }
    }
    
    /// Создаёт конфигурацию гранулярного микшера
    pub fn granular_mixer() -> MixerConfig {
        let mut channels = Vec::new();
        
        // 5 каналов с разными типами
        for i in 0..5 {
            let channel_type = match i {
                0 => ChannelType::Mono,
                1 => ChannelType::Stereo,
                2 => ChannelType::DualMono,
                3 => ChannelType::Stereo,
                4 => ChannelType::Mono,
                _ => ChannelType::Mono,
            };
            
            channels.push(ChannelConfig {
                id: i,
                name: format!("Granular Ch {}", i + 1),
                channel_type,
                level: 0.7,
                pan: (i as f64 - 2.0) * 0.25, // Распределение по панораме
                mute: false,
                solo: false,
                filters: Vec::new(),
                sends: Vec::new(),
            });
        }
        
        MixerConfig {
            name: "Granular Mixer".to_string(),
            channels,
            master: MasterConfig {
                level: 0.8,
                pan: 0.0,
                filters: vec![],
                limiter_enabled: true,
                limiter_threshold: 0.95,
            },
            buses: Vec::new(),
            mode: MixerMode::Parallel,
            sample_rate: 44100.0,
        }
    }
}