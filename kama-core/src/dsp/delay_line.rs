use crate::node::{AudioNode, NodeMetadata, NodeCategory, NodeTypeId};
use crate::param::{ParamValue, ParamType};
use crate::AudioError;

/// Линия задержки с интерполяцией
pub struct DelayLine {
    buffer: Vec<f32>,
    write_pos: usize,
    delay_samples: f32,
    feedback: f32,
    wet_dry: f32,
    sample_rate: f32,
}

impl DelayLine {
    pub fn new(max_delay_seconds: f32, sample_rate: f32) -> Self {
        let buffer_size = (max_delay_seconds * sample_rate) as usize + 1;
        
        Self {
            buffer: vec![0.0; buffer_size],
            write_pos: 0,
            delay_samples: sample_rate * 0.5, // 500ms по умолчанию
            feedback: 0.5,
            wet_dry: 0.5,
            sample_rate,
        }
    }
    
    fn read_interpolated(&self, delay: f32) -> f32 {
        let buffer_len = self.buffer.len();
        let read_pos_f = self.write_pos as f32 - delay;
        
        if read_pos_f < 0.0 {
            return 0.0;
        }
        
        let read_pos = read_pos_f as usize % buffer_len;
        let frac = read_pos_f.fract();
        
        // Линейная интерполяция
        let idx1 = read_pos % buffer_len;
        let idx2 = (read_pos + 1) % buffer_len;
        
        let s1 = self.buffer[idx1];
        let s2 = self.buffer[idx2];
        
        s1 + frac * (s2 - s1)
    }
    
    fn write(&mut self, sample: f32) {
        let buffer_len = self.buffer.len();
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % buffer_len;
    }
}

impl AudioNode for DelayLine {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let input = inputs[0];
        let output = &mut outputs[0];
        
        for i in 0..input.len().min(output.len()) {
            // Прочитать задержанный сигнал
            let delayed = self.read_interpolated(self.delay_samples);
            
            // Смешать dry/wet
            let wet = delayed * self.wet_dry;
            let dry = input[i] * (1.0 - self.wet_dry);
            let mixed = wet + dry;
            
            // Записать в буфер с feedback
            let feedback_signal = delayed * self.feedback;
            self.write(input[i] + feedback_signal);
            
            output[i] = mixed;
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "delay" => Some(ParamValue::Float(self.delay_samples / self.sample_rate)),
            "feedback" => Some(ParamValue::Float(self.feedback)),
            "wet_dry" => Some(ParamValue::Float(self.wet_dry)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("delay", ParamValue::Float(d)) => {
                let delay_samples = d * self.sample_rate;
                self.delay_samples = delay_samples.max(0.0).min(self.buffer.len() as f32);
                Ok(())
            }
            ("feedback", ParamValue::Float(f)) => {
                self.feedback = f.max(0.0).min(1.0);
                Ok(())
            }
            ("wet_dry", ParamValue::Float(w)) => {
                self.wet_dry = w.max(0.0).min(1.0);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        // Можно пересоздать буфер если нужно
    }
    
    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
    }
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<DelayLine>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Delay Line".to_string(),
            category: NodeCategory::Effect,
            description: "Delay effect with interpolation".to_string(),
            author: "Kama Core".to_string(),
            version: "1.0".to_string(),
            parameters: vec![
                crate::node::ParamMetadata {
                    name: "delay".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.5),
                    min: Some(0.0),
                    max: Some(5.0),
                    step: Some(0.001),
                    unit: Some("seconds".to_string()),
                    choices: None,
                },
                crate::node::ParamMetadata {
                    name: "feedback".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.5),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("linear".to_string()),
                    choices: None,
                },
                crate::node::ParamMetadata {
                    name: "wet_dry".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.5),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("ratio".to_string()),
                    choices: None,
                },
            ],
        }
    }
}