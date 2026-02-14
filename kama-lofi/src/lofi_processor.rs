use crate::config::{LofiConfig, ClassicSystem};
use crate::dsp;
use kama_core::{AudioNode, ParamValue, NodeMetadata, NodeCategory, AudioError};

pub struct LofiProcessor {
    config: LofiConfig,
    sample_rate: f32,
    time: f32,
    last_samples: Vec<f32>,
    sample_rate_buffer: Vec<f32>,
    temp_buffer: Vec<f32>,
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
        
        self.last_samples.push(sample);
        
        if self.config.enable_sr_reduction {
            let target_sr = self.config.system.get_sample_rate();
            let sr_factor = (self.sample_rate / target_sr).max(1.0);
            
            if sr_factor > 1.0 {
                if self.last_samples.len() >= sr_factor as usize {
                    sample = self.last_samples[0];
                    self.last_samples.clear();
                } else {
                    return if let Some(last) = self.last_samples.last() {
                        *last
                    } else {
                        sample
                    };
                }
            }
        }
        
        if self.config.enable_bitcrush || self.config.enable_noise {
            sample = dsp::process_lofi_chain(
                sample,
                self.config.system.get_bit_depth(),
                self.sample_rate / self.config.system.get_sample_rate(),
                &self.config.hardware,
                self.time,
            );
        }
        
        self.time += 1.0 / self.sample_rate;
        
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
        
        if self.temp_buffer.len() < buffer_size {
            self.temp_buffer.resize(buffer_size, 0.0);
        }
        
        for i in 0..buffer_size {
            self.temp_buffer[i] = self.process_sample(input[i]);
        }
        
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
                crate::node_params::bit_depth_param(self.config.system.get_bit_depth()),
                crate::node_params::sample_rate_param(self.config.system.get_sample_rate()),
                crate::node_params::dry_wet_param(self.config.dry_wet),
                crate::node_params::output_gain_param(self.config.output_gain),
                crate::node_params::enable_bitcrush_param(self.config.enable_bitcrush),
                crate::node_params::enable_sr_reduction_param(self.config.enable_sr_reduction),
            ],
        }
    }
}