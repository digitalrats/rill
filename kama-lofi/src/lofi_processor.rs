use kama_core::traits::{
    AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId,
    ParamMetadata, ParamType
};
use kama_buffers::{RingBuffer, BufferHead, ReadMode, BufferManager};
use crate::config::{LofiConfig, ClassicSystem};
use crate::dsp;

/// Процессор для lo-fi эмуляции
///
/// Предоставляет эмуляцию классических цифровых аудиосистем:
/// - Биткрашинг (понижение битности)
/// - Понижение частоты дискретизации
/// - Добавление винтажного шума
/// - Эмуляция ЦАП старых устройств
/// - Многоголовочная задержка для ленточных эффектов
pub struct LofiProcessor {
    config: LofiConfig,
    sample_rate: f32,
    time: f32,
    
    // Буферы из kama-buffers
    buffer_manager: Option<BufferManager>,
    delay_buffer: RingBuffer,
    heads: Vec<BufferHead>,
    
    // Состояние для DSP операций
    last_sample: f32,
    sample_hold_counter: usize,
    reduction_factor: usize,
    
    // Для статистики
    processed_samples: u64,
}

impl LofiProcessor {
    /// Создать новый процессор с заданной конфигурацией
    pub fn new(config: LofiConfig) -> Self {
        let buffer_size = match config.system {
            ClassicSystem::Nes => 256,        // Маленький буфер как в NES
            ClassicSystem::Commodore64 => 512, // SID чип
            ClassicSystem::AkaiS900 => 4096,   // Семплер с большей памятью
            ClassicSystem::FairlightCMI => 2048,
            _ => 1024,
        };
        
        Self {
            config,
            sample_rate: 44_100.0,
            time: 0.0,
            buffer_manager: None,
            delay_buffer: RingBuffer::new(buffer_size),
            heads: Vec::new(),
            last_sample: 0.0,
            sample_hold_counter: 0,
            reduction_factor: 1,
            processed_samples: 0,
        }
    }
    
    /// Создать процессор для конкретной системы
    pub fn for_system(system: ClassicSystem) -> Self {
        Self::new(LofiConfig::for_system(system))
    }
    
    /// Интеграция с BufferManager
    pub fn with_buffer_manager(mut self, manager: BufferManager) -> Self {
        self.buffer_manager = Some(manager);
        self
    }
    
    /// Добавить головку воспроизведения (для эмуляции многоголовых магнитофонов)
    pub fn add_head(&mut self, speed: f32, pan: f32, volume: f32) -> usize {
        let id = self.heads.len();
        let mut head = BufferHead::new(id)
            .with_speed(speed)
            .with_pan(pan)
            .with_volume(volume);
        
        // Для кастомных систем добавляем гранулярный режим
        if matches!(self.config.system, ClassicSystem::Custom { .. }) {
            head.read_mode = ReadMode::Granular {
                grain_size: 256,
                spacing: 512,
                randomization: 0.3,
            };
        }
        
        self.heads.push(head);
        id
    }
    
    /// Получить мутабельную ссылку на головку
    pub fn get_head_mut(&mut self, index: usize) -> Option<&mut BufferHead> {
        self.heads.get_mut(index)
    }
    
    /// Обработать один семпл
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let mut sample = input;
        
        // Обновляем коэффициент понижения частоты если изменилась целевая частота
        if self.config.enable_sr_reduction {
            let target_sr = self.config.system.get_sample_rate();
            self.reduction_factor = dsp::quantization::calculate_reduction_factor(
                self.sample_rate, target_sr
            );
        }
        
        // 1. Биткрашинг
        if self.config.enable_bitcrush {
            let bit_depth = self.config.system.get_bit_depth();
            sample = dsp::quantization::bitcrush(sample, bit_depth, true);
        }
        
        // 2. Понижение частоты дискретизации
        if self.config.enable_sr_reduction && self.reduction_factor > 1 {
            sample = dsp::quantization::sample_rate_reduce(
                sample, 
                self.reduction_factor,
                &mut self.last_sample,
                &mut self.sample_hold_counter
            );
        }
        
        // 3. Добавление шума (в стиле системы)
        if self.config.enable_noise {
            sample = dsp::noise::system_noise(self.config.system, sample);
        }
        
        // 4. Эмуляция ЦАП
        sample = dsp::dac_emulation::for_system(self.config.system, sample);
        
        // 5. Запись в буфер задержки для возможных эффектов
        self.delay_buffer.write(&[sample]);
        
        // 6. Обработка головками (если есть)
        if !self.heads.is_empty() {
            let view = self.delay_buffer.view();
            let mut mixed = 0.0;
            
            for head in &mut self.heads {
                if head.enabled {
                    mixed += head.read_sample(&view);
                }
            }
            
            // Микшируем прямой сигнал с обработанным
            sample = sample * 0.7 + mixed * 0.3;
        }
        
        self.time += 1.0 / self.sample_rate;
        self.processed_samples += 1;
        
        // Применяем dry/wet mix
        let wet = sample * self.config.dry_wet;
        let dry = input * (1.0 - self.config.dry_wet);
        
        (wet + dry) * self.config.output_gain
    }
    
    /// Очистить буфер задержки
    pub fn clear_delay_buffer(&mut self) {
        self.delay_buffer.reset();
    }
    
    /// Получить статистику обработки
    pub fn stats(&self) -> (u64, f32) {
        (self.processed_samples, self.time)
    }
    
    /// Установить целевую частоту дискретизации (для custom систем)
    pub fn set_target_sample_rate(&mut self, sr: f32) {
        if let ClassicSystem::Custom { ref mut sample_rate, .. } = self.config.system {
            *sample_rate = sr;
        }
    }
}

impl AudioNode for LofiProcessor {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let input = inputs[0];
        let output = &mut outputs[0];
        let buffer_size = input.len().min(output.len());
        
        // Обрабатываем каждый семпл
        for i in 0..buffer_size {
            output[i] = self.process_sample(input[i]);
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "bit_depth" => Some(ParamValue::Int(self.config.system.get_bit_depth() as i32)),
            "sample_rate" => Some(ParamValue::Float(self.config.system.get_sample_rate())),
            "dry_wet" => Some(ParamValue::Float(self.config.dry_wet)),
            "output_gain" => Some(ParamValue::Float(self.config.output_gain)),
            "enable_bitcrush" => Some(ParamValue::Bool(self.config.enable_bitcrush)),
            "enable_sr_reduction" => Some(ParamValue::Bool(self.config.enable_sr_reduction)),
            "enable_noise" => Some(ParamValue::Bool(self.config.enable_noise)),
            "system" => {
                let system_name = match self.config.system {
                    ClassicSystem::Nes => "NES",
                    ClassicSystem::Commodore64 => "Commodore64",
                    ClassicSystem::AkaiS900 => "AkaiS900",
                    ClassicSystem::FairlightCMI => "FairlightCMI",
                    ClassicSystem::Custom { .. } => "Custom",
                    _ => "Unknown",
                };
                Some(ParamValue::Choice(system_name.to_string()))
            }
            "processed_samples" => Some(ParamValue::Int(self.processed_samples as i32)),
            "num_heads" => Some(ParamValue::Int(self.heads.len() as i32)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("bit_depth", ParamValue::Int(v)) => {
                if let ClassicSystem::Custom { ref mut bit_depth, .. } = self.config.system {
                    *bit_depth = v as u8;
                    Ok(())
                } else {
                    Err(AudioError::Parameter("Cannot change bit_depth of fixed system".into()))
                }
            }
            ("sample_rate", ParamValue::Float(v)) => {
                if let ClassicSystem::Custom { ref mut sample_rate, .. } = self.config.system {
                    *sample_rate = v.max(8000.0).min(192000.0);
                    Ok(())
                } else {
                    Err(AudioError::Parameter("Cannot change sample_rate of fixed system".into()))
                }
            }
            ("dry_wet", ParamValue::Float(v)) => {
                self.config.dry_wet = v.clamp(0.0, 1.0);
                Ok(())
            }
            ("output_gain", ParamValue::Float(v)) => {
                self.config.output_gain = v.max(0.0).min(4.0);
                Ok(())
            }
            ("enable_bitcrush", ParamValue::Bool(v)) => {
                self.config.enable_bitcrush = v;
                Ok(())
            }
            ("enable_sr_reduction", ParamValue::Bool(v)) => {
                self.config.enable_sr_reduction = v;
                Ok(())
            }
            ("enable_noise", ParamValue::Bool(v)) => {
                self.config.enable_noise = v;
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sr: f32) {
        self.sample_rate = sr;
        self.time = 0.0;
        self.processed_samples = 0;
        self.delay_buffer.reset();
        
        // Обновляем target sample rate в конфиге если это кастомная система
        if let ClassicSystem::Custom { ref mut sample_rate, .. } = self.config.system {
            *sample_rate = sr;
        }
        
        // Предвычисляем коэффициент понижения частоты
        if self.config.enable_sr_reduction {
            let target_sr = self.config.system.get_sample_rate();
            self.reduction_factor = dsp::quantization::calculate_reduction_factor(
                sr, target_sr
            );
        }
    }
    
    fn reset(&mut self) {
        self.time = 0.0;
        self.last_sample = 0.0;
        self.sample_hold_counter = 0;
        self.processed_samples = 0;
        self.delay_buffer.reset();
        
        for head in &mut self.heads {
            head.reset();
        }
    }
    
    fn num_inputs(&self) -> usize {
        1  // Моно вход
    }
    
    fn num_outputs(&self) -> usize {
        1  // Моно выход (можно расширить до стерео)
    }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        let system_name = match self.config.system {
            ClassicSystem::Nes => "NES Emulator",
            ClassicSystem::Commodore64 => "Commodore 64 SID",
            ClassicSystem::AkaiS900 => "Akai S900 Sampler",
            ClassicSystem::FairlightCMI => "Fairlight CMI",
            ClassicSystem::Custom { .. } => "Custom Lo-Fi",
            _ => "Lo-Fi Processor",
        };
        
        let description = match self.config.system {
            ClassicSystem::Nes => "Nintendo Entertainment System sound chip".to_string(),
            ClassicSystem::Commodore64 => "Commodore 64 SID chip".to_string(),
            ClassicSystem::AkaiS900 => "Akai S900 12-bit sampler".to_string(),
            ClassicSystem::FairlightCMI => "Fairlight CMI (first digital sampler)".to_string(),
            ClassicSystem::Custom { bit_depth, sample_rate, .. } => 
                format!("Custom {}-bit at {} Hz", bit_depth, sample_rate),
            _ => "vintage digital audio system".to_string(),
        };
        
        NodeMetadata {
            name: system_name.to_string(),
            category: NodeCategory::Effect,
            description,
            author: "Kama Lo-Fi".to_string(),
            version: "0.2.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "system".to_string(),
                    typ: ParamType::Choice,
                    default: ParamValue::Choice("NES".to_string()),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: Some(vec![
                        ("NES".to_string(), 0.0),
                        ("Commodore64".to_string(), 1.0),
                        ("AkaiS900".to_string(), 2.0),
                        ("FairlightCMI".to_string(), 3.0),
                        ("Custom".to_string(), 4.0),
                    ]),
                },
                ParamMetadata {
                    name: "bit_depth".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(8),
                    min: Some(1.0),
                    max: Some(16.0),
                    step: Some(1.0),
                    unit: Some("bits".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "dry_wet".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("mix".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "output_gain".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.0),
                    max: Some(4.0),
                    step: Some(0.1),
                    unit: Some("gain".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "enable_bitcrush".to_string(),
                    typ: ParamType::Bool,
                    default: ParamValue::Bool(true),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: None,
                },
                ParamMetadata {
                    name: "enable_sr_reduction".to_string(),
                    typ: ParamType::Bool,
                    default: ParamValue::Bool(true),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: None,
                },
                ParamMetadata {
                    name: "enable_noise".to_string(),
                    typ: ParamType::Bool,
                    default: ParamValue::Bool(true),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: None,
                },
                ParamMetadata {
                    name: "num_heads".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(0),
                    min: Some(0.0),
                    max: Some(8.0),
                    step: Some(1.0),
                    unit: Some("heads".to_string()),
                    choices: None,
                },
            ],
        }
    }
}