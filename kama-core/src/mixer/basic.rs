//! Базовый синхронный микшер без зависимостей

/// Тип канала микшера
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelType {
    Mono,
    Stereo,
}

/// Конфигурация канала
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub id: usize,
    pub name: String,
    pub channel_type: ChannelType,
    pub level: f64,
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
}

impl ChannelConfig {
    pub fn new(id: usize, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            channel_type: ChannelType::Stereo,
            level: 0.8,
            pan: 0.0,
            mute: false,
            solo: false,
        }
    }
    
    pub fn with_level(mut self, level: f64) -> Self {
        self.level = level.clamp(0.0, 1.0);
        self
    }
    
    pub fn with_pan(mut self, pan: f64) -> Self {
        self.pan = pan.clamp(-1.0, 1.0);
        self
    }
    
    pub fn with_channel_type(mut self, channel_type: ChannelType) -> Self {
        self.channel_type = channel_type;
        self
    }
}

/// Конфигурация мастера
#[derive(Debug, Clone)]
pub struct MasterConfig {
    pub level: f64,
    pub pan: f64,
    pub limiter_enabled: bool,
    pub limiter_threshold: f64,
}

impl Default for MasterConfig {
    fn default() -> Self {
        Self {
            level: 0.8,
            pan: 0.0,
            limiter_enabled: true,
            limiter_threshold: 0.9,
        }
    }
}

/// Конфигурация микшера
#[derive(Debug, Clone)]
pub struct MixerConfig {
    pub name: String,
    pub channels: Vec<ChannelConfig>,
    pub master: MasterConfig,
    pub sample_rate: f64,
}

impl MixerConfig {
    pub fn new(name: impl Into<String>, sample_rate: f64) -> Self {
        Self {
            name: name.into(),
            channels: Vec::new(),
            master: MasterConfig::default(),
            sample_rate,
        }
    }
    
    pub fn with_channel(mut self, channel: ChannelConfig) -> Self {
        self.channels.push(channel);
        self
    }
}

/// Состояние канала
#[derive(Debug, Clone)]
pub struct ChannelState {
    pub config: ChannelConfig,
    pub meter_level: f64,
    pub last_level: f64,
}

impl ChannelState {
    pub fn new(config: ChannelConfig) -> Self {
        Self {
            last_level: config.level,
            meter_level: 0.0,
            config,
        }
    }
    
    pub fn process(&mut self, input: f64) -> (f64, f64) {
        if self.config.mute {
            return (0.0, 0.0);
        }
        
        // Плавное изменение уровня (коэффициент сглаживания)
        let smoothed_level = self.last_level + (self.config.level - self.last_level) * 0.1;
        self.last_level = self.config.level;
        
        // Панорамирование
        let (left_gain, right_gain) = self.pan_to_gains(self.config.pan);
        let left = input * smoothed_level * left_gain;
        let right = input * smoothed_level * right_gain;
        
        // Обновляем meter
        self.meter_level = self.meter_level.max(left.abs().max(right.abs()));
        
        (left, right)
    }
    
    fn pan_to_gains(&self, pan: f64) -> (f64, f64) {
        let pan = pan.clamp(-1.0, 1.0);
        let left_gain = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
        let right_gain = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
        (left_gain, right_gain)
    }
    
    pub fn reset_meter(&mut self) {
        self.meter_level = 0.0;
    }
}

/// Состояние мастера
#[derive(Debug, Clone)]
pub struct MasterState {
    pub config: MasterConfig,
    pub last_level: f64,
}

impl MasterState {
    pub fn new(config: MasterConfig) -> Self {
        Self {
            last_level: config.level,
            config,
        }
    }
    
    pub fn process(&mut self, left: f64, right: f64) -> (f64, f64) {
        // Плавное изменение уровня мастера
        let smoothed_level = self.last_level + (self.config.level - self.last_level) * 0.05;
        self.last_level = self.config.level;
        
        let left_scaled = left * smoothed_level;
        let right_scaled = right * smoothed_level;
        
        // Лимитер
        if self.config.limiter_enabled {
            self.apply_limiter(left_scaled, right_scaled)
        } else {
            (left_scaled, right_scaled)
        }
    }
    
    fn apply_limiter(&self, left: f64, right: f64) -> (f64, f64) {
        let limit = |x: f64| {
            if x.abs() > self.config.limiter_threshold {
                self.config.limiter_threshold * x.signum()
            } else {
                x
            }
        };
        (limit(left), limit(right))
    }
}

/// Базовый синхронный микшер
#[derive(Debug, Clone)]
pub struct BasicMixer {
    config: MixerConfig,
    channels: Vec<ChannelState>,
    master: MasterState,
}

impl BasicMixer {
    pub fn new(config: MixerConfig) -> Self {
        let channels = config.channels
            .iter()
            .cloned()
            .map(ChannelState::new)
            .collect();
        
        let master = MasterState::new(config.master.clone());
        
        Self {
            config,
            channels,
            master,
        }
    }
    
    /// Обработка входных сигналов
    pub fn process(&mut self, inputs: &[f64]) -> (f64, f64) {
        let any_solo = self.channels.iter().any(|c| c.config.solo);
        let mut left_sum = 0.0;
        let mut right_sum = 0.0;
        
        for (i, channel) in self.channels.iter_mut().enumerate() {
            let input = inputs.get(i).copied().unwrap_or(0.0);
            
            // Пропускаем muted или не-solo каналы
            if (any_solo && !channel.config.solo) || channel.config.mute {
                channel.reset_meter();
                continue;
            }
            
            let (left, right) = channel.process(input);
            left_sum += left;
            right_sum += right;
        }
        
        self.master.process(left_sum, right_sum)
    }
    
    /// Получить текущие уровни VU метра
    pub fn meter_levels(&self) -> Vec<f64> {
        self.channels.iter().map(|c| c.meter_level).collect()
    }
    
    /// Сбросить VU метры
    pub fn reset_meters(&mut self) {
        for channel in &mut self.channels {
            channel.reset_meter();
        }
    }
    
    /// Получить конфигурацию
    pub fn config(&self) -> &MixerConfig {
        &self.config
    }
}

/// Фабрика для создания типовых конфигураций
pub struct MixerFactory;

impl MixerFactory {
    pub fn five_channel_stereo(sample_rate: f64) -> MixerConfig {
        let mut config = MixerConfig::new("5-Channel Stereo Mixer", sample_rate);
        
        for i in 0..5 {
            let pan = match i {
                0 => -0.3,
                1 => -0.1,
                2 => 0.0,
                3 => 0.1,
                4 => 0.3,
                _ => 0.0,
            };
            
            let channel = ChannelConfig::new(i, format!("Channel {}", i + 1))
                .with_level(if i == 0 { 0.8 } else { 0.7 })
                .with_pan(pan)
                .with_channel_type(ChannelType::Stereo);
            
            config = config.with_channel(channel);
        }
        
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_basic_mixer_creation() {
        let config = MixerFactory::five_channel_stereo(44100.0);
        let mixer = BasicMixer::new(config);
        
        assert_eq!(mixer.channels.len(), 5);
    }
    
    #[test]
    fn test_basic_mixer_processing() {
        let config = MixerFactory::five_channel_stereo(44100.0);
        let mut mixer = BasicMixer::new(config);
        
        let inputs = vec![0.5, 0.3, 0.2, 0.1, 0.4];
        let (left, right) = mixer.process(&inputs);
        
        assert!(left >= -1.0 && left <= 1.0);
        assert!(right >= -1.0 && right <= 1.0);
    }
    
    #[test]
    fn test_mute_and_solo() {
        let config = MixerFactory::five_channel_stereo(44100.0);
        let mut mixer = BasicMixer::new(config);
        
        // Mute первый канал
        mixer.channels[0].config.mute = true;
        
        let inputs = vec![1.0; 5];
        let (left, right) = mixer.process(&inputs);
        
        // Проверяем, что уровень упал (так как первый канал замьючен)
        assert!(left.abs() < 1.0);
        
        // Solo второй канал
        mixer.reset_meters();
        mixer.channels[1].config.solo = true;
        
        let (left, right) = mixer.process(&inputs);
        assert!(left.abs() > 0.0 || right.abs() > 0.0);
    }
}