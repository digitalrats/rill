use std::sync::Arc;
use serde::{Serialize, Deserialize};
use parking_lot::RwLock;
use rand::Rng;
use std::f32::consts::PI;

// Re-export типов
pub use kama_core::param::{ParamValue as CoreParamValue, ParamType};
use kama_core::{AudioNode, ParamValue, NodeMetadata, NodeCategory, AudioError, AudioResult};
use kama_core::node::ParamMetadata;

// --- Типы ошибок ---
#[derive(thiserror::Error, Debug)]
pub enum LofiError {
    #[error("Bit depth error: {0}")]
    BitDepth(String),
    
    #[error("Sample rate error: {0}")]
    SampleRate(String),
    
    #[error("Noise error: {0}")]
    Noise(String),
    
    #[error("Audio error: {0}")]
    Audio(#[from] AudioError),
}

pub type LofiResult<T> = Result<T, LofiError>;

// --- Конфигурация классических систем ---

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ClassicSystem {
    /// Nintendo Entertainment System (1983)
    /// - 5 каналов: 2 pulse, triangle, noise, DPCM
    /// - 7-bit DAC
    Nes,
    
    /// Commodore 64 SID (6581/8580)
    /// - 3 канала с complex waveforms
    /// - 8-bit DAC
    Commodore64,
    
    /// Yamaha YM2612 (Sega Genesis/Mega Drive)
    /// - 6 каналов FM синтеза
    /// - 9-bit DAC
    SegaGenesis,
    
    /// Roland D-50 (1987) - Первый популярный digital synth
    /// - 16-bit линейный PCM
    /// - 32kHz sample rate
    RolandD50,
    
    /// Akai S900 (1986) - Классический семплер
    /// - 12-bit с нелинейным кодированием
    /// - 40kHz max sample rate
    AkaiS900,
    
    /// E-mu Emulator II (1984)
    /// - 8-bit, 27.7kHz
    /// - Аналоговые фильтры после DAC
    EmulatorII,
    
    /// Fairlight CMI (1979)
    /// - 8-bit, 16kHz
    /// - Первый коммерческий семплер
    FairlightCMI,
    
    /// LinnDrum (1982) - Драм-машина
    /// - 8-bit drum samples
    LinnDrum,
    
    /// Custom - Пользовательская конфигурация
    Custom {
        bit_depth: u8,
        sample_rate: f32,
        nonlinear: bool,
        noise_floor: f32,
    },
}

impl ClassicSystem {
    pub fn get_bit_depth(&self) -> u8 {
        match self {
            ClassicSystem::Nes => 7,
            ClassicSystem::Commodore64 => 8,
            ClassicSystem::SegaGenesis => 9,
            ClassicSystem::RolandD50 => 16,
            ClassicSystem::AkaiS900 => 12,
            ClassicSystem::EmulatorII => 8,
            ClassicSystem::FairlightCMI => 8,
            ClassicSystem::LinnDrum => 8,
            ClassicSystem::Custom { bit_depth, .. } => *bit_depth,
        }
    }
    
    pub fn get_sample_rate(&self) -> f32 {
        match self {
            ClassicSystem::Nes => 44_100.0, // Эмуляция обычно на 44.1kHz
            ClassicSystem::Commodore64 => 44_100.0,
            ClassicSystem::SegaGenesis => 44_100.0,
            ClassicSystem::RolandD50 => 32_000.0,
            ClassicSystem::AkaiS900 => 40_000.0,
            ClassicSystem::EmulatorII => 27_700.0,
            ClassicSystem::FairlightCMI => 16_000.0,
            ClassicSystem::LinnDrum => 44_100.0,
            ClassicSystem::Custom { sample_rate, .. } => *sample_rate,
        }
    }
    
    pub fn has_nonlinear_encoding(&self) -> bool {
        matches!(self, 
            ClassicSystem::AkaiS900 | 
            ClassicSystem::Custom { nonlinear: true, .. }
        )
    }
    
    pub fn get_noise_floor_db(&self) -> f32 {
        match self {
            ClassicSystem::Nes => -42.0,     // ~7 бит
            ClassicSystem::Commodore64 => -48.0, // 8 бит
            ClassicSystem::AkaiS900 => -72.0,    // 12 бит
            ClassicSystem::FairlightCMI => -48.0, // 8 бит с шумами
            ClassicSystem::Custom { noise_floor, .. } => *noise_floor,
            _ => -90.0,
        }
    }
}

// --- Моделирование аппаратных ограничений ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareEmulation {
    pub bit_depth: u8,
    pub sample_rate: f32,
    pub dac_nonlinearity: bool,
    pub clock_drift: f32,        // Дрейф частоты тактового генератора (в %)
    pub voltage_drop: f32,       // Падение напряжения в цепях (0.0-1.0)
    pub crosstalk: f32,          // Перекрестные помехи между каналами
    pub thermal_noise: f32,      // Тепловой шум
    pub ageing_effect: f32,      // Эффект старения компонентов
}

impl Default for HardwareEmulation {
    fn default() -> Self {
        Self {
            bit_depth: 8,
            sample_rate: 44_100.0,
            dac_nonlinearity: true,
            clock_drift: 0.1,      // 0.1% дрейф
            voltage_drop: 0.02,    // 2% падение
            crosstalk: 0.01,       // 1% перекрестные помехи
            thermal_noise: 0.001,  // 0.1% тепловой шум
            ageing_effect: 0.05,   // 5% деградация
        }
    }
}

impl HardwareEmulation {
    pub fn for_system(system: ClassicSystem) -> Self {
        let mut emulation = Self::default();
        
        match system {
            ClassicSystem::Nes => {
                emulation.bit_depth = 7;
                emulation.clock_drift = 0.5;    // Неточный кварц
                emulation.voltage_drop = 0.05;  // Дешевые компоненты
                emulation.thermal_noise = 0.005;
            }
            ClassicSystem::Commodore64 => {
                emulation.bit_depth = 8;
                emulation.dac_nonlinearity = true; // SID известен нелинейностью
                emulation.clock_drift = 0.3;
                emulation.crosstalk = 0.03;     // Плохая разводка платы
            }
            ClassicSystem::AkaiS900 => {
                emulation.bit_depth = 12;
                emulation.dac_nonlinearity = true; // Нелинейное кодирование
                emulation.sample_rate = 40_000.0;
                emulation.thermal_noise = 0.001;
            }
            ClassicSystem::FairlightCMI => {
                emulation.bit_depth = 8;
                emulation.sample_rate = 16_000.0;
                emulation.clock_drift = 1.0;    // Очень неточные часы
                emulation.voltage_drop = 0.1;
                emulation.thermal_noise = 0.01;
            }
            _ => {
                emulation.bit_depth = system.get_bit_depth();
                emulation.sample_rate = system.get_sample_rate();
            }
        }
        
        emulation
    }
}

// --- DSP функции для lo-fi обработки ---

pub mod dsp {
    use super::*;
    
    /// Квантование с заданной битностью
    pub fn quantize(sample: f32, bit_depth: u8, dither: bool) -> f32 {
        if bit_depth >= 24 {
            return sample; // Практически без квантования
        }
        
        let steps = (1u32 << bit_depth) as f32;
        let max_val = 1.0 - (1.0 / steps); // Чтобы избежать clipping
        
        // Масштабируем
        let scaled = sample.clamp(-1.0, 1.0) * max_val;
        
        if dither {
            // Добавляем TPDF (Triangular Probability Density Function) dither
            let dither_amount = 1.0 / steps;
            let dither_sample = (rand::random::<f32>() - 0.5) * 2.0 * dither_amount;
            ((scaled + dither_sample) * steps).round() / steps
        } else {
            (scaled * steps).round() / steps
        }
    }
    
    /// Нелинейное квантование как в Akai S900
    pub fn nonlinear_quantize(sample: f32, bit_depth: u8) -> f32 {
        // Akai использовал нелинейное (логарифмическое) кодирование
        // для лучшего соотношения сигнал/шум на тихих сигналах
        
        let sign = sample.signum();
        let abs_sample = sample.abs().min(1.0);
        
        // Логарифмическое сжатие (μ-law подобное)
        let mu = 100.0;
        let compressed = sign * (1.0 + mu * abs_sample).ln() / (1.0 + mu).ln();
        
        // Линейное квантование сжатого сигнала
        let _quantized = quantize(compressed, bit_depth, false);
        
        // Обратное расширение
        let expanded = sign * ((1.0 + mu).ln().exp() - 1.0) / mu;
        
        expanded.clamp(-1.0, 1.0)
    }
    
    /// Редукция частоты дискретизации с aliasing'ом
    pub fn reduce_sample_rate(input: &[f32], output: &mut [f32], factor: usize) {
        if factor <= 1 {
            output.copy_from_slice(input);
            return;
        }
        
        // Простая децимация без anti-aliasing фильтра
        // (как в ранних цифровых системах)
        for (i, out) in output.iter_mut().enumerate() {
            let src_idx = i * factor;
            if src_idx < input.len() {
                *out = input[src_idx];
            } else {
                *out = 0.0;
            }
        }
    }
    
    /// Эмуляция нелинейности ЦАП
    pub fn dac_nonlinearity(sample: f32, model: DacModel) -> f32 {
        match model {
            DacModel::Ideal => sample,
            DacModel::R2R => {
                // R-2R лестничный ЦАП имеет нелинейность на переходах
                let steps = 256.0; // 8-битный
                let stepped = (sample * steps).round() / steps;
                
                // Добавляем нелинейность на средних уровнях
                let nonlinear = stepped * (1.0 + 0.05 * (2.0 * PI * stepped).sin());
                nonlinear.clamp(-1.0, 1.0)
            }
            DacModel::PWM => {
                // ШИМ ЦАП как в некоторых дешевых системах
                let pwm_noise = (rand::random::<f32>() - 0.5) * 0.01;
                (sample + pwm_noise).clamp(-1.0, 1.0)
            }
            DacModel::Multibit => {
                // Многобитный ЦАП с mismatch ошибками
                let mismatch = 0.02 * (sample * 3.0).sin(); // Гармонические искажения
                (sample + mismatch).clamp(-1.0, 1.0)
            }
        }
    }
    
    /// Добавление теплового шума
    pub fn add_thermal_noise(sample: f32, amount: f32) -> f32 {
        let noise = (rand::random::<f32>() - 0.5) * 2.0 * amount;
        (sample + noise).clamp(-1.0, 1.0)
    }
    
    /// Эмуляция дрейфа тактовой частоты
    pub fn apply_clock_drift(sample_rate: f32, drift: f32, time: f32) -> f32 {
        // Синусоидальный дрейф частоты
        let drift_variation = 1.0 + drift * 0.01 * (2.0 * PI * 0.1 * time).sin();
        sample_rate * drift_variation
    }
    
    /// Эмуляция падения напряжения (sag)
    pub fn voltage_sag(sample: f32, sag: f32) -> f32 {
        // Падение напряжения уменьшает амплитуду
        let sag_factor = 1.0 - sag;
        sample * sag_factor
    }
    
    /// Полная цепочка lo-fi обработки
    pub fn process_lofi_chain(
        input: f32,
        bit_depth: u8,
        _sample_rate_factor: f32,
        hardware: &HardwareEmulation,
        _time: f32,
    ) -> f32 {
        let mut sample = input;
        
        // 1. Падение напряжения
        sample = voltage_sag(sample, hardware.voltage_drop);
        
        // 2. Квантование
        sample = if hardware.dac_nonlinearity {
            nonlinear_quantize(sample, bit_depth)
        } else {
            quantize(sample, bit_depth, true)
        };
        
        // 3. Нелинейность ЦАП
        sample = dac_nonlinearity(sample, DacModel::R2R);
        
        // 4. Тепловой шум
        sample = add_thermal_noise(sample, hardware.thermal_noise);
        
        // 5. Эффект старения
        sample = sample * (1.0 - hardware.ageing_effect * 0.5);
        
        sample.clamp(-1.0, 1.0)
    }
    
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum DacModel {
        Ideal,     // Идеальный ЦАП
        R2R,       // R-2R лестничный (NES, Commodore)
        PWM,       // ШИМ ЦАП (дешевые системы)
        Multibit,  // Многобитный с ошибками
    }
}

// --- AudioNode для lo-fi обработки ---

pub struct LofiProcessor {
    config: LofiConfig,
    sample_rate: f32,
    time: f32,
    last_samples: Vec<f32>,
    sample_rate_buffer: Vec<f32>,
    temp_buffer: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LofiConfig {
    pub system: ClassicSystem,
    pub hardware: HardwareEmulation,
    pub enable_bitcrush: bool,
    pub enable_sr_reduction: bool,
    pub enable_noise: bool,
    pub output_gain: f32,
    pub dry_wet: f32,
}

impl Default for LofiConfig {
    fn default() -> Self {
        Self {
            system: ClassicSystem::Custom {
                bit_depth: 8,
                sample_rate: 44_100.0,
                nonlinear: false,
                noise_floor: -48.0,
            },
            hardware: HardwareEmulation::default(),
            enable_bitcrush: true,
            enable_sr_reduction: true,
            enable_noise: true,
            output_gain: 1.0,
            dry_wet: 1.0, // Полностью wet
        }
    }
}

impl LofiConfig {
    pub fn for_system(system: ClassicSystem) -> Self {
        Self {
            system: system.clone(),
            hardware: HardwareEmulation::for_system(system),
            ..Default::default()
        }
    }
}

impl LofiProcessor {
    pub fn new(config: LofiConfig) -> Self {
        Self {
            config,
            sample_rate: 44_100.0,
            time: 0.0,
            last_samples: Vec::new(),
            sample_rate_buffer: Vec::new(),
            temp_buffer: Vec::new(),
        }
    }
    
    pub fn for_system(system: ClassicSystem) -> Self {
        Self::new(LofiConfig::for_system(system))
    }
    
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let mut sample = input;
        
        // Сохраняем для децимации
        self.last_samples.push(sample);
        
        // Редукция sample rate если включена
        if self.config.enable_sr_reduction {
            let target_sr = self.config.system.get_sample_rate();
            let sr_factor = (self.sample_rate / target_sr).max(1.0);
            
            if sr_factor > 1.0 {
                // Накопили достаточно семплов для децимации
                if self.last_samples.len() >= sr_factor as usize {
                    // Берем каждый N-ый семпл (без anti-aliasing фильтра!)
                    sample = self.last_samples[0];
                    self.last_samples.clear();
                } else {
                    // Пока не накопили - используем предыдущий
                    return if let Some(last) = self.last_samples.last() {
                        *last
                    } else {
                        sample
                    };
                }
            }
        }
        
        // Применяем lo-fi обработку
        if self.config.enable_bitcrush || self.config.enable_noise {
            sample = dsp::process_lofi_chain(
                sample,
                self.config.system.get_bit_depth(),
                self.sample_rate / self.config.system.get_sample_rate(),
                &self.config.hardware,
                self.time,
            );
        }
        
        // Учитываем время для дрейфа частоты
        self.time += 1.0 / self.sample_rate;
        
        // Dry/wet mix
        let wet = sample * self.config.dry_wet;
        let dry = input * (1.0 - self.config.dry_wet);
        
        (wet + dry) * self.config.output_gain
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
        
        // Подготавливаем временный буфер
        if self.temp_buffer.len() < buffer_size {
            self.temp_buffer.resize(buffer_size, 0.0);
        }
        
        // Обрабатываем каждый семпл
        for i in 0..buffer_size {
            self.temp_buffer[i] = self.process_sample(input[i]);
        }
        
        // Копируем результат
        output[..buffer_size].copy_from_slice(&self.temp_buffer[..buffer_size]);
        
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
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("bit_depth", ParamValue::Int(v)) => {
                if let ClassicSystem::Custom { bit_depth, .. } = &mut self.config.system {
                    *bit_depth = v as u8;
                }
                Ok(())
            }
            ("sample_rate", ParamValue::Float(v)) => {
                if let ClassicSystem::Custom { sample_rate, .. } = &mut self.config.system {
                    *sample_rate = v.max(8000.0).min(192000.0);
                }
                Ok(())
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
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.time = 0.0;
        self.last_samples.clear();
        
        // Обновляем sample rate в конфигурации если это Custom
        if let ClassicSystem::Custom { sample_rate: cfg_sr, .. } = &mut self.config.system {
            *cfg_sr = sample_rate;
        }
    }
    
    fn reset(&mut self) {
        self.time = 0.0;
        self.last_samples.clear();
        self.sample_rate_buffer.clear();
        self.temp_buffer.clear();
    }
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: match self.config.system {
                ClassicSystem::Nes => "NES Emulator".to_string(),
                ClassicSystem::Commodore64 => "Commodore 64 SID".to_string(),
                ClassicSystem::AkaiS900 => "Akai S900".to_string(),
                ClassicSystem::FairlightCMI => "Fairlight CMI".to_string(),
                ClassicSystem::Custom { .. } => "Custom Lo-Fi".to_string(),
                _ => "Lo-Fi Processor".to_string(),
            },
            category: NodeCategory::Effect,
            description: "Classic digital audio system emulation".to_string(),
            author: "Kama Lo-Fi".to_string(),
            version: "1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "bit_depth".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(self.config.system.get_bit_depth() as i32),
                    min: Some(1.0),
                    max: Some(16.0),
                    step: Some(1.0),
                    unit: Some("bits".to_string()),
                    choices: Some(vec![
                        ("8-bit".to_string(), 8.0),
                        ("12-bit".to_string(), 12.0),
                        ("16-bit".to_string(), 16.0),
                    ]),
                },
                ParamMetadata {
                    name: "sample_rate".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(self.config.system.get_sample_rate()),
                    min: Some(8000.0),
                    max: Some(48000.0),
                    step: Some(100.0),
                    unit: Some("Hz".to_string()),
                    choices: Some(vec![
                        ("8kHz".to_string(), 8000.0),
                        ("16kHz".to_string(), 16000.0),
                        ("32kHz".to_string(), 32000.0),
                        ("44.1kHz".to_string(), 44100.0),
                    ]),
                },
                ParamMetadata {
                    name: "dry_wet".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(self.config.dry_wet),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("mix".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "output_gain".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(self.config.output_gain),
                    min: Some(0.0),
                    max: Some(4.0),
                    step: Some(0.01),
                    unit: Some("linear".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "enable_bitcrush".to_string(),
                    typ: ParamType::Bool,
                    default: ParamValue::Bool(self.config.enable_bitcrush),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: None,
                },
                ParamMetadata {
                    name: "enable_sr_reduction".to_string(),
                    typ: ParamType::Bool,
                    default: ParamValue::Bool(self.config.enable_sr_reduction),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: None,
                },
            ],
        }
    }
}

// --- Специализированные эмуляторы классических систем ---

pub mod emulators {
    use super::*;
    
    /// Эмулятор NES (2A03/2A07 sound chip)
    pub struct NesEmulator {
        pulse1: NesPulseChannel,
        pulse2: NesPulseChannel,
        triangle: NesTriangleChannel,
        noise: NesNoiseChannel,
        dpcm: NesDpcmChannel,
        mixer: NesMixer,
        lofi: LofiProcessor,
    }
    
    #[derive(Clone)]
    struct NesPulseChannel {
        duty_cycle: f32, // 0.125, 0.25, 0.5, 0.75
        frequency: f32,
        volume: f32,
        phase: f32,
        sweep_enabled: bool,
        sweep_rate: f32,
    }
    
    #[derive(Clone)]
    struct NesTriangleChannel {
        frequency: f32,
        volume: f32,
        phase: f32,
        linear_counter: u8,
    }
    
    #[derive(Clone)]
    struct NesNoiseChannel {
        mode: NoiseMode, // Short or long period
        frequency: f32,
        volume: f32,
        shift_register: u16,
    }
    
    #[derive(Clone)]
    struct NesDpcmChannel {
        sample_rate: f32,
        delta: i8,
        sample_buffer: Vec<i8>,
        position: usize,
    }
    
    struct NesMixer {
        pulse_mix: f32,
        tnd_mix: f32, // Triangle + Noise + DPCM
        output: f32,
    }
    
    #[derive(Debug, Clone, Copy)]
    enum NoiseMode {
        Short, // 93.9Hz - 28.1kHz
        Long,  // 46.9Hz - 14.0kHz
    }
    
    impl NesEmulator {
        pub fn new(sample_rate: f32) -> Self {
            let lofi_config = LofiConfig::for_system(ClassicSystem::Nes);
            
            Self {
                pulse1: NesPulseChannel {
                    duty_cycle: 0.25,
                    frequency: 440.0,
                    volume: 0.5,
                    phase: 0.0,
                    sweep_enabled: false,
                    sweep_rate: 0.0,
                },
                pulse2: NesPulseChannel {
                    duty_cycle: 0.125,
                    frequency: 660.0,
                    volume: 0.3,
                    phase: 0.0,
                    sweep_enabled: false,
                    sweep_rate: 0.0,
                },
                triangle: NesTriangleChannel {
                    frequency: 220.0,
                    volume: 0.4,
                    phase: 0.0,
                    linear_counter: 0,
                },
                noise: NesNoiseChannel {
                    mode: NoiseMode::Short,
                    frequency: 1000.0,
                    volume: 0.2,
                    shift_register: 1,
                },
                dpcm: NesDpcmChannel {
                    sample_rate: sample_rate / 2.0,
                    delta: 0,
                    sample_buffer: Vec::new(),
                    position: 0,
                },
                mixer: NesMixer {
                    pulse_mix: 0.5,
                    tnd_mix: 0.5,
                    output: 0.0,
                },
                lofi: LofiProcessor::new(lofi_config),
            }
        }
        
        pub fn generate(&mut self, output: &mut [f32]) {
            for out in output.iter_mut() {
                // Сохраняем sample_rate для использования
                let sample_rate = self.lofi.sample_rate;
                
                // Обновляем фазы pulse каналов напрямую
                self.pulse1.phase += self.pulse1.frequency / sample_rate;
                if self.pulse1.phase >= 1.0 {
                    self.pulse1.phase -= 1.0;
                }
                
                self.pulse2.phase += self.pulse2.frequency / sample_rate;
                if self.pulse2.phase >= 1.0 {
                    self.pulse2.phase -= 1.0;
                }
                
                // Обновляем фазу triangle напрямую
                self.triangle.phase += self.triangle.frequency / sample_rate;
                if self.triangle.phase >= 1.0 {
                    self.triangle.phase -= 1.0;
                }
                
                // Вычисляем значения pulse каналов
                let pulse1_val = if self.pulse1.phase < self.pulse1.duty_cycle {
                    1.0
                } else {
                    -1.0
                } * self.pulse1.volume;
                
                let pulse2_val = if self.pulse2.phase < self.pulse2.duty_cycle {
                    1.0
                } else {
                    -1.0
                } * self.pulse2.volume;
                
                // Вычисляем значение triangle
                let triangle_val = if self.triangle.phase < 0.5 {
                    self.triangle.phase * 4.0 - 1.0
                } else {
                    3.0 - self.triangle.phase * 4.0
                } * self.triangle.volume;
                
                // Генерируем шум - передаём всё необходимое как параметры
                let noise_val = Self::generate_noise_static(
                    &mut self.noise,
                    self.lofi.sample_rate
                );
                
                // Генерируем DPCM - передаём всё необходимое как параметры
                let dpcm_val = Self::generate_dpcm_static(&mut self.dpcm);
                
                // Микширование
                let pulse_mix = (pulse1_val + pulse2_val) * 0.5;
                let tnd_mix = (triangle_val * 3.0 + noise_val * 2.0 + dpcm_val) / 6.0;
                
                *out = (pulse_mix * self.mixer.pulse_mix + 
                        tnd_mix * self.mixer.tnd_mix) * 0.5;
                
                // Применяем lo-fi обработку
                *out = self.lofi.process_sample(*out);
            }
        }

        // Статический метод для генерации шума - не требует &mut self
        fn generate_noise_static(channel: &mut NesNoiseChannel, sample_rate: f32) -> f32 {
            let ticks_per_sample = sample_rate / channel.frequency;
            static mut TICK_COUNTER: f32 = 0.0;
            
            unsafe {
                TICK_COUNTER += 1.0;
                if TICK_COUNTER >= ticks_per_sample {
                    TICK_COUNTER = 0.0;
                    
                    let feedback = match channel.mode {
                        NoiseMode::Short => (channel.shift_register & 0x0001) ^ 
                                        ((channel.shift_register >> 6) & 0x0001),
                        NoiseMode::Long => (channel.shift_register & 0x0001) ^ 
                                        ((channel.shift_register >> 1) & 0x0001),
                    };
                    
                    channel.shift_register >>= 1;
                    channel.shift_register |= feedback << 14;
                }
                
                let sample = if (channel.shift_register & 0x0001) == 0 { 1.0 } else { -1.0 };
                sample * channel.volume
            }
        }

        // Статический метод для DPCM
        fn generate_dpcm_static(_channel: &mut NesDpcmChannel) -> f32 {
            0.0
        }
    impl AudioNode for NesEmulator {
        fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
            if outputs.is_empty() {
                return Ok(());
            }
            
            let output = &mut outputs[0];
            self.generate(output);
            
            Ok(())
        }
        
        fn get_param(&self, _name: &str) -> Option<ParamValue> { None }
        fn set_param(&mut self, _name: &str, _value: ParamValue) -> Result<(), AudioError> { Ok(()) }
        fn init(&mut self, sample_rate: f32) { self.lofi.init(sample_rate); }
        fn reset(&mut self) { self.lofi.reset(); }
        fn num_inputs(&self) -> usize { 0 }
        fn num_outputs(&self) -> usize { 1 }
        
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "NES Sound Chip".to_string(),
                category: NodeCategory::Generator,
                description: "Nintendo Entertainment System 2A03 sound chip emulation".to_string(),
                author: "Kama Lo-Fi".to_string(),
                version: "1.0".to_string(),
                parameters: Vec::new(),
            }
        }
    }
    
    /// Эмулятор Akai S900 семплера
    pub struct AkaiS900Emulator {
        buffer: Vec<f32>,
        position: f32,  // Изменено с usize на f32 для поддержки дробных позиций
        sample_rate: f32,
        bit_depth: u8,
        pitch: f32,
        loop_enabled: bool,
        loop_start: usize,
        loop_end: usize,
        lofi: LofiProcessor,
    }
    
    impl AkaiS900Emulator {
        pub fn new(sample_rate: f32) -> Self {
            let lofi_config = LofiConfig::for_system(ClassicSystem::AkaiS900);
            
            Self {
                buffer: Vec::new(),
                position: 0.0,
                sample_rate,
                bit_depth: 12,
                pitch: 1.0,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                lofi: LofiProcessor::new(lofi_config),
            }
        }
        
        pub fn load_sample(&mut self, samples: &[f32]) {
            self.buffer = samples.to_vec();
            self.loop_end = samples.len();
        }
        
        pub fn set_pitch(&mut self, pitch: f32) {
            self.pitch = pitch.max(0.1).min(4.0);
        }
        
        pub fn generate(&mut self, output: &mut [f32]) {
            if self.buffer.is_empty() {
                output.fill(0.0);
                return;
            }
            
            for out in output.iter_mut() {
                if (self.position as usize) >= self.buffer.len() {
                    *out = 0.0;
                    continue;
                }
                
                // Читаем семпл (с простой интерполяцией)
                let sample = if (self.position as usize) < self.buffer.len() - 1 {
                    let idx = self.position.floor() as usize;
                    let frac = self.position.fract();
                    self.buffer[idx] * (1.0 - frac) + self.buffer[idx + 1] * frac
                } else {
                    self.buffer[self.position as usize]
                };
                
                // Применяем lo-fi обработку S900
                *out = self.lofi.process_sample(sample);
                
                // Обновляем позицию с учетом pitch
                self.position += self.pitch;
                
                // Обработка петли
                if self.loop_enabled && (self.position as usize) >= self.loop_end {
                    self.position = self.loop_start as f32 + 
                                   (self.position - self.loop_end as f32);
                }
            }
        }
    }
    
    impl AudioNode for AkaiS900Emulator {
        fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
            if outputs.is_empty() {
                return Ok(());
            }
            
            let output = &mut outputs[0];
            self.generate(output);
            
            Ok(())
        }
        
        fn get_param(&self, name: &str) -> Option<ParamValue> {
            match name {
                "pitch" => Some(ParamValue::Float(self.pitch)),
                "loop_enabled" => Some(ParamValue::Bool(self.loop_enabled)),
                _ => None,
            }
        }
        
        fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
            match (name, value) {
                ("pitch", ParamValue::Float(v)) => {
                    self.pitch = v.max(0.1).min(4.0);
                    Ok(())
                }
                ("loop_enabled", ParamValue::Bool(v)) => {
                    self.loop_enabled = v;
                    Ok(())
                }
                _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
            }
        }
        
        fn init(&mut self, sample_rate: f32) {
            self.sample_rate = sample_rate;
            self.lofi.init(sample_rate);
        }
        
        fn reset(&mut self) {
            self.position = 0.0;
            self.lofi.reset();
        }
        
        fn num_inputs(&self) -> usize { 0 }
        fn num_outputs(&self) -> usize { 1 }
        
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "Akai S900".to_string(),
                category: NodeCategory::Generator,
                description: "Akai S900 sampler emulation".to_string(),
                author: "Kama Lo-Fi".to_string(),
                version: "1.0".to_string(),
                parameters: vec![
                    ParamMetadata {
                        name: "pitch".to_string(),
                        typ: ParamType::Float,
                        default: ParamValue::Float(1.0),
                        min: Some(0.1),
                        max: Some(4.0),
                        step: Some(0.01),
                        unit: Some("x".to_string()),
                        choices: None,
                    },
                    ParamMetadata {
                        name: "loop_enabled".to_string(),
                        typ: ParamType::Bool,
                        default: ParamValue::Bool(false),
                        min: None,
                        max: None,
                        step: None,
                        unit: None,
                        choices: None,
                    },
                ],
            }
        }
    }
}

// --- Интеграция с универсальными буферами ---

#[cfg(feature = "buffers")]
pub mod buffer_integration {
    use super::*;
    use kama_buffers::{AudioBuffer, SharedAudioBuffer, UniversalBufferSystem};
    
    /// Lo-Fi обертка для UniversalBufferSystem
    pub struct LofiBufferSystem {
        inner: UniversalBufferSystem,
        lofi_processors: Vec<LofiProcessor>,
        config: LofiConfig,
    }
    
    impl LofiBufferSystem {
        pub fn new(buffer_system: UniversalBufferSystem, config: LofiConfig) -> Self {
            let num_channels = buffer_system.num_outputs();
            let lofi_processors = (0..num_channels)
                .map(|_| LofiProcessor::new(config.clone()))
                .collect();
            
            Self {
                inner: buffer_system,
                lofi_processors,
                config,
            }
        }
        
        pub fn process_with_lofi(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
            // Обрабатываем через внутреннюю систему
            let mut temp_outputs: Vec<Vec<f32>> = (0..self.inner.num_outputs())
                .map(|_| vec![0.0; outputs[0].len()])
                .collect();
            
            let mut temp_output_slices: Vec<&mut [f32]> = temp_outputs.iter_mut()
                .map(|buf| buf.as_mut_slice())
                .collect();
            
            self.inner.process(inputs, &mut temp_output_slices)?;
            
            // Применяем lo-fi обработку к каждому каналу
            for (i, lofi) in self.lofi_processors.iter_mut().enumerate() {
                if i < outputs.len() && i < temp_outputs.len() {
                    let input_slice = &temp_outputs[i];
                    let output_slice = &mut outputs[i];
                    
                    let input_ref = [input_slice.as_slice()];
                    let mut output_refs = [output_slice];
                    
                    lofi.process(&input_ref, &mut output_refs)?;
                }
            }
            
            Ok(())
        }
    }
    
    impl AudioNode for LofiBufferSystem {
        fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
            self.process_with_lofi(inputs, outputs)
        }
        
        fn get_param(&self, name: &str) -> Option<ParamValue> {
            self.inner.get_param(name)
        }
        
        fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
            self.inner.set_param(name, value)
        }
        
        fn init(&mut self, sample_rate: f32) {
            self.inner.init(sample_rate);
            for lofi in &mut self.lofi_processors {
                lofi.init(sample_rate);
            }
        }
        
        fn reset(&mut self) {
            self.inner.reset();
            for lofi in &mut self.lofi_processors {
                lofi.reset();
            }
        }
        
        fn num_inputs(&self) -> usize { self.inner.num_inputs() }
        fn num_outputs(&self) -> usize { self.inner.num_outputs() }
        
        fn metadata(&self) -> NodeMetadata {
            let mut metadata = self.inner.metadata();
            metadata.name = format!("Lo-Fi {}", metadata.name);
            metadata
        }
    }
}

// --- Утилиты для создания lo-fi звуков ---

pub mod lofi_utils {
    use super::*;
    
    /// Создает характерный "8-bit" звук
    pub fn create_8bit_sound(samples: &[f32], bit_depth: u8) -> Vec<f32> {
        samples.iter()
            .map(|&s| dsp::quantize(s, bit_depth, true))
            .collect()
    }
    
    /// Добавляет шум как в старых семплерах
    pub fn add_vintage_noise(samples: &[f32], noise_level: f32) -> Vec<f32> {
        samples.iter()
            .map(|&s| dsp::add_thermal_noise(s, noise_level))
            .collect()
    }
    
    /// Эмулирует деградацию магнитной ленты
    pub fn add_tape_degradation(samples: &[f32], wear: f32) -> Vec<f32> {
        let mut result = Vec::with_capacity(samples.len());
        let mut high_freq_loss = 1.0 - wear * 0.5;
        
        for (i, &sample) in samples.iter().enumerate() {
            // Потеря высоких частот
            let filtered = sample * high_freq_loss;
            
            // Wow & flutter (медленное изменение pitch)
            let wow_flutter = 0.001 * wear * (2.0 * PI * i as f32 * 0.5 / 44100.0).sin();
            let pitched = filtered * (1.0 + wow_flutter);
            
            // Dropouts (пропадание сигнала)
            let dropout_chance = wear * 0.001;
            let final_sample = if rand::random::<f32>() < dropout_chance {
                0.0
            } else {
                pitched
            };
            
            result.push(final_sample.clamp(-1.0, 1.0));
            
            // Увеличиваем деградацию со временем
            high_freq_loss *= 0.99999;
        }
        
        result
    }
    
    /// Создает эффект "старой радиостанции"
    pub fn create_radio_effect(samples: &[f32], sample_rate: f32) -> Vec<f32> {
        let mut result = samples.to_vec();
        
        // Bandpass filter для имитации радио
        let center_freq = 1000.0;
        let q = 2.0;
        
        for i in 2..result.len() {
            // Простой цифровой фильтр
            let alpha = (PI * center_freq / sample_rate).sin() / (2.0 * q);
            let a0 = 1.0 + alpha;
            
            let b0 = alpha;
            let b1 = 0.0;
            let b2 = -alpha;
            let a1 = -2.0 * (2.0 * PI * center_freq / sample_rate).cos();
            let a2 = 1.0 - alpha;
            
            result[i] = (b0 * samples[i] + b1 * samples[i-1] + b2 * samples[i-2]
                        - a1 * result[i-1] - a2 * result[i-2]) / a0;
        }
        
        // Добавляем AM modulation шум
        for sample in result.iter_mut() {
            let am_noise = 0.05 * (2.0 * PI * 50.0 * rand::random::<f32>()).sin();
            *sample = (*sample * (1.0 + am_noise)).clamp(-1.0, 1.0);
        }
        
        result
    }
}

// --- Тесты ---

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_quantization() {
        let test_signal = vec![0.1, 0.5, 0.9, -0.3, -0.8];
        
        // 8-bit квантование
        let quantized_8bit: Vec<f32> = test_signal.iter()
            .map(|&s| dsp::quantize(s, 8, false))
            .collect();
        
        // 12-bit квантование
        let quantized_12bit: Vec<f32> = test_signal.iter()
            .map(|&s| dsp::quantize(s, 12, false))
            .collect();
        
        // 12-bit должно быть точнее
        let error_8bit: f32 = test_signal.iter()
            .zip(quantized_8bit.iter())
            .map(|(&a, &b)| (a - b).abs())
            .sum();
            
        let error_12bit: f32 = test_signal.iter()
            .zip(quantized_12bit.iter())
            .map(|(&a, &b)| (a - b).abs())
            .sum();
        
        assert!(error_12bit < error_8bit);
    }
    
    #[test]
    fn test_lofi_processor() {
        let config = LofiConfig {
            system: ClassicSystem::Custom {
                bit_depth: 8,
                sample_rate: 22050.0,
                nonlinear: false,
                noise_floor: -48.0,
            },
            ..Default::default()
        };
        
        let mut processor = LofiProcessor::new(config);
        processor.init(44100.0);
        
        let input = vec![0.5f32; 1024];
        let mut output = vec![0.0f32; 1024];
        
        let inputs = [&input[..]];
        let mut outputs = [&mut output[..]];
        
        processor.process(&inputs, &mut outputs).unwrap();
        
        // Проверяем, что обработка произошла
        assert_ne!(input[0], output[0]);
        assert!(output.iter().all(|&x| x.abs() <= 1.0));
    }
    
    #[test]
    fn test_nes_emulator() {
        let mut nes = emulators::NesEmulator::new(44100.0);
        
        let mut output = vec![0.0f32; 1024];
        let mut outputs = [&mut output[..]];
        
        nes.process(&[], &mut outputs).unwrap();
        
        // Проверяем, что NES генерирует звук
        assert!(output.iter().any(|&x| x != 0.0));
    }
    
    #[test]
    fn test_hardware_emulation() {
        let hardware = HardwareEmulation::for_system(ClassicSystem::FairlightCMI);
        
        assert_eq!(hardware.bit_depth, 8);
        assert_eq!(hardware.sample_rate, 16000.0);
        assert!(hardware.clock_drift > 0.0);
    }
    
    #[test]
    fn test_akai_s900_emulation() {
        let config = LofiConfig::for_system(ClassicSystem::AkaiS900);
        
        assert_eq!(config.system.get_bit_depth(), 12);
        assert_eq!(config.system.get_sample_rate(), 40000.0);
        assert!(config.hardware.dac_nonlinearity); // S900 имел нелинейное кодирование
    }
}