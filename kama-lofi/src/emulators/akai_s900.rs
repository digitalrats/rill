use kama_core::{AudioNode, ParamValue, NodeMetadata, NodeCategory, AudioError, node::ParamMetadata};
use crate::config::LofiConfig;
use crate::lofi_processor::LofiProcessor;

pub struct AkaiS900Emulator {
    buffer: Vec<f32>,
    position: f32,
    sample_rate: f32,
    bit_depth: u8,
    pitch: f32,
    loop_enabled: bool,
    loop_start: usize,
    loop_end: usize,
    lofi: LofiProcessor,
}

// ... (весь код Akai S900 эмулятора)
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