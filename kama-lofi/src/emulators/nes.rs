use kama_core::{AudioNode, ParamValue, NodeMetadata, NodeCategory, AudioError};
use crate::config::LofiConfig;
use crate::lofi_processor::LofiProcessor;

// NES-specific structs and implementation
// ... (весь код NES эмулятора из оригинального lib.rs)

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

            name: "NES Sound Chip".to_string(),
            category: NodeCategory::Generator,
            description: "Nintendo Entertainment System 2A03 sound chip emulation".to_string(),
            author: "Kama Lo-Fi".to_string(),
            version: "1.0".to_string(),
            parameters
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
