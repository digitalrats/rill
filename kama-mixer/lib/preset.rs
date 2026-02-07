//! Система preset'ов для микшера

use serde::{Serialize, Deserialize};
use std::path::Path;
use crate::{MixerConfig, FilterType, ChannelConfig, FilterConfig, FilterParams};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerPreset {
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub config: MixerConfig,
    pub tags: Vec<String>,
}

impl MixerPreset {
    pub fn new(name: &str, config: MixerConfig) -> Self {
        Self {
            name: name.to_string(),
            description: String::new(),
            author: String::new(),
            version: "1.0".to_string(),
            config,
            tags: Vec::new(),
        }
    }
    
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
    
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        let preset = serde_json::from_str(&json)?;
        Ok(preset)
    }
}

// Предустановленные пресеты
pub mod presets {
    use super::*;
    
    pub fn classic_rock_mixer() -> MixerPreset {
        let config = MixerConfig {
            name: "Classic Rock".to_string(),
            channels: vec![
                ChannelConfig { // Kick
                    id: 0,
                    name: "Kick".to_string(),
                    channel_type: crate::ChannelType::Mono,
                    level: 0.9,
                    pan: 0.0,
                    mute: false,
                    solo: false,
                    filters: vec![],
                },
                ChannelConfig { // Snare
                    id: 1,
                    name: "Snare".to_string(),
                    channel_type: crate::ChannelType::Mono,
                    level: 0.8,
                    pan: 0.0,
                    mute: false,
                    solo: false,
                    filters: vec![FilterConfig {
                        filter_type: FilterType::Bitcrusher,
                        enabled: true,
                        params: FilterParams {
                            bit_depth: Some(12),
                            sample_rate_reduction: Some(0.2),
                            drive: Some(0.3),
                            ..Default::default()
                        },
                        position: 0,
                    }],
                },
                // ... больше каналов
            ],
            master_level: 0.8,
            master_pan: 0.0,
            limiter_enabled: true,
            limiter_threshold: 0.9,
            sample_rate: 44100.0,
        };
        
        MixerPreset::new("Classic Rock", config)
    }
    
    pub fn lofi_beat_mixer() -> MixerPreset {
        let config = MixerConfig {
            name: "Lo-Fi Beat".to_string(),
            channels: (0..5).map(|i| ChannelConfig {
                id: i,
                name: format!("Lo-Fi {}", i + 1),
                channel_type: crate::ChannelType::Mono,
                level: 0.7,
                pan: (i as f32 - 2.0) * 0.3,
                mute: false,
                solo: false,
                filters: vec![FilterConfig {
                    filter_type: FilterType::Bitcrusher,
                    enabled: true,
                    params: FilterParams {
                        bit_depth: Some(8),
                        sample_rate_reduction: Some(0.4 + i as f32 * 0.1),
                        drive: Some(0.2),
                        ..Default::default()
                    },
                    position: 0,
                }],
            }).collect(),
            master_level: 0.8,
            master_pan: 0.0,
            limiter_enabled: true,
            limiter_threshold: 0.95,
            sample_rate: 44100.0,
        };
        
        MixerPreset::new("Lo-Fi Beat", config)
    }
}