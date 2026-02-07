use crate::node::{AudioNode, NodeMetadata, NodeCategory};
use crate::param::{ParamValue, ParamType};
use crate::AudioError;

/// Тип биквадратного фильтра
#[derive(Debug, Clone, Copy)]
pub enum BiquadType {
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

/// Биквадратный фильтр (Direct Form I)
pub struct BiquadFilter {
    filter_type: BiquadType,
    cutoff: f32,
    q: f32,
    sample_rate: f32,
    
    // Коэффициенты
    a0: f32, a1: f32, a2: f32,
    b0: f32, b1: f32, b2: f32,
    
    // Состояния
    x1: f32, x2: f32,
    y1: f32, y2: f32,
}

impl BiquadFilter {
    pub fn new(filter_type: BiquadType, cutoff: f32, q: f32) -> Self {
        Self {
            filter_type,
            cutoff,
            q,
            sample_rate: 44100.0,
            a0: 1.0, a1: 0.0, a2: 0.0,
            b0: 1.0, b1: 0.0, b2: 0.0,
            x1: 0.0, x2: 0.0,
            y1: 0.0, y2: 0.0,
        }
    }
    
    pub fn lowpass(cutoff: f32, q: f32) -> Self {
        Self::new(BiquadType::LowPass, cutoff, q)
    }
    
    pub fn highpass(cutoff: f32, q: f32) -> Self {
        Self::new(BiquadType::HighPass, cutoff, q)
    }
    
    fn update_coefficients(&mut self) {
        let omega = 2.0 * std::f32::consts::PI * self.cutoff / self.sample_rate;
        let alpha = omega.sin() / (2.0 * self.q);
        let cos_omega = omega.cos();
        
        match self.filter_type {
            BiquadType::LowPass => {
                self.b0 = (1.0 - cos_omega) / 2.0;
                self.b1 = 1.0 - cos_omega;
                self.b2 = self.b0;
                self.a0 = 1.0 + alpha;
                self.a1 = -2.0 * cos_omega;
                self.a2 = 1.0 - alpha;
            }
            BiquadType::HighPass => {
                self.b0 = (1.0 + cos_omega) / 2.0;
                self.b1 = -(1.0 + cos_omega);
                self.b2 = self.b0;
                self.a0 = 1.0 + alpha;
                self.a1 = -2.0 * cos_omega;
                self.a2 = 1.0 - alpha;
            }
            BiquadType::BandPass => {
                self.b0 = alpha;
                self.b1 = 0.0;
                self.b2 = -alpha;
                self.a0 = 1.0 + alpha;
                self.a1 = -2.0 * cos_omega;
                self.a2 = 1.0 - alpha;
            }
            BiquadType::Notch => {
                self.b0 = 1.0;
                self.b1 = -2.0 * cos_omega;
                self.b2 = 1.0;
                self.a0 = 1.0 + alpha;
                self.a1 = -2.0 * cos_omega;
                self.a2 = 1.0 - alpha;
            }
        }
        
        // Нормализовать коэффициенты
        self.b0 /= self.a0;
        self.b1 /= self.a0;
        self.b2 /= self.a0;
        self.a1 /= self.a0;
        self.a2 /= self.a0;
    }
}

impl AudioNode for BiquadFilter {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let input = inputs[0];
        let output = &mut outputs[0];
        
        for i in 0..input.len().min(output.len()) {
            let x = input[i];
            
            // Direct Form I
            let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1 - self.a2 * self.y2;
            
            output[i] = y;
            
            // Обновить состояния
            self.x2 = self.x1;
            self.x1 = x;
            self.y2 = self.y1;
            self.y1 = y;
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "cutoff" => Some(ParamValue::Float(self.cutoff)),
            "q" => Some(ParamValue::Float(self.q)),
            "type" => Some(ParamValue::String(
                match self.filter_type {
                    BiquadType::LowPass => "lowpass",
                    BiquadType::HighPass => "highpass",
                    BiquadType::BandPass => "bandpass",
                    BiquadType::Notch => "notch",
                }.to_string()
            )),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("cutoff", ParamValue::Float(c)) => {
                self.cutoff = c.max(20.0).min(self.sample_rate / 2.0);
                self.update_coefficients();
                Ok(())
            }
            ("q", ParamValue::Float(q)) => {
                self.q = q.max(0.1).min(10.0);
                self.update_coefficients();
                Ok(())
            }
            ("type", ParamValue::String(t)) => {
                self.filter_type = match t.as_str() {
                    "lowpass" => BiquadType::LowPass,
                    "highpass" => BiquadType::HighPass,
                    "bandpass" => BiquadType::BandPass,
                    "notch" => BiquadType::Notch,
                    _ => return Err(AudioError::Parameter(format!("Invalid filter type: {}", t))),
                };
                self.update_coefficients();
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_coefficients();
    }
    
    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Biquad Filter".to_string(),
            category: NodeCategory::Filter,
            description: "Second-order IIR filter".to_string(),
            author: "Kama Core".to_string(),
            version: "1.0".to_string(),
            parameters: vec![
                crate::node::ParamMetadata {
                    name: "cutoff".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1000.0),
                    min: Some(20.0),
                    max: Some(20000.0),
                    step: Some(1.0),
                    unit: Some("Hz".to_string()),
                    choices: None,
                },
                crate::node::ParamMetadata {
                    name: "q".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.707),
                    min: Some(0.1),
                    max: Some(10.0),
                    step: Some(0.01),
                    unit: Some("Q".to_string()),
                    choices: None,
                },
                crate::node::ParamMetadata {
                    name: "type".to_string(),
                    typ: ParamType::String,
                    default: ParamValue::String("lowpass".to_string()),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: Some(vec![
                        ("lowpass".to_string(), 0.0),
                        ("highpass".to_string(), 1.0),
                        ("bandpass".to_string(), 2.0),
                        ("notch".to_string(), 3.0),
                    ]),
                },
            ],
        }
    }
}