use std::sync::Arc;
use serde::{Serialize, Deserialize};
use parking_lot::RwLock;
use thiserror::Error;
use kama_core::{AudioNode, ParamValue, NodeMetadata, NodeCategory, AudioError, node::AudioNode as CoreAudioNode};

// Re-export для удобства
pub use kama_core::param::{ParamValue as CoreParamValue, ParamType};

// --- Типы ошибок ---
#[derive(Error, Debug)]
pub enum MixerError {
    #[error("Mixer configuration error: {0}")]
    Config(String),
    
    #[error("Filter error: {0}")]
    Filter(String),
    
    #[error("Channel error: {0}")]
    Channel(String),
    
    #[error("Processing error: {0}")]
    Processing(String),
    
    #[error("Audio error: {0}")]
    Audio(#[from] AudioError),
}

pub type MixerResult<T> = Result<T, MixerError>;

// --- Основные типы (совместимы с kama-core) ---

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ChannelType {
    Mono,
    Stereo,
    DualMono,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum FilterType {
    Bitcrusher,
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Shelf,
    Custom(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MixerMode {
    Normal,
    Parallel,
    Serial,
    Sidechain,
}

// --- Конфигурации с сериализацией для preset'ов ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub id: usize,
    pub name: String,
    pub channel_type: ChannelType,
    pub level: f32,           // 0.0 - 1.0
    pub pan: f32,             // -1.0 (L) до 1.0 (R)
    pub mute: bool,
    pub solo: bool,
    pub filters: Vec<FilterConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    pub filter_type: FilterType,
    pub enabled: bool,
    pub params: FilterParams,
    pub position: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterParams {
    pub bit_depth: Option<u8>,
    pub sample_rate_reduction: Option<f32>,
    pub cutoff: Option<f32>,
    pub resonance: Option<f32>,
    pub drive: Option<f32>,
    pub q: Option<f32>, // Quality factor для фильтров
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerConfig {
    pub name: String,
    pub channels: Vec<ChannelConfig>,
    pub master_level: f32,
    pub master_pan: f32,
    pub limiter_enabled: bool,
    pub limiter_threshold: f32,
    pub sample_rate: f32,
}

// --- DSP модуль с чистыми функциями ---

pub mod dsp {
    use super::*;
    use std::f32::consts::PI;
    
    // Типы для чистых функций обработки
    pub type MonoProcessor = fn(f32, &FilterState) -> f32;
    pub type StereoProcessor = fn((f32, f32), &FilterState) -> (f32, f32);
    
    #[derive(Debug, Clone)]
    pub struct FilterState {
        pub params: FilterParams,
        pub sample_rate: f32,
        pub internal_state: [f32; 4], // Фиксированный размер для производительности
        pub last_input: f32,
        pub last_output: f32,
    }
    
    impl FilterState {
        pub fn new(params: FilterParams, sample_rate: f32) -> Self {
            Self {
                params,
                sample_rate,
                internal_state: [0.0; 4],
                last_input: 0.0,
                last_output: 0.0,
            }
        }
    }
    
    pub mod filters {
        use super::*;
        
        // --- Биткрашер (Mono) ---
        pub fn bitcrusher_mono(input: f32, state: &FilterState) -> f32 {
            let mut sample = input;
            
            // Редукция битности
            if let Some(bit_depth) = state.params.bit_depth {
                let bits = bit_depth.clamp(1, 32);
                let steps = (1u32 << bits) as f32;
                sample = (sample * steps).round() / steps;
            }
            
            // Редукция частоты дискретизации
            if let Some(reduction) = state.params.sample_rate_reduction {
                let reduction = reduction.clamp(0.0, 1.0);
                if reduction > 0.0 && reduction < 1.0 {
                    // Sample and hold
                    let should_hold = rand::random::<f32>() < reduction;
                    sample = if should_hold { state.last_output } else { sample };
                }
            }
            
            // Перегрузка (drive)
            if let Some(drive) = state.params.drive {
                let drive = drive.clamp(0.0, 1.0);
                sample = sample * (1.0 + drive * 3.0);
                sample = sample.tanh(); // Soft clipping
            }
            
            sample
        }
        
        // --- Простой one-pole ФНЧ ---
        pub fn lowpass_mono(input: f32, state: &FilterState) -> f32 {
            if let Some(cutoff) = state.params.cutoff {
                let cutoff = cutoff.max(20.0).min(state.sample_rate * 0.45);
                let rc = 1.0 / (2.0 * PI * cutoff);
                let dt = 1.0 / state.sample_rate;
                let alpha = dt / (rc + dt);
                
                let output = state.last_output + alpha * (input - state.last_output);
                output
            } else {
                input
            }
        }
        
        // --- Простой ФВЧ ---
        pub fn highpass_mono(input: f32, state: &FilterState) -> f32 {
            if let Some(cutoff) = state.params.cutoff {
                let cutoff = cutoff.max(20.0).min(state.sample_rate * 0.45);
                let rc = 1.0 / (2.0 * PI * cutoff);
                let dt = 1.0 / state.sample_rate;
                let alpha = rc / (rc + dt);
                
                let output = alpha * (state.last_output + input - state.last_input);
                output
            } else {
                input
            }
        }
        
        // --- Полосовой фильтр ---
        pub fn bandpass_mono(input: f32, state: &FilterState) -> f32 {
            if let (Some(cutoff), Some(q)) = (state.params.cutoff, state.params.q) {
                let cutoff = cutoff.max(20.0).min(state.sample_rate * 0.45);
                let q = q.max(0.1).min(10.0);
                
                let omega = 2.0 * PI * cutoff / state.sample_rate;
                let alpha = omega.sin() / (2.0 * q);
                
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * omega.cos();
                let a2 = 1.0 - alpha;
                let b0 = alpha;
                let b1 = 0.0;
                let b2 = -alpha;
                
                // Простая реализация биквадратного фильтра
                let x = input;
                let y = (b0 * x + b1 * state.internal_state[0] + b2 * state.internal_state[1]
                    - a1 * state.internal_state[2] - a2 * state.internal_state[3]) / a0;
                
                // Обновляем состояния
                let mut new_state = state.clone();
                new_state.internal_state[1] = state.internal_state[0];
                new_state.internal_state[0] = x;
                new_state.internal_state[3] = state.internal_state[2];
                new_state.internal_state[2] = y;
                
                y
            } else {
                input
            }
        }
        
        // --- Режекторный фильтр ---
        pub fn notch_mono(input: f32, state: &FilterState) -> f32 {
            if let (Some(cutoff), Some(q)) = (state.params.cutoff, state.params.q) {
                let cutoff = cutoff.max(20.0).min(state.sample_rate * 0.45);
                let q = q.max(0.1).min(10.0);
                
                let omega = 2.0 * PI * cutoff / state.sample_rate;
                let alpha = omega.sin() / (2.0 * q);
                
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * omega.cos();
                let a2 = 1.0 - alpha;
                let b0 = 1.0;
                let b1 = -2.0 * omega.cos();
                let b2 = 1.0;
                
                let x = input;
                let y = (b0 * x + b1 * state.internal_state[0] + b2 * state.internal_state[1]
                    - a1 * state.internal_state[2] - a2 * state.internal_state[3]) / a0;
                
                // Обновляем состояния
                let mut new_state = state.clone();
                new_state.internal_state[1] = state.internal_state[0];
                new_state.internal_state[0] = x;
                new_state.internal_state[3] = state.internal_state[2];
                new_state.internal_state[2] = y;
                
                y
            } else {
                input
            }
        }
        
        // --- Композиция фильтров ---
        pub fn chain_mono_filters(filters: &[MonoProcessor]) -> MonoProcessor {
            move |input, state| {
                let mut result = input;
                for &filter in filters {
                    result = filter(result, state);
                }
                result
            }
        }
    }
    
    pub mod mixer {
        use super::*;
        
        // Микширование моно в стерео с панорамой
        pub fn mono_to_stereo(input: f32, pan: f32) -> (f32, f32) {
            let pan = pan.clamp(-1.0, 1.0);
            let left_gain = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
            let right_gain = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
            
            (input * left_gain, input * right_gain)
        }
        
        // Суммирование стерео сигналов
        pub fn sum_stereo(signals: &[(f32, f32)]) -> (f32, f32) {
            let mut left = 0.0;
            let mut right = 0.0;
            
            for &(l, r) in signals {
                left += l;
                right += r;
            }
            
            (left, right)
        }
        
        // Применение уровня с экспоненциальным сглаживанием
        pub fn apply_level((left, right): (f32, f32), level: f32, last_level: f32) -> (f32, f32) {
            // Экспоненциальное сглаживание
            let smoothed_level = 0.95 * last_level + 0.05 * level;
            (left * smoothed_level, right * smoothed_level)
        }
        
        // Мягкий ограничитель (soft clipper)
        pub fn soft_limiter((left, right): (f32, f32), threshold: f32) -> (f32, f32) {
            let limit = |x: f32| {
                if x.abs() > threshold {
                    threshold * x.signum() + (x - threshold * x.signum()).tanh() * 0.3
                } else {
                    x
                }
            };
            
            (limit(left), limit(right))
        }
        
        // Плавный mute
        pub fn smooth_mute((left, right): (f32, f32), muted: bool, mute_smoothing: &mut f32) -> (f32, f32) {
            let target = if muted { 0.0 } else { 1.0 };
            *mute_smoothing = 0.9 * *mute_smoothing + 0.1 * target;
            
            (left * *mute_smoothing, right * *mute_smoothing)
        }
    }
}

// --- AudioNode реализация микшера ---

/// Основной микшер как AudioNode для kama-core
pub struct MixerNode {
    config: MixerConfig,
    channels: Vec<Channel>,
    master_state: MasterState,
    sample_rate: f32,
    temp_buffers: (Vec<f32>, Vec<f32>), // Reusable buffers
    mute_smoothing: f32,
}

struct Channel {
    config: ChannelConfig,
    filter_chain: dsp::MonoProcessor,
    filter_state: dsp::FilterState,
    last_level: f32,
    peak_meter: (f32, f32),
}

struct MasterState {
    level: f32,
    pan: f32,
    limiter_enabled: bool,
    limiter_threshold: f32,
    last_level: f32,
    peak_meter: (f32, f32),
}

impl MixerNode {
    pub fn new(config: MixerConfig) -> Self {
        let sample_rate = config.sample_rate;
        
        // Инициализируем каналы
        let channels = config.channels.iter()
            .cloned()
            .map(|channel_config| {
                // Создаем цепочку фильтров для канала
                let filters: Vec<dsp::MonoProcessor> = channel_config.filters.iter()
                    .filter(|f| f.enabled)
                    .map(|f| match f.filter_type {
                        FilterType::Bitcrusher => dsp::filters::bitcrusher_mono,
                        FilterType::LowPass => dsp::filters::lowpass_mono,
                        FilterType::HighPass => dsp::filters::highpass_mono,
                        FilterType::BandPass => dsp::filters::bandpass_mono,
                        FilterType::Notch => dsp::filters::notch_mono,
                        _ => |x, _| x,
                    })
                    .collect();
                
                let filter_chain = dsp::filters::chain_mono_filters(&filters);
                
                let filter_state = dsp::FilterState::new(
                    channel_config.filters.first()
                        .map(|f| f.params.clone())
                        .unwrap_or_default(),
                    sample_rate,
                );
                
                Channel {
                    config: channel_config,
                    filter_chain,
                    filter_state,
                    last_level: 1.0,
                    peak_meter: (0.0, 0.0),
                }
            })
            .collect();
        
        // Мастер состояние
        let master_state = MasterState {
            level: config.master_level,
            pan: config.master_pan,
            limiter_enabled: config.limiter_enabled,
            limiter_threshold: config.limiter_threshold,
            last_level: config.master_level,
            peak_meter: (0.0, 0.0),
        };
        
        Self {
            config,
            channels,
            master_state,
            sample_rate,
            temp_buffers: (Vec::new(), Vec::new()),
            mute_smoothing: 1.0,
        }
    }
    
    fn process_channel(&mut self, channel_idx: usize, input: &[f32], output_left: &mut [f32], output_right: &mut [f32]) {
        let channel = &mut self.channels[channel_idx];
        
        // Пропускаем muted каналы
        if channel.config.mute {
            for i in 0..input.len() {
                output_left[i] = 0.0;
                output_right[i] = 0.0;
            }
            return;
        }
        
        // Проверяем solo режим
        let any_solo = self.channels.iter().any(|c| c.config.solo);
        if any_solo && !channel.config.solo {
            for i in 0..input.len() {
                output_left[i] = 0.0;
                output_right[i] = 0.0;
            }
            return;
        }
        
        // Обрабатываем каждый семпл
        for i in 0..input.len() {
            let input_sample = input[i];
            
            // Применяем фильтры
            let filtered = (channel.filter_chain)(input_sample, &channel.filter_state);
            
            // Обновляем состояние фильтра
            channel.filter_state.last_input = input_sample;
            channel.filter_state.last_output = filtered;
            
            // Панорамирование и уровень
            let (left, right) = dsp::mixer::mono_to_stereo(filtered, channel.config.pan);
            let (left_out, right_out) = dsp::mixer::apply_level(
                (left, right),
                channel.config.level,
                channel.last_level,
            );
            
            channel.last_level = channel.config.level;
            
            // Обновляем peak meter
            channel.peak_meter.0 = channel.peak_meter.0.max(left_out.abs());
            channel.peak_meter.1 = channel.peak_meter.1.max(right_out.abs());
            
            output_left[i] += left_out;
            output_right[i] += right_out;
        }
    }
    
    fn process_master(&mut self, left_input: &[f32], right_input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut left_output = vec![0.0; left_input.len()];
        let mut right_output = vec![0.0; right_input.len()];
        
        for i in 0..left_input.len() {
            let left = left_input[i];
            let right = right_input[i];
            
            // Применяем мастер панораму
            let (left_panned, right_panned) = if self.master_state.pan != 0.0 {
                let mono = (left + right) * 0.5;
                dsp::mixer::mono_to_stereo(mono, self.master_state.pan)
            } else {
                (left, right)
            };
            
            // Применяем мастер уровень
            let (left_leveled, right_leveled) = dsp::mixer::apply_level(
                (left_panned, right_panned),
                self.master_state.level,
                self.master_state.last_level,
            );
            
            self.master_state.last_level = self.master_state.level;
            
            // Лимитер
            let (final_left, final_right) = if self.master_state.limiter_enabled {
                dsp::mixer::soft_limiter(
                    (left_leveled, right_leveled),
                    self.master_state.limiter_threshold,
                )
            } else {
                (left_leveled, right_leveled)
            };
            
            // Обновляем мастер peak meter
            self.master_state.peak_meter.0 = self.master_state.peak_meter.0.max(final_left.abs());
            self.master_state.peak_meter.1 = self.master_state.peak_meter.1.max(final_right.abs());
            
            left_output[i] = final_left;
            right_output[i] = final_right;
        }
        
        (left_output, right_output)
    }
    
    pub fn get_channel_meter(&self, channel_idx: usize) -> Option<(f32, f32)> {
        self.channels.get(channel_idx).map(|c| c.peak_meter)
    }
    
    pub fn get_master_meter(&self) -> (f32, f32) {
        self.master_state.peak_meter
    }
    
    pub fn reset_meters(&mut self) {
        for channel in &mut self.channels {
            channel.peak_meter = (0.0, 0.0);
        }
        self.master_state.peak_meter = (0.0, 0.0);
    }
    
    pub fn set_channel_level(&mut self, channel_idx: usize, level: f32) {
        if let Some(channel) = self.channels.get_mut(channel_idx) {
            channel.config.level = level.clamp(0.0, 1.0);
        }
    }
    
    pub fn set_channel_pan(&mut self, channel_idx: usize, pan: f32) {
        if let Some(channel) = self.channels.get_mut(channel_idx) {
            channel.config.pan = pan.clamp(-1.0, 1.0);
        }
    }
    
    pub fn toggle_channel_mute(&mut self, channel_idx: usize) {
        if let Some(channel) = self.channels.get_mut(channel_idx) {
            channel.config.mute = !channel.config.mute;
        }
    }
    
    pub fn toggle_channel_solo(&mut self, channel_idx: usize) {
        if let Some(channel) = self.channels.get_mut(channel_idx) {
            channel.config.solo = !channel.config.solo;
        }
    }
    
    pub fn set_master_level(&mut self, level: f32) {
        self.master_state.level = level.clamp(0.0, 1.0);
    }
    
    pub fn set_master_pan(&mut self, pan: f32) {
        self.master_state.pan = pan.clamp(-1.0, 1.0);
    }
    
    pub fn export_config(&self) -> MixerConfig {
        self.config.clone()
    }
}

impl AudioNode for MixerNode {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if outputs.len() < 2 {
            return Err(AudioError::Processing("Mixer requires at least 2 outputs".to_string()));
        }
        
        let buffer_size = outputs[0].len();
        
        // Инициализируем временные буферы если нужно
        if self.temp_buffers.0.len() != buffer_size {
            self.temp_buffers = (
                vec![0.0; buffer_size],
                vec![0.0; buffer_size],
            );
        }
        
        let (temp_left, temp_right) = &mut self.temp_buffers;
        temp_left.fill(0.0);
        temp_right.fill(0.0);
        
        // Обрабатываем каждый входной канал
        for (i, input) in inputs.iter().enumerate() {
            if i < self.channels.len() {
                self.process_channel(i, input, temp_left, temp_right);
            }
        }
        
        // Обрабатываем мастер секцию
        let (master_left, master_right) = self.process_master(temp_left, temp_right);
        
        // Копируем в выходные буферы
        let out_left = &mut outputs[0];
        let out_right = &mut outputs[1];
        
        for i in 0..buffer_size {
            out_left[i] = master_left[i];
            out_right[i] = master_right[i];
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "master_level" => Some(ParamValue::Float(self.master_state.level)),
            "master_pan" => Some(ParamValue::Float(self.master_state.pan)),
            "limiter_enabled" => Some(ParamValue::Bool(self.master_state.limiter_enabled)),
            "limiter_threshold" => Some(ParamValue::Float(self.master_state.limiter_threshold)),
            _ => {
                // Пытаемся получить параметр канала
                if let Some((channel_idx, param_name)) = name.split_once('_') {
                    if let Ok(idx) = channel_idx.parse::<usize>() {
                        if let Some(channel) = self.channels.get(idx) {
                            return match param_name {
                                "level" => Some(ParamValue::Float(channel.config.level)),
                                "pan" => Some(ParamValue::Float(channel.config.pan)),
                                "mute" => Some(ParamValue::Bool(channel.config.mute)),
                                "solo" => Some(ParamValue::Bool(channel.config.solo)),
                                _ => None,
                            };
                        }
                    }
                }
                None
            }
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("master_level", ParamValue::Float(v)) => {
                self.set_master_level(v);
                Ok(())
            }
            ("master_pan", ParamValue::Float(v)) => {
                self.set_master_pan(v);
                Ok(())
            }
            ("limiter_enabled", ParamValue::Bool(v)) => {
                self.master_state.limiter_enabled = v;
                Ok(())
            }
            ("limiter_threshold", ParamValue::Float(v)) => {
                self.master_state.limiter_threshold = v.max(0.01).min(1.0);
                Ok(())
            }
            _ => {
                // Пытаемся установить параметр канала
                if let Some((channel_idx, param_name)) = name.split_once('_') {
                    if let Ok(idx) = channel_idx.parse::<usize>() {
                        if let Some(channel) = self.channels.get_mut(idx) {
                            match (param_name, value) {
                                ("level", ParamValue::Float(v)) => {
                                    channel.config.level = v.clamp(0.0, 1.0);
                                    Ok(())
                                }
                                ("pan", ParamValue::Float(v)) => {
                                    channel.config.pan = v.clamp(-1.0, 1.0);
                                    Ok(())
                                }
                                ("mute", ParamValue::Bool(v)) => {
                                    channel.config.mute = v;
                                    Ok(())
                                }
                                ("solo", ParamValue::Bool(v)) => {
                                    channel.config.solo = v;
                                    Ok(())
                                }
                                _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
                            }
                        } else {
                            Err(AudioError::Parameter(format!("Invalid channel index: {}", idx)))
                        }
                    } else {
                        Err(AudioError::Parameter(format!("Invalid channel specifier: {}", name)))
                    }
                } else {
                    Err(AudioError::Parameter(format!("Unknown parameter: {}", name)))
                }
            }
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.config.sample_rate = sample_rate;
        
        // Обновляем sample rate во всех состояниях фильтров
        for channel in &mut self.channels {
            channel.filter_state.sample_rate = sample_rate;
        }
    }
    
    fn reset(&mut self) {
        for channel in &mut self.channels {
            channel.filter_state = dsp::FilterState::new(
                channel.config.filters.first()
                    .map(|f| f.params.clone())
                    .unwrap_or_default(),
                self.sample_rate,
            );
            channel.last_level = 1.0;
            channel.peak_meter = (0.0, 0.0);
        }
        
        self.master_state.last_level = self.master_state.level;
        self.master_state.peak_meter = (0.0, 0.0);
        self.mute_smoothing = 1.0;
    }
    
    fn num_inputs(&self) -> usize {
        self.channels.len()
    }
    
    fn num_outputs(&self) -> usize {
        2 // Всегда стерео выход
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Mixer".to_string(),
            category: NodeCategory::Mixer,
            description: "Multi-channel mixer with filters and panning".to_string(),
            author: "Kama Mixer".to_string(),
            version: "1.0".to_string(),
            parameters: {
                let mut params = vec![
                    kama_core::node::ParamMetadata {
                        name: "master_level".to_string(),
                        typ: ParamType::Float,
                        default: ParamValue::Float(0.8),
                        min: Some(0.0),
                        max: Some(1.0),
                        step: Some(0.01),
                        unit: Some("linear".to_string()),
                        choices: None,
                    },
                    kama_core::node::ParamMetadata {
                        name: "master_pan".to_string(),
                        typ: ParamType::Float,
                        default: ParamValue::Float(0.0),
                        min: Some(-1.0),
                        max: Some(1.0),
                        step: Some(0.01),
                        unit: Some("pan".to_string()),
                        choices: None,
                    },
                    kama_core::node::ParamMetadata {
                        name: "limiter_enabled".to_string(),
                        typ: ParamType::Bool,
                        default: ParamValue::Bool(true),
                        min: None,
                        max: None,
                        step: None,
                        unit: None,
                        choices: None,
                    },
                    kama_core::node::ParamMetadata {
                        name: "limiter_threshold".to_string(),
                        typ: ParamType::Float,
                        default: ParamValue::Float(0.9),
                        min: Some(0.01),
                        max: Some(1.0),
                        step: Some(0.01),
                        unit: Some("linear".to_string()),
                        choices: None,
                    },
                ];
                
                // Добавляем параметры для каждого канала
                for (i, channel) in self.channels.iter().enumerate() {
                    params.push(kama_core::node::ParamMetadata {
                        name: format!("{}_level", i),
                        typ: ParamType::Float,
                        default: ParamValue::Float(channel.config.level),
                        min: Some(0.0),
                        max: Some(1.0),
                        step: Some(0.01),
                        unit: Some("linear".to_string()),
                        choices: None,
                    });
                    
                    params.push(kama_core::node::ParamMetadata {
                        name: format!("{}_pan", i),
                        typ: ParamType::Float,
                        default: ParamValue::Float(channel.config.pan),
                        min: Some(-1.0),
                        max: Some(1.0),
                        step: Some(0.01),
                        unit: Some("pan".to_string()),
                        choices: None,
                    });
                    
                    params.push(kama_core::node::ParamMetadata {
                        name: format!("{}_mute", i),
                        typ: ParamType::Bool,
                        default: ParamValue::Bool(channel.config.mute),
                        min: None,
                        max: None,
                        step: None,
                        unit: None,
                        choices: None,
                    });
                    
                    params.push(kama_core::node::ParamMetadata {
                        name: format!("{}_solo", i),
                        typ: ParamType::Bool,
                        default: ParamValue::Bool(channel.config.solo),
                        min: None,
                        max: None,
                        step: None,
                        unit: None,
                        choices: None,
                    });
                }
                
                params
            },
        }
    }
}

// --- Фабрики для удобного создания микшеров ---

pub struct MixerFactory;

impl MixerFactory {
    pub fn create_5ch_mixer(sample_rate: f32) -> MixerNode {
        let config = MixerConfig {
            name: "5-Channel Mixer".to_string(),
            channels: (0..5).map(|i| ChannelConfig {
                id: i,
                name: format!("Channel {}", i + 1),
                channel_type: ChannelType::Mono,
                level: match i {
                    0 => 0.8,
                    1 => 0.7,
                    2 => 0.6,
                    3 => 0.7,
                    4 => 0.8,
                    _ => 0.5,
                },
                pan: match i {
                    0 => -0.5,
                    1 => -0.25,
                    2 => 0.0,
                    3 => 0.25,
                    4 => 0.5,
                    _ => 0.0,
                },
                mute: false,
                solo: false,
                filters: if i == 0 {
                    // На первом канале биткрашер по умолчанию
                    vec![FilterConfig {
                        filter_type: FilterType::Bitcrusher,
                        enabled: true,
                        params: FilterParams {
                            bit_depth: Some(8),
                            sample_rate_reduction: Some(0.3),
                            cutoff: None,
                            resonance: None,
                            drive: Some(0.2),
                            q: None,
                        },
                        position: 0,
                    }]
                } else {
                    Vec::new()
                },
            }).collect(),
            master_level: 0.8,
            master_pan: 0.0,
            limiter_enabled: true,
            limiter_threshold: 0.9,
            sample_rate,
        };
        
        MixerNode::new(config)
    }
    
    pub fn create_granular_mixer(sample_rate: f32) -> MixerNode {
        let config = MixerConfig {
            name: "Granular Mixer".to_string(),
            channels: (0..5).map(|i| {
                let filter_type = match i {
                    0 => FilterType::Bitcrusher,
                    1 => FilterType::LowPass,
                    2 => FilterType::HighPass,
                    3 => FilterType::BandPass,
                    4 => FilterType::Notch,
                    _ => FilterType::Bitcrusher,
                };
                
                ChannelConfig {
                    id: i,
                    name: format!("Granular Ch {}", i + 1),
                    channel_type: ChannelType::Mono,
                    level: 0.7,
                    pan: (i as f32 - 2.0) * 0.25,
                    mute: false,
                    solo: false,
                    filters: vec![FilterConfig {
                        filter_type,
                        enabled: true,
                        params: FilterParams {
                            bit_depth: if filter_type == FilterType::Bitcrusher {
                                Some(4 + i as u8 * 2)
                            } else {
                                None
                            },
                            sample_rate_reduction: if filter_type == FilterType::Bitcrusher {
                                Some(0.1 + i as f32 * 0.1)
                            } else {
                                None
                            },
                            cutoff: if filter_type != FilterType::Bitcrusher {
                                Some(1000.0 + i as f32 * 500.0)
                            } else {
                                None
                            },
                            resonance: if filter_type != FilterType::Bitcrusher {
                                Some(0.5)
                            } else {
                                None
                            },
                            drive: Some(0.1),
                            q: if matches!(filter_type, FilterType::BandPass | FilterType::Notch) {
                                Some(2.0)
                            } else {
                                None
                            },
                        },
                        position: 0,
                    }],
                }
            }).collect(),
            master_level: 0.8,
            master_pan: 0.0,
            limiter_enabled: true,
            limiter_threshold: 0.95,
            sample_rate,
        };
        
        MixerNode::new(config)
    }
}

// --- Отдельные фильтры как AudioNode ---

pub struct BitcrusherNode {
    params: FilterParams,
    state: dsp::FilterState,
    sample_rate: f32,
}

impl BitcrusherNode {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            params: FilterParams {
                bit_depth: Some(8),
                sample_rate_reduction: Some(0.0),
                cutoff: None,
                resonance: None,
                drive: Some(0.0),
                q: None,
            },
            state: dsp::FilterState::new(FilterParams::default(), sample_rate),
            sample_rate,
        }
    }
    
    pub fn set_bit_depth(&mut self, bits: u8) {
        self.params.bit_depth = Some(bits.clamp(1, 32));
    }
    
    pub fn set_sample_rate_reduction(&mut self, reduction: f32) {
        self.params.sample_rate_reduction = Some(reduction.clamp(0.0, 1.0));
    }
    
    pub fn set_drive(&mut self, drive: f32) {
        self.params.drive = Some(drive.clamp(0.0, 1.0));
    }
}

impl AudioNode for BitcrusherNode {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let input = inputs[0];
        let output = &mut outputs[0];
        
        for i in 0..input.len().min(output.len()) {
            output[i] = dsp::filters::bitcrusher_mono(input[i], &self.state);
            
            // Обновляем состояние
            self.state.last_input = input[i];
            self.state.last_output = output[i];
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "bit_depth" => self.params.bit_depth.map(ParamValue::Int),
            "sample_rate_reduction" => self.params.sample_rate_reduction.map(ParamValue::Float),
            "drive" => self.params.drive.map(ParamValue::Float),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("bit_depth", ParamValue::Int(v)) => {
                self.set_bit_depth(v as u8);
                Ok(())
            }
            ("sample_rate_reduction", ParamValue::Float(v)) => {
                self.set_sample_rate_reduction(v);
                Ok(())
            }
            ("drive", ParamValue::Float(v)) => {
                self.set_drive(v);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.state.sample_rate = sample_rate;
    }
    
    fn reset(&mut self) {
        self.state = dsp::FilterState::new(self.params.clone(), self.sample_rate);
    }
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Bitcrusher".to_string(),
            category: NodeCategory::Effect,
            description: "Digital bit reduction and sample rate reduction".to_string(),
            author: "Kama Mixer".to_string(),
            version: "1.0".to_string(),
            parameters: vec![
                kama_core::node::ParamMetadata {
                    name: "bit_depth".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(8),
                    min: Some(1.0),
                    max: Some(32.0),
                    step: Some(1.0),
                    unit: Some("bits".to_string()),
                    choices: None,
                },
                kama_core::node::ParamMetadata {
                    name: "sample_rate_reduction".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.0),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("linear".to_string()),
                    choices: None,
                },
                kama_core::node::ParamMetadata {
                    name: "drive".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.0),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("linear".to_string()),
                    choices: None,
                },
            ],
        }
    }
}

// Аналогично можно реализовать другие фильтры:
// - LowPassNode
// - HighPassNode  
// - BandPassNode
// - NotchNode

// --- Интеграция с kama-automation ---

#[cfg(feature = "automation")]
pub mod automation_integration {
    use super::*;
    use kama_automation::{AutomationManager, Servo, ServoMapping};
    
    pub struct AutomatedMixer {
        mixer: MixerNode,
        automation: AutomationManager,
    }
    
    impl AutomatedMixer {
        pub fn new(mixer: MixerNode, sample_rate: f32) -> Self {
            Self {
                mixer,
                automation: AutomationManager::new(sample_rate as f64),
            }
        }
        
        pub fn add_channel_automation(&mut self, channel_idx: usize, param: &str) {
            // Здесь можно добавить LFO автоматизацию для параметров канала
            // Используя kama-automation систему
        }
        
        pub fn process_with_automation(
            &mut self,
            inputs: &[&[f32]],
            outputs: &mut [&mut [f32]],
        ) -> Result<(), AudioError> {
            // Обновляем автоматизацию
            self.automation.process();
            
            // Применяем автоматизированные параметры
            // (нужна интеграция с параметрами микшера)
            
            // Обрабатываем аудио
            self.mixer.process(inputs, outputs)
        }
    }
}

// --- Вспомогательные реализации ---

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            bit_depth: None,
            sample_rate_reduction: None,
            cutoff: None,
            resonance: None,
            drive: None,
            q: None,
        }
    }
}

// --- Примеры использования ---

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mixer_creation() {
        let mixer = MixerFactory::create_5ch_mixer(44100.0);
        
        assert_eq!(mixer.num_inputs(), 5);
        assert_eq!(mixer.num_outputs(), 2);
        
        let metadata = mixer.metadata();
        assert_eq!(metadata.name, "Mixer");
        assert_eq!(metadata.category, NodeCategory::Mixer);
    }
    
    #[test]
    fn test_bitcrusher_node() {
        let mut bitcrusher = BitcrusherNode::new(44100.0);
        
        // Проверяем параметры по умолчанию
        assert_eq!(bitcrusher.get_param("bit_depth"), Some(ParamValue::Int(8)));
        
        // Меняем параметры
        bitcrusher.set_param("bit_depth", ParamValue::Int(4)).unwrap();
        assert_eq!(bitcrusher.get_param("bit_depth"), Some(ParamValue::Int(4)));
        
        // Тестируем обработку
        let input = [0.5f32; 128];
        let mut output = [0.0f32; 128];
        let inputs = [&input[..]];
        let mut outputs = [&mut output[..]];
        
        bitcrusher.process(&inputs, &mut outputs).unwrap();
        
        // Проверяем, что выход не равен входу (биткрашер что-то сделал)
        assert_ne!(input[0], output[0]);
    }
    
    #[test]
    fn test_mixer_processing() {
        let mut mixer = MixerFactory::create_5ch_mixer(44100.0);
        
        // Создаем тестовые входы
        let inputs: Vec<Vec<f32>> = (0..5)
            .map(|i| vec![0.1 * (i + 1) as f32; 64])
            .collect();
        
        let input_refs: Vec<&[f32]> = inputs.iter().map(|v| v.as_slice()).collect();
        let mut left_output = vec![0.0f32; 64];
        let mut right_output = vec![0.0f32; 64];
        let mut outputs = [&mut left_output[..], &mut right_output[..]];
        
        // Обрабатываем
        mixer.process(&input_refs, &mut outputs).unwrap();
        
        // Проверяем, что выходы не нулевые
        assert!(left_output.iter().any(|&x| x != 0.0));
        assert!(right_output.iter().any(|&x| x != 0.0));
        
        // Проверяем peak meter
        let master_meter = mixer.get_master_meter();
        assert!(master_meter.0 > 0.0);
        assert!(master_meter.1 > 0.0);
    }
    
    #[test]
    fn test_dsp_functions() {
        use crate::dsp;
        
        // Тест биткрашера
        let state = dsp::FilterState::new(FilterParams {
            bit_depth: Some(4),
            sample_rate_reduction: Some(0.0),
            ..Default::default()
        }, 44100.0);
        
        let input = 0.75;
        let output = dsp::filters::bitcrusher_mono(input, &state);
        
        // При 4 битах квантование должно быть заметно
        assert_ne!(input, output);
        
        // Тест lowpass
        let state2 = dsp::FilterState::new(FilterParams {
            cutoff: Some(1000.0),
            ..Default::default()
        }, 44100.0);
        
        let output2 = dsp::filters::lowpass_mono(1.0, &state2);
        assert!(output2 <= 1.0);
    }
}